use crate::db;
use crate::library_walk::{
    self, PageSnapshot, WalkMachine, WalkSection, WalkStop, final_counts_line,
    same_library_page, section_label, settled_stop, walk_block_reason,
};
use crate::model::ImportReport;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

const LOG_NAME: &str = "booth-browser.log";

pub const WINDOW_LABEL: &str = "booth";
/// ログイン確認とセッション読み取りだけの隠した窓。ダウンロードでは見せない。
const PROBE_LABEL: &str = "booth-probe";
const BROWSER_USED_EVENT: &str = "booth-browser-used";
pub const LIBRARY_URL: &str = "https://accounts.booth.pm/library";
#[cfg(any(not(target_os = "macos"), test))]
const SESSION_DIR_NAME: &str = "booth-browser";
/// Fixed WKWebsiteDataStore id so the BOOTH window does not share cookies with the main window.
/// macOS 14 and later. Windows keeps using a separate WebView2 folder instead.
#[cfg(any(target_os = "macos", test))]
pub const BOOTH_DATA_STORE_ID: [u8; 16] = *b"vrclibman-booth!";

const EXTRACT_JS: &str = include_str!("../../extension/extract.js");

#[cfg(any(not(target_os = "macos"), test))]
pub fn session_dir(local_data: &Path) -> PathBuf {
    local_data.join(SESSION_DIR_NAME)
}

#[cfg(any(not(target_os = "macos"), test))]
pub fn session_dir_is_safe(local_data: &Path, dir: &Path) -> bool {
    dir != local_data
        && dir.file_name().and_then(|name| name.to_str()) == Some(SESSION_DIR_NAME)
        && dir.starts_with(local_data)
}

pub fn library_page_for_import(href: &str) -> Result<(), String> {
    let url = Url::parse(href).map_err(|_| "ページのURLを読めません".to_string())?;
    if url.scheme() != "https" {
        return Err("https の BOOTH ライブラリだけ取り込みます".into());
    }
    let host = url.host_str().unwrap_or("");
    let booth = host == "booth.pm" || host.ends_with(".booth.pm");
    if !booth {
        return Err("BOOTH のライブラリを開いてから取り込んでください".into());
    }
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    if is_age_gate(&path, &query) {
        return Err(
            "年齢確認は自動では通過しません。自分でページを開いてから、もう一度取り込んでください"
                .into(),
        );
    }
    if !path.contains("/library") {
        return Err("今開いているライブラリのページだけ取り込みます".into());
    }
    Ok(())
}

fn is_age_gate(path: &str, query: &str) -> bool {
    let haystack = format!("{path}?{query}");
    [
        "age_verification",
        "age-verification",
        "age_check",
        "r18",
        "r-18",
    ]
    .iter()
    .any(|token| haystack.contains(token))
}

pub fn page_script() -> String {
    let mut script = String::from("(function () {\n  try {\n");
    script.push_str(EXTRACT_JS);
    script.push_str(
        "\n    var href = String(location.href || \"\");\n    var products = globalThis.VrcLibManExtract.extractPage(document, href);\n    return { ok: true, href: href, products: products };\n  } catch (error) {\n    return { ok: false, error: String(error && error.message ? error.message : error) };\n  }\n})()",
    );
    script
}

pub fn parse_page_read(raw: &str) -> Result<(String, serde_json::Value), String> {
    let mut value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "ページの結果を読めません".to_string())?;
    if let serde_json::Value::String(inner) = value {
        value = serde_json::from_str(&inner).map_err(|_| "ページの結果を読めません".to_string())?;
    }
    let ok = value
        .get("ok")
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    if !ok {
        let message = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("このページを読めませんでした");
        return Err(message.to_string());
    }
    let href = value
        .get("href")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let products = value
        .get("products")
        .cloned()
        .unwrap_or(serde_json::Value::Array(Vec::new()));
    Ok((href, products))
}

pub fn export_json(products: serde_json::Value) -> String {
    serde_json::json!({
        "format": "vrclibman.booth-export",
        "version": 1,
        "source": "browser",
        "products": products,
    })
    .to_string()
}

fn log_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|err| format!("保存先を読めません: {err}"))?
        .join(LOG_NAME))
}

fn browser_log(app: &AppHandle, message: &str) {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("{stamp} {message}\n");
    eprintln!("VRCLibMan: {message}");
    let Ok(path) = log_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 200_000 {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let lines: Vec<&str> = text.lines().collect();
                let start = lines.len().saturating_sub(200);
                let _ = std::fs::write(&path, format!("{}\n", lines[start..].join("\n")));
            }
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// クエリや断片は記録しない。パスワードや戻り先のトークンを残さないため。
pub fn public_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or("");
    format!("{}://{host}{}", url.scheme(), url.path())
}

/// メールとパスワードのログインで出る別窓は、同じ窓で開く。
/// Google や年齢確認、それ以外のサイトは開かない。
pub fn open_in_same_window(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let host = url.host_str().unwrap_or("");
    let known = host == "booth.pm"
        || host.ends_with(".booth.pm")
        || host == "pixiv.net"
        || host.ends_with(".pixiv.net");
    if !known {
        return false;
    }
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    !is_age_gate(&path, &query)
}

fn open_popup_in_same_window(app: &AppHandle, url: Url) {
    if should_follow_popup(&url, download_job_armed()) {
        browser_log(
            app,
            &format!("別窓の要求を今の窓で開きます {}", public_url(&url)),
        );
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            if window.navigate(url).is_err() {
                browser_log(app, "別窓の移動に失敗しました");
            }
        }
        return;
    }
    browser_log(app, &format!("別窓の要求は開きません {}", public_url(&url)));
}

/// 受け取り中だけ、署名付きの別ホストも同じ窓で開く。クエリは記録しない。
/// 年齢確認の URL は、受け取り中でも開かない。
pub fn should_follow_popup(url: &Url, armed: bool) -> bool {
    if open_in_same_window(url) {
        return true;
    }
    if !armed || url.scheme() != "https" {
        return false;
    }
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    !is_age_gate(&path, &query)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDiagnostic {
    pub path: String,
    pub text: String,
}

pub fn diagnostic(app: &AppHandle) -> Result<BrowserDiagnostic, String> {
    let path = log_path(app)?;
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "まだ記録はありません".into());
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(40);
    Ok(BrowserDiagnostic {
        path: path.display().to_string(),
        text: if lines.is_empty() {
            "まだ記録はありません".into()
        } else {
            lines[start..].join("\n")
        },
    })
}

fn browser_enabled(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<crate::AppState>();
    let connection = state
        .db
        .lock()
        .map_err(|_| "データベースをロックできません".to_string())?;
    if !db::settings(&connection)?.builtin_browser {
        return Err("内蔵ブラウザは設定で有効にしてから使います".into());
    }
    Ok(())
}

#[cfg(any(not(target_os = "macos"), test))]
fn local_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map_err(|err| format!("保存先を読めません: {err}"))
}

/// Windows の WebView2 は再入できない。同期コマンドの中で
/// `WebviewWindowBuilder::build` すると、新しい窓だけが応答しなくなる
/// （メインの窓はメッセージを処理できるので生きたままに見える）。
/// IPC のコールバックが返ったあと、メインスレッドで作る。
/// https://github.com/tauri-apps/wry/issues/583
pub async fn dispatch_ui<T, F>(app: AppHandle, work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = app.clone();
        app.run_on_main_thread(move || {
            let _ = sender.send(work(&handle));
        })
        .map_err(|err| format!("画面の処理を予約できません: {err}"))?;
        receiver
            .recv()
            .map_err(|_| "画面の処理が返りませんでした".to_string())?
    })
    .await
    .map_err(|err| format!("画面の処理を実行できません: {err}"))?
}

pub fn open_browser(app: &AppHandle) -> Result<(), String> {
    // 設定の確認はここで終わらせ、データベースのロックを持ったまま窓を作らない。
    if let Err(err) = browser_enabled(app) {
        browser_log(app, &format!("内蔵ブラウザは使えません: {err}"));
        return Err(err);
    }
    if let Some(probe) = app.get_webview_window(PROBE_LABEL) {
        let _ = probe.destroy();
    }
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        browser_log(app, "既にある窓を前面に出します");
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let url = Url::parse(LIBRARY_URL).map_err(|err| err.to_string())?;
    let for_popup = app.clone();
    let mut builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
        .title("BOOTH")
        .inner_size(1100.0, 800.0)
        .focused(true)
        .on_page_load(|window, payload| {
            if payload.url().scheme() != "https" && payload.url().scheme() != "http" {
                return;
            }
            let kind = match payload.event() {
                tauri::webview::PageLoadEvent::Started => "読み始め",
                tauri::webview::PageLoadEvent::Finished => "読み終わり",
            };
            browser_log(
                window.app_handle(),
                &format!("{kind} {}", public_url(payload.url())),
            );
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                && !library_walk_running()
            {
                let _ = window.emit(BROWSER_USED_EVENT, ());
            }
        })
        .on_new_window(move |url, _features| {
            open_popup_in_same_window(&for_popup, url);
            tauri::webview::NewWindowResponse::Deny
        })
        .on_download(|_webview, event| handle_booth_download(event));
    // WKWebView already sends a Safari user agent. Leave it unchanged unless Booth
    // starts rejecting the embedded login; then set one current Safari string here.
    #[cfg(not(target_os = "macos"))]
    {
        let dir = session_dir(&local_data_dir(app)?);
        if !session_dir_is_safe(&local_data_dir(app)?, &dir) {
            return Err("セッションの場所が不正です".into());
        }
        std::fs::create_dir_all(&dir)
            .map_err(|err| format!("セッション用のフォルダを作れません: {err}"))?;
        // メイン画面の EBWebView とは別フォルダ。同じ data_directory だと
        // WebView2 がロックして、新しい窓だけが止まる。
        builder = builder.data_directory(dir.clone());
        browser_log(app, &format!("セッションフォルダ {}", dir.display()));
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.data_store_identifier(BOOTH_DATA_STORE_ID);
        browser_log(app, "セッションは macOS のデータストアです");
    }
    browser_log(app, "窓を作っています");
    let window = match builder.build() {
        Ok(window) => window,
        Err(err) => {
            let message = format!("ブラウザを開けません: {err}");
            browser_log(app, &message);
            return Err(message);
        }
    };
    let app_for_close = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            let _ = app_for_close.emit(BROWSER_USED_EVENT, ());
        }
    });
    browser_log(app, "窓を開きました");
    Ok(())
}

pub fn show_library(app: &AppHandle) -> Result<(), String> {
    browser_enabled(app)?;
    open_browser(app)?;
    browser_log(app, "ライブラリへ移動します");
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or("ブラウザを開いてから移動してください")?;
    let url = Url::parse(LIBRARY_URL).map_err(|err| err.to_string())?;
    window.navigate(url).map_err(|err| {
        let message = format!("ライブラリを開けません: {err}");
        browser_log(app, &message);
        message
    })
}

pub fn browse_history(app: &AppHandle, forward: bool) -> Result<(), String> {
    browser_enabled(app)?;
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or("ブラウザを開いてから移動してください")?;
    let script = if forward {
        browser_log(app, "進む");
        "history.forward()"
    } else {
        browser_log(app, "戻る");
        "history.back()"
    };
    window.eval(script).map_err(|err| {
        let message = format!("移動できません: {err}");
        browser_log(app, &message);
        message
    })
}

pub fn import_current_page(app: &AppHandle) -> Result<ImportReport, String> {
    // ページの結果を待つ間、UI スレッドを止めない。待ちはコマンド側の spawn_blocking に置く。
    let result = import_current_page_inner(app);
    match &result {
        Ok(report) => browser_log(
            app,
            &format!(
                "取り込み完了 追加 {} 更新 {} ダウンロード {}",
                report.inserted, report.updated, report.downloads
            ),
        ),
        Err(err) => browser_log(app, &format!("取り込み失敗 {err}")),
    }
    result
}

fn import_current_page_inner(app: &AppHandle) -> Result<ImportReport, String> {
    browser_enabled(app)?;
    browser_log(app, "取り込みを開始");
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or("ブラウザを開いて、ライブラリのページを表示してから取り込んでください")?;
    let (sender, receiver) = std::sync::mpsc::channel();
    window
        .eval_with_callback(page_script(), move |result| {
            let _ = sender.send(result);
        })
        .map_err(|err| format!("ページを読めません: {err}"))?;
    let raw = receiver
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| "ページの読み取りが返りませんでした".to_string())?;
    let (href, products) = parse_page_read(&raw)?;
    if let Ok(url) = Url::parse(&href) {
        browser_log(app, &format!("取り込み対象 {}", public_url(&url)));
    }
    library_page_for_import(&href)?;
    let state = app.state::<crate::AppState>();
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "データベースをロックできません".to_string())?;
    db::import_export_json(&mut connection, &export_json(products))
}

pub fn close_browser(app: &AppHandle) -> Result<(), String> {
    browser_log(app, "窓を閉じます");
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.destroy();
    }
    Ok(())
}

/// WINDOW_LABEL が残っているあいだは、同じユーザーデータに二本目の WebView を作らない。
pub fn logout_may_build_webview(window_label_exists: bool) -> bool {
    !window_label_exists
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoutOrder {
    /// 見えている窓を UI スレッドで壊してから、保存してあるプロファイルを消す。
    DestroyThenClear,
    /// 見えている窓が無いので、保存してあるプロファイルだけ消す。
    ClearOnly,
}

pub fn logout_order(window_label_exists: bool) -> LogoutOrder {
    if window_label_exists {
        LogoutOrder::DestroyThenClear
    } else {
        LogoutOrder::ClearOnly
    }
}

/// 見えている窓と、同じプロファイルの隠した窓を UI スレッドで捨てる。build は呼ばない。
fn destroy_session_windows(app: &AppHandle) -> bool {
    let mut closed = false;
    for label in [WINDOW_LABEL, PROBE_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.destroy();
            closed = true;
        }
    }
    if closed {
        browser_log(app, "ログアウトのため窓を閉じます");
    }
    closed
}

/// 内蔵ブラウザの BOOTH / pixiv セッションだけを消す。
/// 同じユーザーデータに WebView2 が残ったまま次の窓を作ると固まるので、
/// 先に UI スレッドで窓を壊してから、保存してあるプロファイルを消す。
pub async fn clear_session(app: &AppHandle) -> Result<(), String> {
    browser_log(app, "セッションを消します");
    let (existed, closed) = dispatch_ui(app.clone(), |handle| {
        let existed = handle.get_webview_window(WINDOW_LABEL).is_some();
        let closed = destroy_session_windows(handle);
        if existed && logout_may_build_webview(true) {
            return Err("表示中のブラウザがあるので、別の窓は作りません".into());
        }
        Ok((existed, closed))
    })
    .await?;
    if closed {
        let _ = tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(300));
        })
        .await;
    }
    match logout_order(existed) {
        LogoutOrder::DestroyThenClear | LogoutOrder::ClearOnly => clear_stored_profile(app).await,
    }
}

async fn clear_stored_profile(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        clear_macos_data_store(app).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        clear_folder_session(app).await
    }
}

#[cfg(not(target_os = "macos"))]
async fn clear_folder_session(app: &AppHandle) -> Result<(), String> {
    let root = local_data_dir(app)?;
    let dir = session_dir(&root);
    if !session_dir_is_safe(&root, &dir) {
        return Err("セッションの場所が不正です".into());
    }
    for _ in 0..10 {
        if !dir.exists() {
            browser_log(app, "セッションを消しました");
            return Ok(());
        }
        if std::fs::remove_dir_all(&dir).is_ok() {
            browser_log(app, "セッションを消しました");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let message =
        "セッションのフォルダを消せませんでした。ブラウザを閉じてからもう一度押してください";
    browser_log(app, message);
    Err(message.into())
}

#[cfg(target_os = "macos")]
async fn clear_macos_data_store(app: &AppHandle) -> Result<(), String> {
    let ids = app
        .fetch_data_store_identifiers()
        .await
        .map_err(|err| format!("ログイン状態を確認できません: {err}"))?;
    if !ids.iter().any(|id| id == &BOOTH_DATA_STORE_ID) {
        return Ok(());
    }
    for _ in 0..8 {
        match app.remove_data_store(BOOTH_DATA_STORE_ID).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                if err.to_string().contains("currently opened") {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                return Err(format!("ログイン状態を消せません: {err}"));
            }
        }
    }
    Err("ログイン状態を消せませんでした。ブラウザを閉じてからもう一度押してください".into())
}

static PROBE_BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoothFileStatus {
    booth_id: String,
    variation: String,
    filename: String,
    phase: String,
    message: String,
}

#[allow(dead_code)]
enum DownloadSignal {
    Started {
        host: String,
        destination: PathBuf,
    },
    Finished {
        success: bool,
        path: Option<PathBuf>,
    },
    Rejected {
        age_gate: bool,
    },
}

struct ArmedDownload {
    events: std::sync::mpsc::SyncSender<DownloadSignal>,
    started: bool,
    dir: PathBuf,
    row_filename: String,
}

struct DownloadJob(Mutex<Option<ArmedDownload>>);

static DOWNLOAD_JOB: DownloadJob = DownloadJob(Mutex::new(None));

fn job_lock() -> std::sync::MutexGuard<'static, Option<ArmedDownload>> {
    DOWNLOAD_JOB.0.lock().unwrap_or_else(|err| err.into_inner())
}

fn download_job_armed() -> bool {
    job_lock().is_some()
}

struct JobGuard;

impl Drop for JobGuard {
    fn drop(&mut self) {
        *job_lock() = None;
    }
}

fn arm_download(job: ArmedDownload) -> Result<JobGuard, String> {
    let mut slot = job_lock();
    if slot.is_some() {
        return Err(db::BROWSER_BUSY_MESSAGE.into());
    }
    *slot = Some(job);
    Ok(JobGuard)
}

pub struct DownloadChoice {
    pub started: bool,
    pub dir: PathBuf,
    pub row_filename: String,
}

#[derive(Debug)]
pub enum DownloadChoiceResult {
    AllowDefault,
    Cancel,
    Reject { age_gate: bool },
    Save(PathBuf),
}

/// One armed click owns the next download. Anything else keeps the webview's own folder.
pub fn decide_download(
    job: Option<&DownloadChoice>,
    url: &Url,
    suggested: &Path,
) -> DownloadChoiceResult {
    let Some(job) = job else {
        return DownloadChoiceResult::AllowDefault;
    };
    if job.started {
        return DownloadChoiceResult::Cancel;
    }
    if !job.dir.is_absolute() {
        return DownloadChoiceResult::Reject { age_gate: false };
    }
    if url.scheme() != "https" {
        return DownloadChoiceResult::Reject { age_gate: false };
    }
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    if is_age_gate(&path, &query) {
        return DownloadChoiceResult::Reject { age_gate: true };
    }
    let suggested = suggested
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let preferred = crate::library_files::choose_download_name(&job.row_filename, suggested);
    let name = unique_staging_name(&job.dir, &preferred);
    DownloadChoiceResult::Save(job.dir.join(name))
}

fn unique_staging_name(dir: &Path, preferred: &str) -> String {
    let preferred = crate::library_files::safe_filename(preferred)
        .unwrap_or_else(|| "received.bin".to_string());
    if std::fs::symlink_metadata(dir.join(&preferred)).is_err() {
        return preferred;
    }
    let path = Path::new(&preferred);
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|name| name.to_str());
    for index in 2..50 {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        if std::fs::symlink_metadata(dir.join(&name)).is_err() {
            return name;
        }
    }
    match extension {
        Some(extension) => format!("{stem}-copy.{extension}"),
        None => format!("{stem}-copy"),
    }
}

fn handle_booth_download(event: DownloadEvent<'_>) -> bool {
    match event {
        DownloadEvent::Requested { url, destination } => {
            // 全取得の途中では、ページが始めたダウンロードを保存しない。
            if library_walk_running() && !download_job_armed() {
                return false;
            }
            let mut guard = job_lock();
            let choice = guard.as_ref().map(|job| DownloadChoice {
                started: job.started,
                dir: job.dir.clone(),
                row_filename: job.row_filename.clone(),
            });
            match decide_download(choice.as_ref(), &url, destination) {
                DownloadChoiceResult::AllowDefault => true,
                DownloadChoiceResult::Cancel => false,
                DownloadChoiceResult::Reject { age_gate } => {
                    if let Some(job) = guard.as_ref() {
                        let _ = job.events.try_send(DownloadSignal::Rejected { age_gate });
                    }
                    false
                }
                DownloadChoiceResult::Save(path) => {
                    let Some(job) = guard.as_mut() else {
                        return true;
                    };
                    if job.started {
                        return false;
                    }
                    job.started = true;
                    *destination = path.clone();
                    let host = url.host_str().unwrap_or("").to_string();
                    let _ = job.events.try_send(DownloadSignal::Started {
                        host,
                        destination: path,
                    });
                    true
                }
            }
        }
        DownloadEvent::Finished { path, success, .. } => {
            if let Some(job) = job_lock().as_ref() {
                let _ = job
                    .events
                    .try_send(DownloadSignal::Finished { success, path });
            }
            true
        }
        _ => true,
    }
}

pub fn downloadable_url(downloadable_id: &str) -> Result<Url, String> {
    if !crate::library_files::valid_booth_id(downloadable_id) {
        return Err(db::BROWSER_MISSING_ID_MESSAGE.into());
    }
    Url::parse(&format!("https://booth.pm/downloadables/{downloadable_id}"))
        .map_err(|_| "ダウンロードのURLを作れません".to_string())
}

pub fn receipt_log(booth_id: &str, downloadable_id: &str) -> Option<String> {
    if !crate::library_files::valid_booth_id(booth_id)
        || !crate::library_files::valid_booth_id(downloadable_id)
    {
        return None;
    }
    Some(format!(
        "ファイルの受け取りを開始しました 商品 {booth_id} 番号 {downloadable_id}"
    ))
}

pub fn host_log(host: &str) -> String {
    let ok = !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
    if ok {
        format!("受け取り先ホスト {host}")
    } else {
        "受け取りを開始しました".to_string()
    }
}

fn emit_file_status(
    app: &AppHandle,
    booth_id: &str,
    variation: &str,
    filename: &str,
    phase: &str,
    message: &str,
) {
    let _ = app.emit(
        "booth-file-download",
        BoothFileStatus {
            booth_id: booth_id.to_string(),
            variation: variation.to_string(),
            filename: filename.to_string(),
            phase: phase.to_string(),
            message: message.to_string(),
        },
    );
    let (current, total) = if phase == "done" { (1, 1) } else { (0, 0) };
    let _ = app.emit(
        "job-progress",
        crate::model::JobProgress {
            job: "download".to_string(),
            current,
            total,
            message: message.to_string(),
        },
    );
}

struct ProbeTurn;

impl ProbeTurn {
    async fn acquire() -> Result<Self, ()> {
        let started = std::time::Instant::now();
        loop {
            if !PROBE_BUSY.swap(true, Ordering::SeqCst) {
                return Ok(Self);
            }
            if started.elapsed() > Duration::from_secs(20) {
                return Err(());
            }
            let _ = tauri::async_runtime::spawn_blocking(|| {
                std::thread::sleep(Duration::from_millis(40))
            })
            .await;
        }
    }
}

impl Drop for ProbeTurn {
    fn drop(&mut self) {
        PROBE_BUSY.store(false, Ordering::SeqCst);
    }
}

/// 隠した窓。visible は false、タスクバーに出さない、フォーカスしない。
fn open_profile_probe(app: &AppHandle, url: Url) -> Result<(), String> {
    if app.get_webview_window(WINDOW_LABEL).is_some() {
        return Err("表示中のブラウザがあるので、別の窓は作りません".into());
    }
    if let Some(window) = app.get_webview_window(PROBE_LABEL) {
        let _ = window.destroy();
    }
    let mut builder = WebviewWindowBuilder::new(app, PROBE_LABEL, WebviewUrl::External(url))
        .title("BOOTH")
        .inner_size(16.0, 16.0)
        .position(-20000.0, -20000.0)
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .decorations(false)
        .always_on_bottom(true)
        .on_download(|_webview, event| handle_booth_download(event));
    #[cfg(not(target_os = "macos"))]
    {
        let root = local_data_dir(app)?;
        let dir = session_dir(&root);
        if !session_dir_is_safe(&root, &dir) {
            return Err("セッションの場所が不正です".into());
        }
        std::fs::create_dir_all(&dir).map_err(|_| "ログイン状態を確認できません".to_string())?;
        builder = builder.data_directory(dir);
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.data_store_identifier(BOOTH_DATA_STORE_ID);
    }
    let window = builder
        .build()
        .map_err(|_| "ログイン状態を確認できません".to_string())?;
    let _ = window.hide();
    Ok(())
}

async fn destroy_probe(app: &AppHandle) {
    let app = app.clone();
    let _ = dispatch_ui(app, |handle| {
        if let Some(window) = handle.get_webview_window(PROBE_LABEL) {
            let _ = window.destroy();
        }
        Ok(())
    })
    .await;
    // 同じデータフォルダをすぐ開き直すと、WebView2 がまだロックしていることがある。
    let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(300)))
        .await;
}

/// 見えている窓があればそれを使い、無ければ隠した窓を作ってすぐ捨てる。
/// クッキーの読み取りは dispatch_ui の外、ワーカーで行う。
async fn booth_cookie_header(
    app: &AppHandle,
    url: Url,
) -> Result<String, crate::quiet::QuietError> {
    let _turn = ProbeTurn::acquire()
        .await
        .map_err(|_| crate::quiet::QuietError::Failed)?;
    let created = dispatch_ui(app.clone(), move |handle| {
        if handle.get_webview_window(WINDOW_LABEL).is_some() {
            return Ok(false);
        }
        let blank =
            Url::parse("about:blank").map_err(|_| "ログイン状態を確認できません".to_string())?;
        open_profile_probe(handle, blank)?;
        Ok(true)
    })
    .await
    .map_err(|_| crate::quiet::QuietError::Failed)?;
    let app_for_cookies = app.clone();
    let url_for_cookies = url.clone();
    let header = tauri::async_runtime::spawn_blocking(move || {
        let label = if created { PROBE_LABEL } else { WINDOW_LABEL };
        let window = app_for_cookies
            .get_webview_window(label)
            .ok_or_else(|| "ログイン状態を確認できません".to_string())?;
        crate::quiet::session_header(&window, &url_for_cookies)
    })
    .await;
    if created {
        destroy_probe(app).await;
    }
    header
        .map_err(|_| crate::quiet::QuietError::Failed)?
        .map_err(|_| crate::quiet::QuietError::Failed)
}

async fn receive_without_window(
    app: &AppHandle,
    download_url: Url,
    dir: PathBuf,
    row_filename: String,
) -> Result<PathBuf, crate::quiet::QuietError> {
    let header = booth_cookie_header(app, download_url.clone()).await?;
    if header.is_empty() {
        return Err(crate::quiet::QuietError::Login);
    }
    let fetched =
        crate::quiet::fetch_staged_file(&header, &download_url, &dir, &row_filename).await?;
    drop(header);
    browser_log(app, &host_log(&fetched.host));
    Ok(fetched.path)
}

fn login_message(err: crate::quiet::QuietError) -> String {
    match err {
        crate::quiet::QuietError::Login => db::BROWSER_LOGIN_MESSAGE.to_string(),
        crate::quiet::QuietError::AgeGate => db::BROWSER_AGE_MESSAGE.to_string(),
        crate::quiet::QuietError::Failed => db::BROWSER_DOWNLOAD_FAILED.to_string(),
    }
}

/// 受け取りでは、見えている窓を開いたり前面に出したりしない。
/// WebView2 は同じ data_directory の環境を二つ作るとロックするので、
/// ファイル自体は隠した窓のダウンロードではなく、セッションを別スレッドで読んで保存する。
/// ログインが必要なときだけ、既存のログイン窓を開く。
pub async fn download_one_file(
    app: AppHandle,
    booth_id: String,
    variation: String,
    filename: String,
) -> Result<(), String> {
    if library_walk_running() {
        return Err("ライブラリの取り込み中です。終わってから受け取ってください".into());
    }
    let app_for_plan = app.clone();
    let booth_for_plan = booth_id.clone();
    let variation_for_plan = variation.clone();
    let filename_for_plan = filename.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        let request = {
            let state = app_for_plan.state::<crate::AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::plan_browser_download(
                &connection,
                &booth_for_plan,
                &variation_for_plan,
                &filename_for_plan,
            )?
        };
        let downloads = crate::library_files::downloads_folder()?;
        let dir = crate::library_files::prepare_staging_dir(&downloads, &request.booth_id)?;
        Ok::<_, String>((request, dir))
    })
    .await
    .map_err(|err| format!("受け取りを準備できません: {err}"))??;
    let (request, dir) = plan;
    let url = downloadable_url(&request.downloadable_id)?;
    emit_file_status(
        &app,
        &request.booth_id,
        &request.variation,
        &request.filename,
        "receiving",
        "受け取っています",
    );
    if let Some(line) = receipt_log(&request.booth_id, &request.downloadable_id) {
        browser_log(&app, &line);
    }
    let (sender, _receiver) = std::sync::mpsc::sync_channel(8);
    let guard = arm_download(ArmedDownload {
        events: sender,
        started: true,
        dir: dir.clone(),
        row_filename: request.filename.clone(),
    })?;
    let received = receive_without_window(&app, url, dir, request.filename.clone()).await;
    drop(guard);
    let source = match received {
        Ok(path) => path,
        Err(err) => {
            let message = login_message(err);
            if err == crate::quiet::QuietError::Login {
                let _ = dispatch_ui(app.clone(), |handle| open_browser(handle)).await;
                browser_log(&app, "ログインが必要です");
            } else if err == crate::quiet::QuietError::AgeGate {
                browser_log(&app, "年齢確認は自動では通過しません");
            } else {
                browser_log(&app, "受け取りに失敗しました");
            }
            emit_file_status(
                &app,
                &request.booth_id,
                &request.variation,
                &request.filename,
                "error",
                &message,
            );
            return Err(message);
        }
    };
    emit_file_status(
        &app,
        &request.booth_id,
        &request.variation,
        &request.filename,
        "receiving",
        "ライブラリに移しています",
    );
    let app_for_commit = app.clone();
    let committed = tauri::async_runtime::spawn_blocking(move || {
        let state = app_for_commit.state::<crate::AppState>();
        let mut connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        db::commit_browser_download(&mut connection, &request, &source)
    })
    .await
    .map_err(|err| format!("ファイルを移動できません: {err}"))?;
    match committed {
        Ok(_) => {
            emit_file_status(
                &app,
                &booth_id,
                &variation,
                &filename,
                "done",
                "ライブラリに移しました",
            );
            browser_log(&app, "ファイルをライブラリへ移しました");
            let _ = app.emit("library-changed", ());
            Ok(())
        }
        Err(err) => {
            if err == db::BROWSER_LOGIN_MESSAGE {
                let _ = dispatch_ui(app.clone(), |handle| open_browser(handle)).await;
                browser_log(&app, "ログインが必要です");
            } else {
                browser_log(&app, "受け取りに失敗しました");
            }
            emit_file_status(&app, &booth_id, &variation, &filename, "error", &err);
            Err(err)
        }
    }
}

const WALK_CANCELLED: &str = "ライブラリ全取得を中止しました";
pub const WALK_LOGIN_NOTICE: &str = "ログインの期限が切れているか、ライブラリのページではありません。内蔵ブラウザでログインしてから、もう一度実行してください";

static WALK_CANCEL: AtomicBool = AtomicBool::new(false);
static WALK_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn library_walk_running() -> bool {
    WALK_RUNNING.load(Ordering::SeqCst)
}

pub fn request_library_walk_cancel() {
    WALK_CANCEL.store(true, Ordering::SeqCst);
}

fn walk_cancel_requested() -> bool {
    WALK_CANCEL.load(Ordering::SeqCst)
}

struct WalkRunningGuard;

impl Drop for WalkRunningGuard {
    fn drop(&mut self) {
        WALK_RUNNING.store(false, Ordering::SeqCst);
    }
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryWalkReport {
    pub products: u32,
    pub files: u32,
    pub named_files: u32,
    pub unnamed_files: u32,
    pub pages: u32,
    pub cancelled: bool,
    pub notice: String,
    pub inserted: u32,
    pub updated: u32,
    pub unchanged: u32,
}

fn walk_page_script() -> String {
    let mut script = String::from("(function () {\n  try {\n");
    script.push_str(EXTRACT_JS);
    script.push_str(
        "\n    var href = String(location.href || \"\");\n    var products = globalThis.VrcLibManExtract.extractPage(document, href);\n    var lastPage = globalThis.VrcLibManExtract.libraryLastPage(document, href);\n    return { ok: true, href: href, products: products, lastPage: lastPage };\n  } catch (error) {\n    return { ok: false, error: String(error && error.message ? error.message : error) };\n  }\n})()",
    );
    script
}

const PROBE_JS: &str = "(function () {\n  try {\n    return { ok: true, href: String(location.href || \"\"), ready: String(document.readyState || \"\") };\n  } catch (error) {\n    return { ok: false, error: String(error && error.message ? error.message : error) };\n  }\n})()";

fn unwrap_eval_json(raw: &str) -> Result<serde_json::Value, String> {
    let mut value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "ページの結果を読めません".to_string())?;
    if let serde_json::Value::String(inner) = value {
        value = serde_json::from_str(&inner).map_err(|_| "ページの結果を読めません".to_string())?;
    }
    Ok(value)
}

fn parse_walk_snapshot(raw: &str) -> Result<PageSnapshot, String> {
    let value = unwrap_eval_json(raw)?;
    let ok = value
        .get("ok")
        .and_then(|item| item.as_bool())
        .unwrap_or(false);
    if !ok {
        let message = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("このページを読めませんでした");
        return Err(message.to_string());
    }
    let href = value
        .get("href")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let products = value
        .get("products")
        .and_then(|item| item.as_array())
        .cloned()
        .unwrap_or_default();
    let last_page = value
        .get("lastPage")
        .and_then(|item| item.as_u64())
        .unwrap_or(1) as u32;
    Ok(PageSnapshot {
        href,
        products,
        last_page,
    })
}

fn parse_probe(raw: &str) -> Result<(String, String), String> {
    let value = unwrap_eval_json(raw)?;
    let href = value
        .get("href")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    let ready = value
        .get("ready")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .to_string();
    Ok((href, ready))
}

fn emit_walk_progress(app: &AppHandle, current: u32, total: u32, message: &str) {
    let _ = app.emit(
        "job-progress",
        crate::model::JobProgress {
            job: "browser-library".to_string(),
            current,
            total,
            message: message.to_string(),
        },
    );
}

async fn pause_for(total: Duration) {
    let slice = Duration::from_millis(50);
    let mut waited = Duration::ZERO;
    while waited < total {
        if walk_cancel_requested() {
            return;
        }
        let step = slice.min(total - waited);
        let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(step)).await;
        waited += step;
    }
}

async fn eval_booth(app: &AppHandle, script: String) -> Result<String, String> {
    eval_label(app, WINDOW_LABEL, script).await
}

async fn eval_label(app: &AppHandle, label: &str, script: String) -> Result<String, String> {
    let label = label.to_string();
    let (sender, receiver) = std::sync::mpsc::channel();
    dispatch_ui(app.clone(), move |handle| {
        let window = handle
            .get_webview_window(&label)
            .ok_or("ブラウザが開いていません")?;
        window
            .eval_with_callback(script, move |result| {
                let _ = sender.send(result);
            })
            .map_err(|err| format!("ページを読めません: {err}"))
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| "ページの読み取りが返りませんでした".to_string())
    })
    .await
    .map_err(|err| format!("ページを読めません: {err}"))?
}

/// 見えている窓があればその場で読み、無ければ隠した窓でライブラリを開いて読む。
/// 読み終わったら隠した窓は捨てる。クッキーは読まない。
pub async fn read_booth_account(app: AppHandle) -> Result<crate::quiet::BoothAccount, String> {
    browser_enabled(&app)?;
    let _turn = ProbeTurn::acquire()
        .await
        .map_err(|_| "ログイン状態を確認できません".to_string())?;
    let library = Url::parse(LIBRARY_URL).map_err(|_| "ページのURLを読めません".to_string())?;
    let created = dispatch_ui(app.clone(), move |handle| {
        if handle.get_webview_window(WINDOW_LABEL).is_some() {
            return Ok(false);
        }
        open_profile_probe(handle, library)?;
        Ok(true)
    })
    .await?;
    let raw = if created {
        wait_for_account_markup(&app).await
    } else {
        eval_label(
            &app,
            WINDOW_LABEL,
            crate::quiet::account_script().to_string(),
        )
        .await
    };
    if created {
        destroy_probe(&app).await;
    }
    let raw = raw?;
    let account = crate::quiet::parse_account_eval(&raw)?;
    browser_log(&app, "アカウントを確認しました");
    Ok(account)
}

async fn wait_for_account_markup(app: &AppHandle) -> Result<String, String> {
    let started = std::time::Instant::now();
    let script = crate::quiet::account_script().to_string();
    loop {
        let raw = eval_label(app, PROBE_LABEL, script.clone()).await?;
        let ready = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| {
                let value = if let serde_json::Value::String(inner) = value {
                    serde_json::from_str(&inner).unwrap_or(serde_json::Value::Null)
                } else {
                    value
                };
                value.get("ready").and_then(|item| item.as_bool())
            })
            .unwrap_or(false);
        if ready || started.elapsed() > Duration::from_secs(12) {
            return Ok(raw);
        }
        let _ =
            tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(250)))
                .await;
    }
}

async fn navigate_booth(
    app: &AppHandle,
    url: url::Url,
    section: WalkSection,
    page: u32,
) -> Result<(), String> {
    let label = section_label(section);
    dispatch_ui(app.clone(), move |handle| {
        if handle.get_webview_window(WINDOW_LABEL).is_none() {
            open_browser(handle)?;
        }
        browser_enabled(handle)?;
        let window = handle
            .get_webview_window(WINDOW_LABEL)
            .ok_or("ブラウザを開いてから取り込んでください")?;
        browser_log(
            handle,
            &format!("{label} の {page} ページ目を開きます {}", public_url(&url)),
        );
        window
            .navigate(url)
            .map_err(|err| format!("ページを開けません: {err}"))
    })
    .await
}

async fn wait_until_settled(app: &AppHandle, target: &url::Url) -> Result<String, String> {
    let started = std::time::Instant::now();
    loop {
        if walk_cancel_requested() {
            return Err(WALK_CANCELLED.into());
        }
        if started.elapsed() > Duration::from_secs(20) {
            return Err("ページの読み込みが返りませんでした".into());
        }
        let raw = eval_booth(app, PROBE_JS.to_string()).await?;
        let (href, ready) = parse_probe(&raw)?;
        if ready == "complete"
            && (same_library_page(&href, target) || settled_stop(&href).is_some())
        {
            return Ok(href);
        }
        pause_for(Duration::from_millis(200)).await;
    }
}

async fn read_library_page(
    app: &AppHandle,
    planned: &library_walk::PlannedPage,
) -> Result<PageSnapshot, String> {
    if walk_cancel_requested() {
        return Err(WALK_CANCELLED.into());
    }
    navigate_booth(app, planned.url.clone(), planned.section, planned.page).await?;
    let href = wait_until_settled(app, &planned.url).await?;
    if walk_cancel_requested() {
        return Err(WALK_CANCELLED.into());
    }
    if walk_block_reason(&href).is_some() {
        return Ok(PageSnapshot {
            href,
            products: Vec::new(),
            last_page: 1,
        });
    }
    let raw = eval_booth(app, walk_page_script()).await?;
    let mut snapshot = parse_walk_snapshot(&raw)?;
    if snapshot.href.is_empty() {
        snapshot.href = href;
    }
    if let Ok(url) = url::Url::parse(&snapshot.href) {
        browser_log(app, &format!("読み終わり {}", public_url(&url)));
    }
    Ok(snapshot)
}

/// ライブラリとギフトを 1 ページずつ読む。移動と eval は IPC の外で行う。ファイルは受け取らない。
pub async fn import_library(app: AppHandle) -> Result<LibraryWalkReport, String> {
    browser_enabled(&app)?;
    if download_job_armed() {
        return Err(db::BROWSER_BUSY_MESSAGE.into());
    }
    if WALK_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("ライブラリの取り込み中です".into());
    }
    let _guard = WalkRunningGuard;
    let page_delay = {
        let state = app.state::<crate::AppState>();
        let connection = state.db.lock().map_err(|_| "データベースをロックできません".to_string())?;
        Duration::from_millis(db::booth_page_interval_ms(&connection)?)
    };
    WALK_CANCEL.store(false, Ordering::SeqCst);
    browser_log(&app, "ライブラリ全取得を開始");
    let mut machine = WalkMachine::new();
    loop {
        if walk_cancel_requested() {
            machine.cancel();
            break;
        }
        let Some(planned) = machine.plan() else {
            break;
        };
        let line = machine.progress_line_for(&planned);
        emit_walk_progress(&app, 0, 0, &line);
        if planned.pause {
            if walk_cancel_requested() {
                machine.cancel();
                break;
            }
            pause_for(page_delay).await;
            if walk_cancel_requested() {
                machine.cancel();
                break;
            }
        }
        if let Some(reason) = walk_block_reason(planned.url.as_str()) {
            machine.stop_for(reason);
            break;
        }
        match read_library_page(&app, &planned).await {
            Ok(snapshot) => machine.accept(snapshot),
            Err(err) if err == WALK_CANCELLED || walk_cancel_requested() => {
                machine.cancel();
                break;
            }
            Err(err) => {
                machine.fail(err);
                break;
            }
        }
        if machine.stopped() {
            break;
        }
    }
    let collected = machine.collected();
    let mut report = LibraryWalkReport {
        products: collected.product_count(),
        files: collected.files,
        named_files: collected.named_files,
        unnamed_files: collected.unnamed_files,
        pages: collected.pages,
        cancelled: collected.stop == WalkStop::Cancelled,
        notice: match collected.stop {
            WalkStop::Login => WALK_LOGIN_NOTICE.to_string(),
            WalkStop::AgeGate => db::BROWSER_AGE_MESSAGE.to_string(),
            WalkStop::Failed => collected.error,
            WalkStop::Cancelled | WalkStop::Finished => String::new(),
        },
        inserted: 0,
        updated: 0,
        unchanged: 0,
    };
    if !collected.products.is_empty() {
        let json = export_json(serde_json::Value::Array(collected.products));
        let app_for_db = app.clone();
        let imported = tauri::async_runtime::spawn_blocking(move || {
            let state = app_for_db.state::<crate::AppState>();
            let mut connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::import_export_json(&mut connection, &json)
        })
        .await
        .map_err(|err| format!("取り込みを実行できません: {err}"))??;
        report.inserted = imported.inserted;
        report.updated = imported.updated;
        report.unchanged = imported.unchanged;
    }
    let summary = final_counts_line(
        report.products,
        report.files,
        report.named_files,
        report.unnamed_files,
    );
    browser_log(&app, &format!("ライブラリ全取得 {summary}"));
    if report.cancelled {
        browser_log(&app, "ライブラリ全取得を中止しました");
    } else if report.notice == WALK_LOGIN_NOTICE {
        browser_log(&app, "ログインが必要なので、ライブラリ全取得を止めました");
    } else if report.notice == db::BROWSER_AGE_MESSAGE {
        browser_log(
            &app,
            "年齢確認は自動では通過しないので、ライブラリ全取得を止めました",
        );
    } else if !report.notice.is_empty() {
        browser_log(&app, "ライブラリ全取得を止めました");
    }
    emit_walk_progress(&app, 1, 1, &summary);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_open_library_page_can_be_imported() {
        assert!(library_page_for_import("https://accounts.booth.pm/library").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library?page=2").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library/gifts").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library/gifts?page=2").is_ok());
        assert!(library_page_for_import("https://booth.pm/ja/items/4876459").is_err());
        assert!(library_page_for_import("https://accounts.pixiv.net/login").is_err());
        assert!(library_page_for_import("http://accounts.booth.pm/library").is_err());
        assert!(library_page_for_import("https://evil.example/library").is_err());
        assert!(library_page_for_import("https://accounts.booth.pm/age_verification").is_err());
        assert!(library_page_for_import("https://www.pixiv.net/age_verification.php").is_err());
        assert!(library_page_for_import("https://accounts.booth.pm/library?r18=1").is_err());
        assert_eq!(LIBRARY_URL, "https://accounts.booth.pm/library");
    }

    #[test]
    fn page_script_reads_the_document_and_does_not_touch_cookies() {
        let script = page_script();
        let lower = script.to_ascii_lowercase();
        assert!(script.contains("extractPage"));
        assert!(!lower.contains("document.cookie"));
        assert!(!lower.contains("credentials"));
        assert!(!script.contains("fetch("));
    }

    #[test]
    fn booth_data_store_id_is_fixed_and_separate() {
        assert_eq!(BOOTH_DATA_STORE_ID.len(), 16);
        assert_ne!(BOOTH_DATA_STORE_ID, [0u8; 16]);
        assert_eq!(&BOOTH_DATA_STORE_ID, b"vrclibman-booth!");
    }

    #[test]
    fn booth_window_is_created_after_the_ipc_callback_returns() {
        let commands = include_str!("commands.rs");
        for name in [
            "open_booth_browser",
            "show_booth_library",
            "browse_booth_history",
            "close_booth_browser",
        ] {
            let marker = format!("async fn {name}");
            let start = commands
                .find(&marker)
                .unwrap_or_else(|| panic!("{name} は async のままにする"));
            let rest = &commands[start..];
            let end = rest[marker.len()..]
                .find("\n#[tauri::command]")
                .map(|index| marker.len() + index)
                .unwrap_or(rest.len());
            let body = &rest[..end];
            assert!(body.contains("dispatch_ui"), "{name} は IPC の外で窓を触る");
            assert!(
                !commands.contains(&format!("pub fn {name}")),
                "{name} を同期コマンドに戻さない"
            );
        }
        let browser = include_str!("browser.rs");
        assert!(browser.contains("fn dispatch_ui"));
        assert!(browser.contains("run_on_main_thread"));
        assert!(browser.contains("https://github.com/tauri-apps/wry/issues/583"));
    }

    #[test]
    fn login_popups_stay_on_pixiv_or_booth_and_skip_secrets() {
        let login = Url::parse("https://accounts.pixiv.net/login?return_to=secret-token").unwrap();
        assert!(open_in_same_window(&login));
        assert_eq!(public_url(&login), "https://accounts.pixiv.net/login");
        assert!(!public_url(&login).contains("secret"));
        let library = Url::parse("https://accounts.booth.pm/library?page=2").unwrap();
        assert!(open_in_same_window(&library));
        assert_eq!(public_url(&library), "https://accounts.booth.pm/library");
        assert!(!open_in_same_window(
            &Url::parse("https://accounts.google.com/o/oauth2/auth?code=secret").unwrap()
        ));
        assert!(!open_in_same_window(
            &Url::parse("https://accounts.booth.pm/age_verification?r18=1").unwrap()
        ));
        assert!(!open_in_same_window(
            &Url::parse("http://accounts.pixiv.net/login").unwrap()
        ));
        assert!(!open_in_same_window(
            &Url::parse("https://evil.example/login").unwrap()
        ));
    }

    #[test]
    fn session_directory_stays_beside_the_library_database() {
        let _linked: fn(&AppHandle) -> Result<PathBuf, String> = local_data_dir;
        let root = Path::new("/tmp/vrclibman-local");
        let dir = session_dir(root);
        assert!(session_dir_is_safe(root, &dir));
        assert!(!session_dir_is_safe(root, root));
        assert!(!session_dir_is_safe(root, &root.join("EBWebView")));
        let temp = std::env::temp_dir().join(format!("vrclibman-browser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("booth-browser")).unwrap();
        std::fs::write(temp.join("library.sqlite"), b"db").unwrap();
        std::fs::write(temp.join("booth-browser").join("cookie"), b"secret").unwrap();
        assert!(session_dir_is_safe(&temp, &session_dir(&temp)));
        std::fs::remove_dir_all(session_dir(&temp)).unwrap();
        assert!(temp.join("library.sqlite").is_file());
        assert!(!session_dir(&temp).exists());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_session_is_the_website_data_store_not_a_folder() {
        assert_eq!(BOOTH_DATA_STORE_ID.len(), 16);
        let root = Path::new("/tmp/vrclibman-local");
        assert!(session_dir_is_safe(root, &session_dir(root)));
    }

    #[test]
    fn parsed_page_uses_the_existing_export_shape() {
        let raw = r#"{"ok":true,"href":"https://accounts.booth.pm/library","products":[{"boothId":"4876459","title":"ライム","url":"https://booth.pm/ja/items/4876459"}]}"#;
        let (href, products) = parse_page_read(raw).unwrap();
        library_page_for_import(&href).unwrap();
        let exported = export_json(products);
        assert!(exported.contains("\"source\":\"browser\""));
        assert!(exported.contains("4876459"));
        let quoted = serde_json::to_string(&serde_json::json!(raw)).unwrap();
        let (href, _) = parse_page_read(&quoted).unwrap();
        assert_eq!(href, "https://accounts.booth.pm/library");
    }

    #[test]
    fn one_file_download_stays_inside_the_logged_in_window() {
        let url = downloadable_url("2336025").unwrap();
        assert_eq!(url.as_str(), "https://booth.pm/downloadables/2336025");
        assert!(url.query().is_none());
        assert!(downloadable_url("12").is_err());
        assert!(downloadable_url("../2336025").is_err());
        assert!(downloadable_url("2336025?token=secret").is_err());
        assert!(downloadable_url("https://booth.pm/downloadables/2336025").is_err());
        let line = receipt_log("424242", "2336025").unwrap();
        assert!(!line.contains('?'));
        assert!(!line.contains("http"));
        assert_eq!(
            host_log("cdn.example.test"),
            "受け取り先ホスト cdn.example.test"
        );
        assert!(!host_log("cdn.example.test?token=secret").contains("token"));
        assert!(!host_log("cdn.example.test?token=secret").contains('?'));

        let commands = include_str!("commands.rs");
        assert!(commands.contains("async fn download_booth_file"));
        assert!(!commands.contains("pub fn download_booth_file"));
        let source = include_str!("browser.rs");
        let production = source.split("mod tests").next().unwrap();
        assert!(production.contains("pub async fn download_one_file"));
        assert!(production.contains("downloadable_url"));
        assert!(production.contains(".navigate(url)"));
        assert!(production.contains("dispatch_ui"));
        assert!(production.contains("on_download"));
        assert!(!production.contains("cookies_for_url"));
        assert!(!production.contains("reqwest"));
        assert!(!production.contains("document.cookie"));
    }

    #[test]
    fn an_armed_download_accepts_one_https_file_and_skips_age_gates() {
        let dir = std::env::temp_dir().join(format!("vrclibman-choice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let job = DownloadChoice {
            started: false,
            dir: dir.clone(),
            row_filename: "2336025".into(),
        };
        let signed = Url::parse("https://cdn.example.test/files/a?token=secret").unwrap();
        let saved = decide_download(
            Some(&job),
            &signed,
            Path::new("C:/Users/someone/Downloads/body.unitypackage"),
        );
        match saved {
            DownloadChoiceResult::Save(path) => {
                assert_eq!(path, dir.join("body.unitypackage"));
                assert!(!path.display().to_string().contains("token"));
            }
            other => panic!("expected a save path, got {other:?}"),
        }
        assert!(!should_follow_popup(&signed, false));
        assert!(should_follow_popup(&signed, true));
        assert_eq!(public_url(&signed), "https://cdn.example.test/files/a");
        let gate = Url::parse("https://cdn.example.test/age_verification?r18=1").unwrap();
        assert!(!should_follow_popup(&gate, true));
        assert!(matches!(
            decide_download(Some(&job), &gate, Path::new("page.html")),
            DownloadChoiceResult::Reject { age_gate: true }
        ));
        let plain = Url::parse("http://cdn.example.test/body.unitypackage").unwrap();
        assert!(matches!(
            decide_download(Some(&job), &plain, Path::new("body.unitypackage")),
            DownloadChoiceResult::Reject { age_gate: false }
        ));
        let busy = DownloadChoice {
            started: true,
            dir: dir.clone(),
            row_filename: "2336025".into(),
        };
        let file_url = Url::parse("https://booth.pm/downloadables/2336025").unwrap();
        assert!(matches!(
            decide_download(Some(&busy), &file_url, Path::new("body.unitypackage")),
            DownloadChoiceResult::Cancel
        ));
        assert!(matches!(
            decide_download(None, &file_url, Path::new("body.unitypackage")),
            DownloadChoiceResult::AllowDefault
        ));
        std::fs::write(dir.join("body.unitypackage"), b"already").unwrap();
        match decide_download(Some(&job), &file_url, Path::new("body.unitypackage")) {
            DownloadChoiceResult::Save(path) => {
                assert_eq!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("body (2).unitypackage")
                );
            }
            other => panic!("expected a unique name, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_walk_command_is_async_and_does_not_fetch_booth() {
        let commands = include_str!("commands.rs");
        assert!(commands.contains("async fn import_booth_library"));
        assert!(!commands.contains("pub fn import_booth_library"));
        let browser = include_str!("browser.rs");
        let start = browser.find("async fn eval_booth").expect("eval_booth");
        let rest = &browser[start..];
        let end = rest.find("\n#[cfg(test)]").unwrap_or(rest.len());
        let body = &rest[..end];
        assert!(body.contains("pub async fn import_library"));
        assert!(body.contains("dispatch_ui"));
        assert!(body.contains(".navigate("));
        assert!(body.contains("eval_with_callback"));
        assert!(body.contains("page_delay"));
        assert!(body.contains("import_export_json"));
        assert!(!body.contains("cookies_for_url"));
        assert!(!body.contains(".cookies("));
        assert!(!body.contains("reqwest"));
        assert!(!body.contains("downloadables"));
        assert!(!body.contains("download_one_file"));
        let production = browser.split("mod tests").next().unwrap();
        assert!(!production.contains("cookies_for_url"));
        assert!(!production.contains("reqwest"));
        let walk = include_str!("library_walk.rs");
        let walk_production = walk.split("mod tests").next().unwrap();
        assert!(!walk_production.contains("cookies_for_url"));
        assert!(!walk_production.contains("reqwest"));
        assert!(!walk_production.contains("downloadables"));
        assert_eq!(library_walk::PAGE_DELAY_MS, 1000);
    }

    #[test]
    fn open_log_keeps_the_path_and_drops_the_query() {
        let url = library_walk::library_walk_url(library_walk::WalkSection::Gifts, 2).unwrap();
        let line = format!("ギフト の 2 ページ目を開きます {}", public_url(&url));
        assert!(line.contains("https://accounts.booth.pm/library/gifts"));
        assert!(!line.contains('?'));
        assert!(!line.contains("サンプル"));
        let summary = library_walk::final_counts_line(2, 4, 3, 1);
        assert_eq!(summary, "商品 2、ファイル 4、名前あり 3、名前なし 1");
        assert!(!summary.contains('?'));
    }

    #[test]
    fn a_named_file_is_kept_when_the_walk_imports_an_id() {
        let dir = std::env::temp_dir().join(format!("vrclibman-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut connection = db::open(&dir.join("library.sqlite")).unwrap();
        let named = serde_json::json!([{
            "boothId": "424242",
            "title": "サンプル商品",
            "downloads": [
                {"filename": "sample_body.unitypackage", "downloadableId": "2336025", "variation": ""},
                {"filename": "2336028", "downloadableId": "2336028", "variation": ""}
            ]
        }]);
        db::import_export_json(&mut connection, &export_json(named)).unwrap();
        let id_only = serde_json::json!([{
            "boothId": "424242",
            "title": "サンプル商品",
            "downloads": [
                {"filename": "2336025", "downloadableId": "2336025", "variation": ""},
                {"filename": "2336028", "downloadableId": "2336028", "variation": ""}
            ]
        }]);
        db::import_export_json(&mut connection, &export_json(id_only)).unwrap();
        let filename: String = connection
            .query_row(
                "SELECT filename FROM product_files WHERE booth_id = '424242' AND downloadable_id = '2336025'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(filename, "sample_body.unitypackage");
        let unnamed: String = connection
            .query_row(
                "SELECT filename FROM product_files WHERE booth_id = '424242' AND downloadable_id = '2336028'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unnamed, "2336028");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quiet_download_does_not_show_or_focus_a_window() {
        let browser = include_str!("browser.rs");
        let production = browser.split("mod tests").next().unwrap();
        let download = function_body(production, "fn download_one_file");
        let receive = function_body(production, "fn receive_without_window");
        let cookies = function_body(production, "fn booth_cookie_header");
        let probe = function_body(production, "fn open_profile_probe");
        for body in [download, receive, cookies, probe] {
            assert!(!body.contains(".show("), "{body}");
            assert!(!body.contains("set_focus"), "{body}");
            assert!(!body.contains("unminimize"), "{body}");
        }
        assert!(!download.contains(".navigate("));
        assert!(!receive.contains(".navigate("));
        assert!(download.contains("receive_without_window"));
        assert!(download.contains("open_browser"));
        assert!(cookies.contains("spawn_blocking"));
        assert!(cookies.contains("session_header"));
        assert!(cookies.contains("dispatch_ui"));
        assert!(!cookies.contains("cookies_for_url"));
        let session = function_body(include_str!("quiet.rs"), "fn session_header");
        assert!(session.contains("cookies_for_url"));
        assert!(!session.contains("dispatch_ui"));
        assert!(!session.contains(".build("));
        assert!(probe.contains("visible(false)"));
        assert!(probe.contains("skip_taskbar(true)"));
        assert!(probe.contains("focused(false)"));
        assert!(probe.contains(".build("));
        assert!(production.contains("fn open_browser"));
        let open = function_body(production, "fn open_browser");
        assert!(open.contains(".show()"));
        let commands = include_str!("commands.rs");
        assert!(commands.contains("async fn booth_account"));
        assert!(commands.contains("async fn download_booth_file"));
        assert!(!commands.contains("pub fn download_booth_file"));
        assert!(!commands.contains("pub fn booth_account"));
    }

    #[test]
    fn logout_refuses_a_second_webview_while_the_booth_window_exists() {
        assert!(!logout_may_build_webview(true));
        assert!(logout_may_build_webview(false));
        assert_eq!(logout_order(true), LogoutOrder::DestroyThenClear);
        assert_eq!(logout_order(false), LogoutOrder::ClearOnly);

        let browser = include_str!("browser.rs");
        let production = browser.split("mod tests").next().unwrap();
        let start = production
            .find("pub fn logout_may_build_webview")
            .expect("logout plan");
        let end = production[start..]
            .find("static PROBE_BUSY")
            .expect("logout region");
        let logout = &production[start..start + end];
        let clear = function_body(logout, "fn clear_session");
        let destroy_call = clear
            .find("destroy_session_windows")
            .expect("destroy on the UI thread");
        let profile = clear
            .find("clear_stored_profile")
            .expect("clear after destroy");
        assert!(destroy_call < profile);
        assert!(clear.contains("dispatch_ui"));
        assert!(clear.find("dispatch_ui").unwrap() < profile);
        assert!(!clear.contains(".build("));
        assert!(!clear.contains("WebviewWindowBuilder"));
        assert!(!logout.contains(".build("));
        assert!(!logout.contains("WebviewWindowBuilder"));
        assert!(!logout.contains(".cookies("));
        assert!(!logout.contains("cookies_for_url"));
        assert!(!logout.contains("\"main\""));

        let destroy = function_body(logout, "fn destroy_session_windows");
        assert!(destroy.contains(".destroy()"));
        assert!(destroy.contains("WINDOW_LABEL"));
        assert!(destroy.contains("PROBE_LABEL"));
        assert!(destroy.find(".build(").is_none());

        let folder = function_body(logout, "fn clear_folder_session");
        assert!(folder.contains("session_dir"));
        assert!(folder.contains("session_dir_is_safe"));
        assert!(folder.contains("remove_dir_all"));
        assert!(!folder.contains(".build("));
        assert!(!folder.contains(".destroy()"));
        assert!(!folder.contains("dispatch_ui"));

        let macos = function_body(logout, "fn clear_macos_data_store");
        assert!(macos.contains("remove_data_store"));
        assert!(macos.contains("BOOTH_DATA_STORE_ID"));
        assert!(!macos.contains(".build("));
        assert!(!macos.contains(".destroy()"));
        assert!(!macos.contains(".cookies("));

        let commands = include_str!("commands.rs");
        let command = function_body(commands, "fn clear_booth_session");
        assert!(command.contains("clear_session"));
        assert!(!command.contains(".build("));
        assert!(!command.contains(".cookies("));
    }

    fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let rest = &source[start..];
        let after = rest.find('\n').unwrap() + 1;
        let tail = &rest[after..];
        let end = tail
            .find("\nfn ")
            .or_else(|| tail.find("\npub fn "))
            .or_else(|| tail.find("\npub async fn "))
            .or_else(|| tail.find("\nasync fn "))
            .or_else(|| tail.find("\nstruct "))
            .or_else(|| tail.find("\nenum "))
            .or_else(|| tail.find("\nimpl "))
            .unwrap_or(tail.len());
        &rest[..after + end]
    }
}
