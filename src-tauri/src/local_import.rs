use crate::AppState;
use crate::db;
use crate::model::ImportReport;
use std::collections::HashMap;
use std::io::Read;
use tauri::{AppHandle, Emitter, Manager};
use tiny_http::{Header, Request, Response, StatusCode};

/// Fixed loopback port. Vite uses 1420, and VRCVideoCacher uses 9696.
pub const LOCAL_IMPORT_PORT: u16 = 17321;
pub const AUTH_FAIL_LIMIT: u32 = 5;
pub const AUTH_LOCK_SECS: u64 = 30;
const MAX_BODY: usize = 16 * 1024 * 1024;
const TOKEN_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn bind_addr() -> String {
    format!("127.0.0.1:{LOCAL_IMPORT_PORT}")
}

pub fn generate_token() -> String {
    let mut chars = Vec::with_capacity(8);
    let mut buf = [0u8; 8];
    getrandom::fill(&mut buf).expect("OS entropy");
    for byte in buf {
        chars.push(TOKEN_ALPHABET[(byte % 32) as usize] as char);
    }
    chars.into_iter().collect()
}

pub fn stored_token_ok(value: &str) -> bool {
    let normalized = normalize_token(value);
    normalized.len() == 8
        && normalized
            .bytes()
            .all(|byte| TOKEN_ALPHABET.contains(&byte))
}

pub fn display_token(stored: &str) -> String {
    let normalized = normalize_token(stored);
    if normalized.len() == 8 {
        format!("{}-{}", &normalized[..4], &normalized[4..])
    } else {
        normalized
    }
}

pub fn start_receiver(app: AppHandle) -> Result<(), String> {
    let server = tiny_http::Server::http(bind_addr())
        .map_err(|err| format!("拡張機能との接続を開始できません。アプリを再起動してください: {err}"))?;
    std::thread::Builder::new()
        .name("vrclibman-import".into())
        .spawn(move || {
            for mut request in server.incoming_requests() {
                let response = respond(&app, &mut request);
                let _ = request.respond(response);
            }
        })
        .map_err(|err| format!("拡張機能との接続を開始できません。アプリを再起動してください: {err}"))?;
    Ok(())
}

fn respond(app: &AppHandle, request: &mut Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let method = request.method().as_str().to_string();
    let path = request.url().to_string();
    let origin = header(request, "Origin").map(str::to_string);
    let authorization = header(request, "Authorization").map(str::to_string);
    let content_length =
        header(request, "Content-Length").and_then(|value| value.parse::<usize>().ok());
    let view = RequestView {
        method: &method,
        path: &path,
        origin: origin.as_deref(),
        authorization: authorization.as_deref(),
    };
    if !view.origin.is_some_and(is_extension_origin) {
        discard_body(request);
        return response_for(
            origin.as_deref(),
            Gate::reject(403, "VRCLibMan の拡張機能から接続してください"),
            None,
        );
    }
    if view.method.eq_ignore_ascii_case("OPTIONS") {
        discard_body(request);
        return response_for(origin.as_deref(), Gate::Preflight, None);
    }
    let state = app.state::<AppState>();
    let (expected, pinned) = match state.db.lock() {
        Ok(connection) => match (
            db::extension_token(&connection),
            db::extension_origin(&connection),
        ) {
            (Ok(token), Ok(pinned)) => (token, pinned),
            (Err(err), _) | (_, Err(err)) => {
                discard_body(request);
                return response_for(
                    origin.as_deref(),
                    Gate::Reject {
                        status: 500,
                        message: "",
                        message_owned: Some(err),
                    },
                    None,
                );
            }
        },
        Err(_) => {
            discard_body(request);
            return response_for(
                origin.as_deref(),
                Gate::reject(500, "データベースをロックできません"),
                None,
            );
        }
    };
    let origin_key = origin.clone().unwrap_or_default();
    let now = now_ms();
    let locked = state
        .import_guard
        .lock()
        .map(|mut guard| guard.is_locked(&origin_key, now))
        .unwrap_or(false);
    let gate = classify(
        view,
        Access {
            token: &expected,
            pinned: pinned.as_deref(),
            locked,
        },
    );
    if matches!(gate, Gate::Reject { status: 401, .. }) {
        if let Ok(mut guard) = state.import_guard.lock() {
            guard.record_failure(&origin_key, now);
        }
    }
    if matches!(
        gate,
        Gate::Health
            | Gate::Import
            | Gate::KnownIds
            | Gate::DownloadFinished
            | Gate::RouteRead
            | Gate::RouteWrite
    ) {
        if let Ok(mut guard) = state.import_guard.lock() {
            guard.record_success(&origin_key);
        }
        if let Ok(connection) = state.db.lock() {
            let _ = crate::integration::record_extension_seen(
                &connection, header(request, "X-VRCLibMan-Extension-Version"),
            );
        }
        if pinned.is_none() {
            if let Ok(connection) = state.db.lock() {
                let _ = db::set_extension_origin(&connection, &origin_key);
            }
        }
    }
    if matches!(gate, Gate::KnownIds) {
        discard_body(request);
        let ids = {
            let connection = match state.db.lock() {
                Ok(connection) => connection,
                Err(_) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::reject(500, "データベースをロックできません"),
                        None,
                    );
                }
            };
            match db::known_booth_ids(&connection) {
                Ok(ids) => ids,
                Err(err) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::Reject {
                            status: 500,
                            message: "",
                            message_owned: Some(err),
                        },
                        None,
                    );
                }
            }
        };
        let body = serde_json::json!({ "ids": ids }).to_string();
        return cors_json(origin.as_deref(), 200, body);
    }
    if matches!(gate, Gate::RouteRead) {
        discard_body(request);
        let enabled = {
            let connection = match state.db.lock() {
                Ok(connection) => connection,
                Err(_) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::reject(500, "データベースをロックできません"),
                        None,
                    );
                }
            };
            match db::settings(&connection) {
                Ok(settings) => settings.route_downloads,
                Err(err) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::Reject {
                            status: 500,
                            message: "",
                            message_owned: Some(err),
                        },
                        None,
                    );
                }
            }
        };
        return cors_json(
            origin.as_deref(),
            200,
            serde_json::json!({ "enabled": enabled }).to_string(),
        );
    }
    if !matches!(gate, Gate::Import | Gate::DownloadFinished | Gate::RouteWrite) {
        discard_body(request);
        return response_for(origin.as_deref(), gate, None);
    }
    if content_length.is_some_and(|length| length > MAX_BODY) {
        return response_for(
            origin.as_deref(),
            Gate::reject(413, "JSONが大きすぎます"),
            None,
        );
    }
    let body = match read_limited(request.as_reader(), MAX_BODY) {
        Ok(body) => body,
        Err(()) => {
            return response_for(
                origin.as_deref(),
                Gate::reject(413, "JSONが大きすぎます"),
                None,
            );
        }
    };
    let text = match String::from_utf8(body) {
        Ok(text) => text,
        Err(_) => {
            return response_for(
                origin.as_deref(),
                Gate::reject(400, "JSONを読めません"),
                None,
            );
        }
    };
    if matches!(gate, Gate::RouteWrite) {
        let enabled = match serde_json::from_str::<RouteUpdate>(&text) {
            Ok(update) => update.enabled,
            Err(_) => {
                return response_for(
                    origin.as_deref(),
                    Gate::reject(400, "設定を読めません"),
                    None,
                );
            }
        };
        let saved = {
            let connection = match state.db.lock() {
                Ok(connection) => connection,
                Err(_) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::reject(500, "データベースをロックできません"),
                        None,
                    );
                }
            };
            match db::settings(&connection).and_then(|settings| {
                db::save_settings(
                    &connection,
                    &crate::model::SettingsInput {
                        ollama_base_url: settings.ollama_base_url,
                        ollama_model: settings.ollama_model,
                        confidence_threshold: settings.confidence_threshold,
                        library_folder: settings.library_folder,
                        builtin_browser: settings.builtin_browser,
                        remote_library_folder: settings.remote_library_folder,
                        route_downloads: enabled,
                        llm_backend: settings.llm_backend,
                        background_mode: settings.background_mode,
                        launch_at_login: settings.launch_at_login,
                    },
                )
            }) {
                Ok(()) => enabled,
                Err(err) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::Reject {
                            status: 500,
                            message: "",
                            message_owned: Some(err),
                        },
                        None,
                    );
                }
            }
        };
        return cors_json(
            origin.as_deref(),
            200,
            serde_json::json!({ "enabled": saved }).to_string(),
        );
    }
    if matches!(gate, Gate::DownloadFinished) {
        let notice = match serde_json::from_str::<crate::model::DownloadFinished>(&text) {
            Ok(notice) => notice,
            Err(_) => {
                return response_for(
                    origin.as_deref(),
                    Gate::reject(400, "ダウンロードの通知を読めません"),
                    None,
                );
            }
        };
        let plan = {
            let mut connection = match state.db.lock() {
                Ok(connection) => connection,
                Err(_) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::reject(500, "データベースをロックできません"),
                        None,
                    );
                }
            };
            match db::prepare_download(&mut connection, &notice) {
                Ok(plan) => plan,
                Err(err) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::Reject {
                            status: 400,
                            message: "",
                            message_owned: Some(err),
                        },
                        None,
                    );
                }
            }
        };
        let placed = match crate::library_files::place_into_library(
            &plan.source,
            plan.library.as_deref(),
            &plan.booth_id,
        ) {
            Ok(path) => path,
            Err(err) => {
                return response_for(
                    origin.as_deref(),
                    Gate::Reject {
                        status: 400,
                        message: "",
                        message_owned: Some(err),
                    },
                    None,
                );
            }
        };
        let recorded = {
            let connection = match state.db.lock() {
                Ok(connection) => connection,
                Err(_) => {
                    return response_for(
                        origin.as_deref(),
                        Gate::reject(500, "データベースをロックできません"),
                        None,
                    );
                }
            };
            db::record_placed_download(
                &connection,
                &plan.booth_id,
                &plan.variation,
                &placed,
                notice.size,
            )
        };
        return match recorded {
            Ok(()) => {
                let _ = app.emit("library-changed", ());
                cors_json(
                    origin.as_deref(),
                    200,
                    serde_json::json!({
                        "ok": true,
                        "boothId": plan.booth_id,
                        "path": placed.display().to_string(),
                    })
                    .to_string(),
                )
            }
            Err(err) => response_for(
                origin.as_deref(),
                Gate::Reject {
                    status: 500,
                    message: "",
                    message_owned: Some(err),
                },
                None,
            ),
        };
    }
    let imported = {
        let mut connection = match state.db.lock() {
            Ok(connection) => connection,
            Err(_) => {
                return response_for(
                    origin.as_deref(),
                    Gate::reject(500, "データベースをロックできません"),
                    None,
                );
            }
        };
        let imported = db::import_export_json(&mut connection, &text);
        if let Ok(ref report) = imported {
            let _ = crate::integration::record_import(&connection, "extension", report);
        }
        imported
    };
    match imported {
        Ok(report) => {
            let _ = app.emit("library-changed", ());
            response_for(origin.as_deref(), Gate::Import, Some(report))
        }
        Err(err) => response_for(
            origin.as_deref(),
            Gate::Reject {
                status: 400,
                message_owned: Some(err),
                message: "",
            },
            None,
        ),
    }
}

#[derive(serde::Deserialize)]
struct RouteUpdate {
    enabled: bool,
}

fn cors_json(
    origin: Option<&str>,
    status: u16,
    body: String,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_string(body).with_status_code(StatusCode(status));
    if let Some(origin) = origin.filter(|origin| is_extension_origin(origin)) {
        response = add_cors(response, origin);
    }
    response
}

struct RequestView<'a> {
    method: &'a str,
    path: &'a str,
    origin: Option<&'a str>,
    authorization: Option<&'a str>,
}

enum Gate {
    Preflight,
    Health,
    Import,
    KnownIds,
    DownloadFinished,
    RouteRead,
    RouteWrite,
    Reject {
        status: u16,
        message: &'static str,
        message_owned: Option<String>,
    },
}

impl Gate {
    fn reject(status: u16, message: &'static str) -> Self {
        Self::Reject {
            status,
            message,
            message_owned: None,
        }
    }
}

struct Access<'a> {
    token: &'a str,
    pinned: Option<&'a str>,
    locked: bool,
}

fn classify(request: RequestView<'_>, access: Access<'_>) -> Gate {
    let Some(origin) = request.origin.filter(|origin| is_extension_origin(origin)) else {
        return Gate::reject(403, "VRCLibMan の拡張機能から接続してください");
    };
    if access.pinned.is_some_and(|pinned| pinned != origin) {
        return Gate::reject(
            403,
            "別の拡張機能と接続済みです。アプリの設定でペアリングコードを作り直して接続してください",
        );
    }
    if request.method.eq_ignore_ascii_case("OPTIONS") {
        return Gate::Preflight;
    }
    if access.locked {
        return Gate::reject(
            429,
            "接続の試行が多すぎます。30 秒待ってからもう一度お試しください",
        );
    }
    if !token_ok(request.authorization, access.token) {
        return Gate::reject(
            401,
            "ペアリングコードを確認し、拡張機能に入力し直してください",
        );
    }
    let path = request.path.split('?').next().unwrap_or("");
    if request.method.eq_ignore_ascii_case("GET") && path == "/health" {
        return Gate::Health;
    }
    if request.method.eq_ignore_ascii_case("POST") && path == "/import" {
        return Gate::Import;
    }
    if request.method.eq_ignore_ascii_case("GET") && path == "/known-ids" {
        return Gate::KnownIds;
    }
    if request.method.eq_ignore_ascii_case("POST") && path == "/download-finished" {
        return Gate::DownloadFinished;
    }
    if request.method.eq_ignore_ascii_case("GET") && path == "/route-downloads" {
        return Gate::RouteRead;
    }
    if request.method.eq_ignore_ascii_case("POST") && path == "/route-downloads" {
        return Gate::RouteWrite;
    }
    if !matches!(request.method.to_ascii_uppercase().as_str(), "GET" | "POST") {
        return Gate::reject(
            405,
            "この操作には対応していません。アプリと拡張機能を更新してください",
        );
    }
    Gate::reject(
        404,
        "この操作には対応していません。アプリと拡張機能を更新してください",
    )
}

fn token_ok(authorization: Option<&str>, expected: &str) -> bool {
    let Some(provided) = bearer_token(authorization) else {
        return false;
    };
    tokens_match(&provided, expected)
}

fn response_for(
    origin: Option<&str>,
    gate: Gate,
    report: Option<ImportReport>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let (status, body) = match gate {
        Gate::Preflight => (204, String::new()),
        Gate::Health => (200, r#"{"ok":true}"#.to_string()),
        Gate::KnownIds => (200, r#"{"ids":[]}"#.to_string()),
        Gate::DownloadFinished | Gate::RouteRead | Gate::RouteWrite => (200, "{}".to_string()),
        Gate::Import => (
            200,
            serde_json::to_string(&report.expect("import report")).unwrap_or_else(|_| "{}".into()),
        ),
        Gate::Reject {
            status,
            message,
            message_owned,
        } => {
            let text = message_owned.unwrap_or_else(|| message.to_string());
            (status, serde_json::json!({ "error": text }).to_string())
        }
    };
    let mut response = Response::from_string(body).with_status_code(StatusCode(status));
    if status == 429 {
        if let Ok(header) =
            Header::from_bytes(b"Retry-After", AUTH_LOCK_SECS.to_string().as_bytes())
        {
            response = response.with_header(header);
        }
    }
    if origin.is_some_and(is_extension_origin) {
        if let Some(origin) = origin {
            response = add_cors(response, origin);
        }
    }
    response
}

fn add_cors(
    response: Response<std::io::Cursor<Vec<u8>>>,
    origin: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = response;
    for (name, value) in [
        ("Access-Control-Allow-Origin", origin),
        (
            "Access-Control-Allow-Headers",
            "authorization, content-type, x-vrclibman-extension-version",
        ),
        ("Access-Control-Allow-Methods", "GET, POST, OPTIONS"),
        ("Access-Control-Allow-Private-Network", "true"),
        ("Vary", "Origin"),
        ("Cache-Control", "no-store"),
    ] {
        if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response = response.with_header(header);
        }
    }
    response
}

fn discard_body(request: &mut Request) {
    let _ = read_limited(request.as_reader(), MAX_BODY);
}

fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str())
}

fn read_limited(reader: &mut dyn Read, limit: usize) -> Result<Vec<u8>, ()> {
    let mut body = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        if body.len() + read > limit {
            return Err(());
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok(body)
}

pub fn is_extension_origin(origin: &str) -> bool {
    let Some(id) = origin.strip_prefix("chrome-extension://") else {
        return false;
    };
    id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte))
}

fn bearer_token(authorization: Option<&str>) -> Option<String> {
    let value = authorization?.trim();
    let token = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn normalize_token(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_uppercase())
        .collect()
}

#[derive(Debug, Default)]
pub struct ImportGuard {
    failures: HashMap<String, OriginFailures>,
}

#[derive(Debug, Default)]
struct OriginFailures {
    count: u32,
    locked_until_ms: Option<u64>,
}

impl ImportGuard {
    pub fn clear(&mut self) {
        self.failures.clear();
    }

    pub fn is_locked(&mut self, origin: &str, now_ms: u64) -> bool {
        let Some(entry) = self.failures.get_mut(origin) else {
            return false;
        };
        match entry.locked_until_ms {
            Some(until) if now_ms < until => true,
            Some(_) => {
                entry.count = 0;
                entry.locked_until_ms = None;
                false
            }
            None => false,
        }
    }

    pub fn record_failure(&mut self, origin: &str, now_ms: u64) {
        if self.is_locked(origin, now_ms) {
            return;
        }
        let entry = self.failures.entry(origin.to_string()).or_default();
        entry.count += 1;
        if entry.count >= AUTH_FAIL_LIMIT {
            entry.locked_until_ms = Some(now_ms.saturating_add(AUTH_LOCK_SECS * 1000));
        }
    }

    pub fn record_success(&mut self, origin: &str) {
        self.failures.remove(origin);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn tokens_match(provided: &str, expected: &str) -> bool {
    let provided = normalize_token(provided);
    let expected = normalize_token(expected);
    if expected.is_empty() {
        return false;
    }
    let mut diff = u8::from(provided.len() != expected.len());
    let length = provided.len().max(expected.len());
    let provided = provided.as_bytes();
    let expected = expected.as_bytes();
    for index in 0..length {
        let left = provided.get(index).copied().unwrap_or(0);
        let right = expected.get(index).copied().unwrap_or(0);
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    const EXTENSION: &str = "chrome-extension://abcdefghijklmnopabcdefghijklmnop";

    fn view<'a>(
        method: &'a str,
        path: &'a str,
        origin: Option<&'a str>,
        authorization: Option<&'a str>,
    ) -> RequestView<'a> {
        RequestView {
            method,
            path,
            origin,
            authorization,
        }
    }

    #[test]
    fn receiver_uses_fixed_loopback_port() {
        assert_eq!(bind_addr(), "127.0.0.1:17321");
        assert_ne!(LOCAL_IMPORT_PORT, 1420);
        assert_ne!(LOCAL_IMPORT_PORT, 9696);
    }

    #[test]
    fn token_and_origin_gates() {
        let expected = "ABCD2345";
        let gate = |method, path, origin, authorization| {
            classify(
                view(method, path, origin, authorization),
                Access {
                    token: expected,
                    pinned: None,
                    locked: false,
                },
            )
        };
        assert!(matches!(
            gate("GET", "/health", None, Some("Bearer ABCD2345")),
            Gate::Reject { status: 403, .. }
        ));
        assert!(matches!(
            gate(
                "POST",
                "/import",
                Some("https://booth.pm"),
                Some("Bearer ABCD2345")
            ),
            Gate::Reject { status: 403, .. }
        ));
        assert!(matches!(
            gate("POST", "/import", Some("null"), Some("Bearer ABCD2345")),
            Gate::Reject { status: 403, .. }
        ));
        assert!(matches!(
            gate(
                "GET",
                "/health",
                Some("chrome-extension://not-an-id"),
                Some("Bearer ABCD2345")
            ),
            Gate::Reject { status: 403, .. }
        ));
        assert!(matches!(
            gate("OPTIONS", "/import", Some("https://evil.example"), None),
            Gate::Reject { status: 403, .. }
        ));
        assert!(matches!(
            gate("OPTIONS", "/import", Some(EXTENSION), None),
            Gate::Preflight
        ));
        assert!(matches!(
            gate("GET", "/health", Some(EXTENSION), None),
            Gate::Reject { status: 401, .. }
        ));
        assert!(matches!(
            gate("GET", "/health", Some(EXTENSION), Some("Bearer nope")),
            Gate::Reject { status: 401, .. }
        ));
        assert!(matches!(
            gate("GET", "/health", Some(EXTENSION), Some("Bearer ABCD-2345")),
            Gate::Health
        ));
        assert!(matches!(
            gate("POST", "/import", Some(EXTENSION), None),
            Gate::Reject { status: 401, .. }
        ));
        assert!(matches!(
            gate("POST", "/import", Some(EXTENSION), Some("Bearer ABCD-2345")),
            Gate::Import
        ));
        assert!(matches!(
            gate("GET", "/known-ids", Some(EXTENSION), Some("Bearer ABCD-2345")),
            Gate::KnownIds
        ));
        assert!(matches!(
            gate(
                "POST",
                "/download-finished",
                Some(EXTENSION),
                Some("Bearer ABCD-2345")
            ),
            Gate::DownloadFinished
        ));
        assert!(matches!(
            gate(
                "GET",
                "/route-downloads",
                Some(EXTENSION),
                Some("Bearer ABCD-2345")
            ),
            Gate::RouteRead
        ));
        assert!(matches!(
            gate(
                "POST",
                "/route-downloads",
                Some(EXTENSION),
                Some("Bearer ABCD-2345")
            ),
            Gate::RouteWrite
        ));
        assert!(matches!(
            gate("POST", "/download-finished", Some(EXTENSION), None),
            Gate::Reject { status: 401, .. }
        ));
        assert!(matches!(
            gate("GET", "/known-ids", Some(EXTENSION), None),
            Gate::Reject { status: 401, .. }
        ));
        assert!(token_ok(Some("Bearer ABCD-2345"), expected));
        assert!(token_ok(Some("bearer abcd2345"), expected));
        assert!(!token_ok(None, expected));
        assert!(!token_ok(Some("Bearer XXXX9999"), expected));
        assert!(!token_ok(Some("ABCD2345"), expected));
        assert!(matches!(
            gate(
                "DELETE",
                "/import",
                Some(EXTENSION),
                Some("Bearer ABCD2345")
            ),
            Gate::Reject { status: 405, .. }
        ));
    }

    #[test]
    fn first_successful_origin_is_pinned_until_the_code_changes() {
        const OTHER: &str = "chrome-extension://pppppppppppppppppppppppppppppppp";
        let allow = |origin: &str, pinned: Option<&str>, locked: bool| {
            classify(
                view("GET", "/health", Some(origin), Some("Bearer ABCD-2345")),
                Access {
                    token: "ABCD2345",
                    pinned,
                    locked,
                },
            )
        };
        assert!(matches!(allow(EXTENSION, None, false), Gate::Health));
        assert!(matches!(allow(OTHER, None, false), Gate::Health));
        assert!(matches!(
            allow(OTHER, Some(EXTENSION), false),
            Gate::Reject { status: 403, .. }
        ));
        assert!(matches!(
            allow(EXTENSION, Some(EXTENSION), false),
            Gate::Health
        ));
        assert!(matches!(
            allow(EXTENSION, Some(EXTENSION), true),
            Gate::Reject { status: 429, .. }
        ));

        let connection = temp_db();
        assert!(db::extension_origin(&connection).unwrap().is_none());
        db::set_extension_origin(&connection, EXTENSION).unwrap();
        assert_eq!(
            db::extension_origin(&connection).unwrap().as_deref(),
            Some(EXTENSION)
        );
        db::regenerate_extension_token(&connection).unwrap();
        assert!(db::extension_origin(&connection).unwrap().is_none());
    }

    #[test]
    fn repeated_401s_lock_that_origin_for_a_short_time() {
        let mut guard = ImportGuard::default();
        let now = 1_000_000_u64;
        assert!(!guard.is_locked(EXTENSION, now));
        for _ in 0..AUTH_FAIL_LIMIT - 1 {
            guard.record_failure(EXTENSION, now);
            assert!(!guard.is_locked(EXTENSION, now));
        }
        guard.record_failure(EXTENSION, now);
        assert!(guard.is_locked(EXTENSION, now));
        assert!(guard.is_locked(EXTENSION, now + AUTH_LOCK_SECS * 1000 - 1));
        assert!(!guard.is_locked(EXTENSION, now + AUTH_LOCK_SECS * 1000));
        guard.record_success(EXTENSION);
        assert!(!guard.is_locked(EXTENSION, now));
        guard.record_failure(EXTENSION, now);
        guard.clear();
        assert!(!guard.is_locked(EXTENSION, now));
    }

    #[test]
    fn payload_import_matches_file_import() {
        let json = r#"{
            "format": "vrclibman.booth-export",
            "version": 1,
            "source": "library",
            "products": [
                {"boothId":"2468135","title":"架空の服","shop":"工房","descriptionSnippet":"架空ちゃん対応の衣装","url":"https://booth.pm/ja/items/2468135"}
            ]
        }"#;
        let mut direct = temp_db();
        let mut received = temp_db();
        let from_file = db::import_export_json(&mut direct, json).unwrap();
        let from_receiver = db::import_export_json(&mut received, json).unwrap();
        assert_eq!(from_file, from_receiver);
        assert_eq!(from_receiver.inserted, 1);
        assert_eq!(row(&direct, "2468135"), row(&received, "2468135"));
        let (needs_review, category, extracted_at) = row(&received, "2468135");
        assert_eq!(needs_review, 1);
        assert!(category.is_none());
        assert!(extracted_at.is_none());
    }

    #[test]
    fn live_receiver_checks_origin_and_token_then_imports() {
        let dir = temp_dir();
        let connection = db::open(&dir.0.join("library.sqlite")).unwrap();
        let token = db::extension_token(&connection).unwrap();
        drop(connection);
        let imported = Arc::new(Mutex::new(Vec::<String>::new()));
        let imported_thread = imported.clone();
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        // Windows builds of tiny_http only have ListenAddr::IP, so the binding is irrefutable there.
        #[cfg(not(unix))]
        let tiny_http::ListenAddr::IP(addr) = server.server_addr();
        #[cfg(unix)]
        let addr = match server.server_addr() {
            tiny_http::ListenAddr::IP(addr) => addr,
            tiny_http::ListenAddr::Unix(_) => panic!("expected an ip listener"),
        };
        let expected = token.clone();
        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let method = request.method().as_str().to_string();
                let path = request.url().to_string();
                let origin = header(&request, "Origin").map(str::to_string);
                let authorization = header(&request, "Authorization").map(str::to_string);
                let gate = classify(
                    RequestView {
                        method: &method,
                        path: &path,
                        origin: origin.as_deref(),
                        authorization: authorization.as_deref(),
                    },
                    Access {
                        token: &expected,
                        pinned: None,
                        locked: false,
                    },
                );
                let response = if matches!(gate, Gate::Import) {
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).unwrap();
                    let mut connection = db::open(&dir.0.join("library.sqlite")).unwrap();
                    match db::import_export_json(&mut connection, &body) {
                        Ok(report) => {
                            imported_thread.lock().unwrap().push(body);
                            response_for(origin.as_deref(), Gate::Import, Some(report))
                        }
                        Err(err) => response_for(
                            origin.as_deref(),
                            Gate::Reject {
                                status: 400,
                                message: "",
                                message_owned: Some(err),
                            },
                            None,
                        ),
                    }
                } else {
                    response_for(origin.as_deref(), gate, None)
                };
                let _ = request.respond(response);
            }
        });

        let (status, _) = exchange(
            addr,
            "GET",
            "/health",
            Some("https://booth.pm"),
            Some(&format!("Bearer {token}")),
            "",
        );
        assert_eq!(status, 403);

        let (status, _) = exchange(addr, "GET", "/health", Some(EXTENSION), None, "");
        assert_eq!(status, 401);
        let (status, body) = exchange(
            addr,
            "GET",
            "/health",
            Some(EXTENSION),
            Some(&format!("Bearer {}", display_token(&token))),
            "",
        );
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("\"ok\":true"), "{body}");

        let (status, _) = exchange(
            addr,
            "POST",
            "/import",
            Some(EXTENSION),
            Some("Bearer NOPE9999"),
            "{}",
        );
        assert_eq!(status, 401);

        let payload = r#"{"format":"vrclibman.booth-export","version":1,"products":[{"boothId":"1357246","title":"届いた服","descriptionSnippet":"説明"}]}"#;
        let (status, body) = exchange(
            addr,
            "POST",
            "/import",
            Some(EXTENSION),
            Some(&format!("Bearer {}", display_token(&token))),
            payload,
        );
        assert_eq!(status, 200, "{body}");
        assert!(body.contains("\"inserted\":1"), "{body}");
        assert_eq!(imported.lock().unwrap().len(), 1);
    }

    fn exchange(
        addr: SocketAddr,
        method: &str,
        path: &str,
        origin: Option<&str>,
        authorization: Option<&str>,
        body: &str,
    ) -> (u16, String) {
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        if let Some(origin) = origin {
            request.push_str(&format!("Origin: {origin}\r\n"));
        }
        if let Some(authorization) = authorization {
            request.push_str(&format!("Authorization: {authorization}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(body);
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            }
        }
        let text = String::from_utf8_lossy(&bytes).to_string();
        let status = text
            .split_whitespace()
            .nth(1)
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    struct Temp(std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir() -> Temp {
        let path = std::env::temp_dir().join(format!(
            "vrclibman-link-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Temp(path)
    }

    fn temp_db() -> rusqlite::Connection {
        let dir = temp_dir();
        let connection = db::open(&dir.0.join("library.sqlite")).unwrap();
        std::mem::forget(dir);
        connection
    }

    fn row(
        connection: &rusqlite::Connection,
        booth_id: &str,
    ) -> (i64, Option<String>, Option<String>) {
        connection
            .query_row(
                "SELECT needs_review, category, extracted_at FROM products WHERE booth_id = ?1",
                [booth_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }
}
