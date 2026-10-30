//! Classification through an OpenAI-compatible chat completions API (opt-in).
//!
//! The prompt, parser and guardrails are the ones the local path uses. The API key
//! lives only in the OS credential store; it is never written to SQLite, logs,
//! exports, or error strings. Error strings carry only a fixed kind and the HTTP
//! status, never the response body (some providers echo part of the key there).

use crate::model::{CloudTestResult, Extraction};
use regex::Regex;
use serde_json::{Value, json};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Semaphore;
use url::Url;

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
/// Account name inside the OS credential store. The service is the app identifier.
pub const KEY_ACCOUNT: &str = "cloud-classifier-api-key";
/// Requests to the provider that may be outstanding at the same time.
pub const MAX_IN_FLIGHT: usize = 2;
/// Retries after HTTP 429 before the product is counted as failed.
pub const MAX_RATE_LIMIT_RETRIES: u32 = 3;
/// A longer Retry-After stops instead of waiting.
const MAX_RETRY_AFTER_SECS: f64 = 60.0;

/// API key held in memory only while a request is built. `Debug` never shows it.
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudError {
    MissingKey,
    MissingModel,
    Unauthorized,
    Forbidden,
    NotFound,
    RateLimited,
    Server(u16),
    BadRequest(u16),
    Timeout,
    Network,
    InvalidResponse,
    EmptyResponse,
    KeyStore,
}

impl CloudError {
    /// Kind written to `booth-browser.log`.
    pub fn kind(self) -> &'static str {
        match self {
            Self::MissingKey => "キー未保存",
            Self::MissingModel => "モデル未設定",
            Self::Unauthorized => "認証失敗",
            Self::Forbidden => "権限なし",
            Self::NotFound => "モデルなし",
            Self::RateLimited => "回数制限",
            Self::Server(_) => "サーバーエラー",
            Self::BadRequest(_) => "依頼エラー",
            Self::Timeout => "タイムアウト",
            Self::Network => "接続失敗",
            Self::InvalidResponse => "不正な出力",
            Self::EmptyResponse => "空応答",
            Self::KeyStore => "キー保存先",
        }
    }

    pub fn message(self) -> String {
        match self {
            Self::MissingKey => "クラウドAPIのキーが保存されていません。設定で保存してください".into(),
            Self::MissingModel => "クラウドAPIのモデル名が設定されていません".into(),
            Self::Unauthorized => {
                "クラウドAPIの認証に失敗しました（401）。APIキーを確認してください".into()
            }
            Self::Forbidden => {
                "クラウドAPIに拒否されました（403）。キーの権限とモデルの利用可否を確認してください"
                    .into()
            }
            Self::NotFound => {
                "クラウドAPIにモデルまたは接続先がありません（404）。接続先とモデル名を確認してください"
                    .into()
            }
            Self::RateLimited => {
                "クラウドAPIの回数制限に達しました（429）。時間をおいて再度分類してください".into()
            }
            Self::Server(code) => format!("クラウドAPIのサーバーエラーです（{code}）"),
            Self::BadRequest(code) => format!("クラウドAPIが依頼を受け付けませんでした（{code}）"),
            Self::Timeout => "クラウドAPIの応答がタイムアウトしました".into(),
            Self::Network => {
                "クラウドAPIに接続できません。ネットワークと接続先を確認してください".into()
            }
            Self::InvalidResponse => "クラウドAPIの応答を読めません".into(),
            Self::EmptyResponse => "クラウドAPIが空の応答を返しました".into(),
            Self::KeyStore => "クラウドAPIのキーを OS の資格情報から読めません".into(),
        }
    }
}

const ALL_KINDS: [CloudError; 13] = [
    CloudError::MissingKey,
    CloudError::MissingModel,
    CloudError::Unauthorized,
    CloudError::Forbidden,
    CloudError::NotFound,
    CloudError::RateLimited,
    CloudError::Server(500),
    CloudError::BadRequest(400),
    CloudError::Timeout,
    CloudError::Network,
    CloudError::InvalidResponse,
    CloudError::EmptyResponse,
    CloudError::KeyStore,
];

/// Recovers the kind from a message built by [`CloudError::message`].
pub fn failure_kind(message: &str) -> Option<&'static str> {
    if !message.starts_with("クラウドAPI") {
        return None;
    }
    ALL_KINDS.iter().find_map(|error| {
        let text = error.message();
        let head = text.split('（').next().unwrap_or(&text);
        message.starts_with(head).then(|| error.kind())
    })
}

pub fn status_error(status: u16) -> CloudError {
    match status {
        401 => CloudError::Unauthorized,
        403 => CloudError::Forbidden,
        404 => CloudError::NotFound,
        429 => CloudError::RateLimited,
        500..=599 => CloudError::Server(status),
        _ => CloudError::BadRequest(status),
    }
}

fn send_error(err: &reqwest::Error) -> CloudError {
    if err.is_timeout() {
        CloudError::Timeout
    } else {
        CloudError::Network
    }
}

pub fn validate_base_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("クラウドAPIの接続先を入力してください".into());
    }
    let url = Url::parse(trimmed).map_err(|_| "クラウドAPIの接続先 URL が正しくありません".to_string())?;
    let host = url.host_str().unwrap_or("");
    let loopback = matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1");
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => return Err("クラウドAPIの接続先は https にしてください".into()),
    }
    if host.is_empty() {
        return Err("クラウドAPIの接続先 URL が正しくありません".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("クラウドAPIの接続先に認証情報は入れられません。キーは別欄に保存します".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("クラウドAPIの接続先に ? や # は入れられません".into());
    }
    Ok(trimmed.to_string())
}

/// Free text, because providers name models differently. Empty is allowed until use.
pub fn validate_model_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.chars().count() > 200 {
        return Err("クラウドAPIのモデル名が長すぎます".into());
    }
    if name.chars().any(char::is_control) {
        return Err("クラウドAPIのモデル名に使えない文字があります".into());
    }
    Ok(name.to_string())
}

/// Never echoes the value back in the error.
pub fn validate_api_key(value: &str) -> Result<String, String> {
    let key = value.trim();
    if key.is_empty() {
        return Err("APIキーを入力してください".into());
    }
    if key.len() > 1024 {
        return Err("APIキーが長すぎます".into());
    }
    if !key.chars().all(|ch| ch.is_ascii_graphic()) {
        return Err("APIキーに使えない文字（空白や全角文字）があります".into());
    }
    Ok(key.to_string())
}

/// Age-restricted products never go to the cloud. BOOTH's `is_adult` flag is the
/// main signal; R-18 tags or title markers count too in case the flag is missing.
pub fn age_restricted(is_adult: bool, title: &str, tags: &[String]) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)(^|[^a-z0-9])r-?18(g)?([^0-9]|$)|18禁|成人向け|アダルト").unwrap()
    });
    is_adult || re.is_match(title) || tags.iter().any(|tag| re.is_match(tag))
}

/// Where one product is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Cloud,
    Ollama,
    Apple,
    /// Age-restricted, cloud selected, and no local model ready: left unclassified.
    Withhold,
}

/// Age-restricted products never take [`Route::Cloud`].
pub fn route_for(backend: &str, restricted: bool, ollama_ready: bool, apple_ready: bool) -> Route {
    match backend {
        "apple" => Route::Apple,
        "cloud" if !restricted => Route::Cloud,
        "cloud" if ollama_ready => Route::Ollama,
        "cloud" if apple_ready => Route::Apple,
        "cloud" => Route::Withhold,
        _ => Route::Ollama,
    }
}

pub fn withheld_line(booth_id: &str) -> String {
    format!(
        "クラウド送信見送り {} 年齢制限",
        crate::booth_detail::id_tail(booth_id)
    )
}

pub fn chat_url(base: &str) -> String {
    format!("{}/chat/completions", base.trim_end_matches('/'))
}

pub fn models_url(base: &str) -> String {
    format!("{}/models", base.trim_end_matches('/'))
}

/// Same system prompt and user message as the Ollama path.
pub fn build_request(
    model: &str,
    title: &str,
    description: &str,
    category_line: &str,
    tags_line: &str,
) -> Value {
    json!({
        "model": model,
        "temperature": 0.1,
        "response_format": {"type": "json_object"},
        "messages": [
            {"role": "system", "content": crate::ollama::SYSTEM_PROMPT},
            {"role": "user", "content": crate::ollama::user_prompt(title, description, category_line, tags_line)}
        ]
    })
}

/// Some providers or models reject `response_format` or a non-default temperature
/// with 400. The second try drops both; the parser still finds the JSON object.
pub fn relax_request(payload: &mut Value) {
    if let Some(object) = payload.as_object_mut() {
        object.remove("response_format");
        object.remove("temperature");
    }
}

pub fn response_content(body: &str) -> Result<String, CloudError> {
    let value: Value = serde_json::from_str(body).map_err(|_| CloudError::InvalidResponse)?;
    let message = value
        .pointer("/choices/0/message")
        .ok_or(CloudError::InvalidResponse)?;
    let content = match message.get("content") {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")
            .trim()
            .to_string(),
        _ => String::new(),
    };
    if !content.is_empty() {
        return Ok(content);
    }
    if let Some(reasoning) = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| text.contains('{') && text.contains('}'))
    {
        return Ok(reasoning.to_string());
    }
    Err(CloudError::EmptyResponse)
}

/// `retry` counts retries already made. None means stop.
pub fn rate_limit_delay(retry: u32, retry_after: Option<&str>) -> Option<Duration> {
    if retry >= MAX_RATE_LIMIT_RETRIES {
        return None;
    }
    if let Some(seconds) = retry_after.and_then(|value| value.trim().parse::<f64>().ok()) {
        if !seconds.is_finite() || seconds > MAX_RETRY_AFTER_SECS {
            return None;
        }
        return Some(Duration::from_secs_f64(seconds.max(0.0)));
    }
    Some(Duration::from_secs(2u64 << retry))
}

fn slots() -> &'static Semaphore {
    static SLOTS: OnceLock<Semaphore> = OnceLock::new();
    SLOTS.get_or_init(|| Semaphore::new(MAX_IN_FLIGHT))
}

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|err| format!("HTTPクライアントを初期化できません: {err}"))
}

pub fn probe_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|err| format!("HTTPクライアントを初期化できません: {err}"))
}

async fn wait(delay: Duration, cancel: &AtomicBool) -> bool {
    let step = Duration::from_millis(250);
    let mut left = delay;
    while !left.is_zero() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let now = left.min(step);
        tokio::time::sleep(now).await;
        left = left.saturating_sub(now);
    }
    !cancel.load(Ordering::Relaxed)
}

#[allow(clippy::too_many_arguments)]
pub async fn extract_product(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    key: &ApiKey,
    title: &str,
    description: &str,
    category_line: &str,
    tags_line: &str,
    cancel: &AtomicBool,
) -> Result<Extraction, String> {
    let base = validate_base_url(base)?;
    let model = validate_model_name(model)?;
    if model.is_empty() {
        return Err(CloudError::MissingModel.message());
    }
    let mut payload = build_request(&model, title, description, category_line, tags_line);
    let mut relaxed = false;
    let mut retries = 0;
    loop {
        let (status, retry_after, body) = {
            let _permit = slots()
                .acquire()
                .await
                .map_err(|_| CloudError::Network.message())?;
            let response = client
                .post(chat_url(&base))
                .bearer_auth(key.expose())
                .json(&payload)
                .send()
                .await
                .map_err(|err| send_error(&err).message())?;
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let body = response
                .text()
                .await
                .map_err(|err| send_error(&err).message())?;
            (status, retry_after, body)
        };
        if (200..300).contains(&status) {
            let content = response_content(&body).map_err(CloudError::message)?;
            return crate::ollama::parse_extraction(&content);
        }
        if status == 400 && !relaxed {
            relaxed = true;
            relax_request(&mut payload);
            continue;
        }
        if status == 429 {
            if let Some(delay) = rate_limit_delay(retries, retry_after.as_deref()) {
                retries += 1;
                if wait(delay, cancel).await {
                    continue;
                }
            }
        }
        return Err(status_error(status).message());
    }
}

/// Connection test: `GET {base}/models` with the key. No product data is sent.
pub async fn test_connection(
    client: &reqwest::Client,
    base: &str,
    model: &str,
    key: Option<&ApiKey>,
) -> CloudTestResult {
    let fail = |error: CloudError| CloudTestResult {
        ok: false,
        kind: error.kind().into(),
        message: error.message(),
        model_listed: None,
    };
    let base = match validate_base_url(base) {
        Ok(base) => base,
        Err(message) => {
            return CloudTestResult {
                ok: false,
                kind: "設定".into(),
                message,
                model_listed: None,
            };
        }
    };
    let Some(key) = key else {
        return fail(CloudError::MissingKey);
    };
    let response = {
        let Ok(_permit) = slots().acquire().await else {
            return fail(CloudError::Network);
        };
        match client
            .get(models_url(&base))
            .bearer_auth(key.expose())
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status().as_u16();
                match response.text().await {
                    Ok(body) => (status, body),
                    Err(err) => return fail(send_error(&err)),
                }
            }
            Err(err) => return fail(send_error(&err)),
        }
    };
    let (status, body) = response;
    if !(200..300).contains(&status) {
        return fail(status_error(status));
    }
    let model = model.trim();
    let listed = listed_models(&body).map(|ids| ids.iter().any(|id| id == model));
    let message = match (model.is_empty(), listed) {
        (true, _) => "接続できました。モデル名を入力してください".to_string(),
        (false, Some(true)) => format!("接続できました。モデル {model} を使えます"),
        (false, Some(false)) => format!(
            "接続できました。ただし、モデル {model} は一覧にありません。モデル名を確認してください"
        ),
        (false, None) => "接続できました".to_string(),
    };
    CloudTestResult {
        ok: true,
        kind: "成功".into(),
        message,
        model_listed: if model.is_empty() { None } else { listed },
    }
}

pub fn listed_models(body: &str) -> Option<Vec<String>> {
    let value: Value = serde_json::from_str(body).ok()?;
    let data = value.get("data")?.as_array()?;
    Some(
        data.iter()
            .filter_map(|item| item.get("id").and_then(Value::as_str).map(str::to_string))
            .collect(),
    )
}

/// The OS credential store, or an in-memory stand-in for tests.
pub trait SecretStore {
    fn read(&self) -> Result<Option<String>, String>;
    fn write(&self, key: &str) -> Result<(), String>;
    fn remove(&self) -> Result<(), String>;
}

pub fn key_saved(store: &dyn SecretStore) -> Result<bool, String> {
    Ok(store.read()?.is_some_and(|key| !key.is_empty()))
}

pub fn save_key(store: &dyn SecretStore, raw: &str) -> Result<(), String> {
    let key = validate_api_key(raw)?;
    store.write(&key)
}

pub fn load_key(store: &dyn SecretStore) -> Result<ApiKey, String> {
    match store.read()? {
        Some(key) if !key.is_empty() => Ok(ApiKey::new(key)),
        _ => Err(CloudError::MissingKey.message()),
    }
}

/// Windows Credential Manager / macOS Keychain entry `<identifier>` / [`KEY_ACCOUNT`].
pub struct OsKeyStore {
    service: String,
}

impl OsKeyStore {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_string(),
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl OsKeyStore {
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, KEY_ACCOUNT)
            .map_err(|_| "OS の資格情報を開けません".to_string())
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl SecretStore for OsKeyStore {
    fn read(&self) -> Result<Option<String>, String> {
        match self.entry()?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(CloudError::KeyStore.message()),
        }
    }

    fn write(&self, key: &str) -> Result<(), String> {
        self.entry()?
            .set_password(key)
            .map_err(|_| "APIキーを OS の資格情報に保存できません".to_string())
    }

    fn remove(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("APIキーを OS の資格情報から削除できません".into()),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
impl SecretStore for OsKeyStore {
    fn read(&self) -> Result<Option<String>, String> {
        let _ = &self.service;
        Ok(None)
    }

    fn write(&self, _key: &str) -> Result<(), String> {
        Err("この OS では APIキーを保存できません".into())
    }

    fn remove(&self) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) struct MemoryKeyStore(pub std::sync::Mutex<Option<String>>);

#[cfg(test)]
impl SecretStore for MemoryKeyStore {
    fn read(&self) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn write(&self, key: &str) -> Result<(), String> {
        *self.0.lock().unwrap() = Some(key.to_string());
        Ok(())
    }

    fn remove(&self) -> Result<(), String> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMMY_KEY: &str = "sk-dummy-0123456789abcdefTESTONLY";

    #[test]
    fn request_reuses_the_local_prompt_and_asks_for_json() {
        let payload = build_request("gpt-test", "ダミー衣装", "ダミー説明", "3D衣装", "ダミータグ");
        assert_eq!(payload["model"], "gpt-test");
        assert_eq!(payload["response_format"]["type"], "json_object");
        assert_eq!(payload["temperature"], 0.1);
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], crate::ollama::SYSTEM_PROMPT);
        let user = messages[1]["content"].as_str().unwrap();
        assert_eq!(
            user,
            crate::ollama::user_prompt("ダミー衣装", "ダミー説明", "3D衣装", "ダミータグ")
        );
        assert!(user.contains("BOOTHカテゴリ: 3D衣装"));
        assert!(user.contains("タグ: ダミータグ"));
        let text = payload.to_string();
        assert!(!text.contains(DUMMY_KEY));
        let mut relaxed = payload.clone();
        relax_request(&mut relaxed);
        assert!(relaxed.get("response_format").is_none());
        assert!(relaxed.get("temperature").is_none());
        assert_eq!(relaxed["messages"], payload["messages"]);
    }

    #[test]
    fn urls_join_the_base() {
        assert_eq!(chat_url("https://api.openai.com/v1"), "https://api.openai.com/v1/chat/completions");
        assert_eq!(chat_url("https://api.x.ai/v1/"), "https://api.x.ai/v1/chat/completions");
        assert_eq!(models_url("https://api.x.ai/v1"), "https://api.x.ai/v1/models");
    }

    #[test]
    fn base_url_accepts_https_and_loopback_http_only() {
        assert_eq!(validate_base_url(" https://api.openai.com/v1/ ").unwrap(), "https://api.openai.com/v1");
        assert_eq!(validate_base_url("https://api.x.ai/v1").unwrap(), "https://api.x.ai/v1");
        assert!(validate_base_url("http://127.0.0.1:1234/v1").is_ok());
        assert!(validate_base_url("http://api.example.com/v1").is_err());
        assert!(validate_base_url("https://user:pass@api.example.com/v1").is_err());
        assert!(validate_base_url("https://api.example.com/v1?key=x").is_err());
        assert!(validate_base_url("").is_err());
        assert!(validate_base_url("ftp://api.example.com").is_err());
    }

    #[test]
    fn model_and_key_validation() {
        assert_eq!(validate_model_name("  grok-4  ").unwrap(), "grok-4");
        assert_eq!(validate_model_name("").unwrap(), "");
        assert!(validate_model_name("a\nb").is_err());
        assert_eq!(validate_api_key(&format!(" {DUMMY_KEY} ")).unwrap(), DUMMY_KEY);
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("sk abc").is_err());
        let err = validate_api_key("sk-ｘ").unwrap_err();
        assert!(!err.contains("sk-"));
    }

    #[test]
    fn parses_string_array_and_reasoning_content() {
        let json = r#"{\"category\":\"outfit\",\"compatible_avatars\":[\"ライム\"],\"is_generic\":false,\"confidence\":0.9}"#;
        let body = format!(r#"{{"choices":[{{"message":{{"role":"assistant","content":"{json}"}}}}]}}"#);
        let content = response_content(&body).unwrap();
        let extraction = crate::ollama::parse_extraction(&content).unwrap();
        assert_eq!(extraction.compatible_avatars, vec!["ライム".to_string()]);

        let parts = format!(
            r#"{{"choices":[{{"message":{{"content":[{{"type":"text","text":"```json\n{json}\n```"}}]}}}}]}}"#
        );
        let content = response_content(&parts).unwrap();
        assert!(crate::ollama::parse_extraction(&content).is_ok());

        let reasoning = format!(
            r#"{{"choices":[{{"message":{{"content":"","reasoning_content":"{json}"}}}}]}}"#
        );
        assert!(response_content(&reasoning).is_ok());
    }

    #[test]
    fn empty_and_invalid_responses_have_distinct_kinds() {
        assert_eq!(
            response_content(r#"{"choices":[{"message":{"content":"  "}}]}"#),
            Err(CloudError::EmptyResponse)
        );
        assert_eq!(response_content("<html>"), Err(CloudError::InvalidResponse));
        assert_eq!(response_content(r#"{"choices":[]}"#), Err(CloudError::InvalidResponse));
        let parsed = response_content(r#"{"choices":[{"message":{"content":"not json"}}]}"#).unwrap();
        assert!(crate::ollama::parse_extraction(&parsed).is_err());
    }

    #[test]
    fn status_codes_map_to_distinct_kinds() {
        assert_eq!(status_error(401), CloudError::Unauthorized);
        assert_eq!(status_error(403), CloudError::Forbidden);
        assert_eq!(status_error(404), CloudError::NotFound);
        assert_eq!(status_error(429), CloudError::RateLimited);
        assert_eq!(status_error(500), CloudError::Server(500));
        assert_eq!(status_error(503), CloudError::Server(503));
        assert_eq!(status_error(422), CloudError::BadRequest(422));
        let kinds: std::collections::HashSet<_> = ALL_KINDS.iter().map(|error| error.kind()).collect();
        assert_eq!(kinds.len(), ALL_KINDS.len());
        for error in ALL_KINDS.iter().copied().chain([CloudError::Server(502), CloudError::BadRequest(422)]) {
            assert_eq!(failure_kind(&error.message()), Some(error.kind()), "{error:?}");
        }
        assert_eq!(failure_kind("Ollamaに接続できません"), None);
    }

    #[test]
    fn failure_log_lines_use_cloud_kinds_and_hide_ids() {
        for (error, kind) in [
            (CloudError::Unauthorized, "認証失敗"),
            (CloudError::Forbidden, "権限なし"),
            (CloudError::RateLimited, "回数制限"),
            (CloudError::Server(503), "サーバーエラー"),
            (CloudError::Timeout, "タイムアウト"),
            (CloudError::Network, "接続失敗"),
            (CloudError::InvalidResponse, "不正な出力"),
            (CloudError::EmptyResponse, "空応答"),
            (CloudError::MissingKey, "キー未保存"),
        ] {
            let line = crate::browser::classification_failure_line("12345678", &error.message());
            assert!(line.starts_with(&format!("分類失敗 5678 {kind} ")), "{line}");
            assert!(!line.contains("12345678"));
            assert!(!line.contains(DUMMY_KEY));
        }
    }

    #[test]
    fn rate_limit_backoff_respects_retry_after_and_caps() {
        assert_eq!(rate_limit_delay(0, None), Some(Duration::from_secs(2)));
        assert_eq!(rate_limit_delay(1, None), Some(Duration::from_secs(4)));
        assert_eq!(rate_limit_delay(2, None), Some(Duration::from_secs(8)));
        assert_eq!(rate_limit_delay(3, None), None);
        assert_eq!(rate_limit_delay(0, Some("7")), Some(Duration::from_secs(7)));
        assert_eq!(rate_limit_delay(0, Some("1.5")), Some(Duration::from_millis(1500)));
        assert_eq!(rate_limit_delay(0, Some("120")), None);
        assert_eq!(rate_limit_delay(1, Some("Wed, 21 Oct 2026 07:28:00 GMT")), Some(Duration::from_secs(4)));
        assert_eq!(MAX_IN_FLIGHT, 2);
    }

    #[test]
    fn age_restricted_products_are_detected() {
        assert!(age_restricted(true, "ダミー", &[]));
        assert!(age_restricted(false, "【R-18】ダミー", &[]));
        assert!(age_restricted(false, "ダミー R18版", &[]));
        assert!(age_restricted(false, "ダミー", &["R-18".to_string()]));
        assert!(age_restricted(false, "ダミー", &["r18g".to_string()]));
        assert!(age_restricted(false, "ダミー", &["成人向け".to_string()]));
        assert!(!age_restricted(false, "VR180 カメラ", &[]));
        assert!(!age_restricted(false, "ダミー衣装 ver1.8", &["VRChat".to_string()]));
    }

    #[test]
    fn age_restricted_products_never_route_to_the_cloud() {
        for ollama in [false, true] {
            for apple in [false, true] {
                assert_ne!(route_for("cloud", true, ollama, apple), Route::Cloud);
                assert_eq!(route_for("cloud", false, ollama, apple), Route::Cloud);
            }
        }
        assert_eq!(route_for("cloud", true, true, true), Route::Ollama);
        assert_eq!(route_for("cloud", true, false, true), Route::Apple);
        assert_eq!(route_for("cloud", true, false, false), Route::Withhold);
        assert_eq!(route_for("ollama", true, false, false), Route::Ollama);
        assert_eq!(route_for("apple", true, false, false), Route::Apple);
        assert_eq!(route_for("unknown", false, false, false), Route::Ollama);
        let line = withheld_line("87654321");
        assert_eq!(line, "クラウド送信見送り 4321 年齢制限");
    }

    #[test]
    fn key_round_trips_through_the_store_and_debug_hides_it() {
        let store = MemoryKeyStore(std::sync::Mutex::new(None));
        assert!(!key_saved(&store).unwrap());
        assert_eq!(load_key(&store).unwrap_err(), CloudError::MissingKey.message());
        save_key(&store, &format!("  {DUMMY_KEY}\n")).unwrap();
        assert!(key_saved(&store).unwrap());
        let key = load_key(&store).unwrap();
        assert_eq!(key.expose(), DUMMY_KEY);
        assert!(!format!("{key:?}").contains(DUMMY_KEY));
        store.remove().unwrap();
        assert!(!key_saved(&store).unwrap());
        assert!(save_key(&store, "").is_err());
    }

    #[test]
    fn model_list_check() {
        let body = r#"{"object":"list","data":[{"id":"gpt-test"},{"id":"grok-test"}]}"#;
        assert_eq!(listed_models(body).unwrap(), vec!["gpt-test", "grok-test"]);
        assert!(listed_models("[]").is_none());
    }

    #[test]
    fn test_connection_without_key_reports_missing_key() {
        let client = probe_client().unwrap();
        let result = tauri::async_runtime::block_on(test_connection(
            &client,
            "https://api.openai.com/v1",
            "gpt-test",
            None,
        ));
        assert!(!result.ok);
        assert_eq!(result.kind, "キー未保存");
        let bad = tauri::async_runtime::block_on(test_connection(&client, "http://example.com", "m", None));
        assert!(!bad.ok);
        assert_eq!(bad.kind, "設定");
    }
}
