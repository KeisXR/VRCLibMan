use crate::db;
use crate::library_walk::{
    self, PageSnapshot, WalkMachine, WalkSection, WalkStop, final_counts_line,
    same_library_page, section_label, settled_stop, walk_block_reason,
};
use crate::model::ImportReport;
use crate::progress::ProgressGuard;
use serde::Serialize;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

const LOG_NAME: &str = "booth-browser.log";

pub const WINDOW_LABEL: &str = "booth";
/// ログイン確認とセッション読み取りだけの隠した窓。ダウンロードでは見せない。
const PROBE_LABEL: &str = "booth-probe";
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
        return Err("https の BOOTH ライブラリだけインポートします".into());
    }
    let host = url.host_str().unwrap_or("");
    let booth = host == "booth.pm" || host.ends_with(".booth.pm");
    if !booth {
        return Err("BOOTH のライブラリを開いてからインポートしてください".into());
    }
    let path = url.path().to_ascii_lowercase();
    let query = url.query().unwrap_or("").to_ascii_lowercase();
    if is_age_gate(&path, &query) {
        return Err(
            "年齢確認は自動では通過しません。自分でページを開いてから、もう一度インポートしてください"
                .into(),
        );
    }
    if !path.contains("/library") {
        return Err("今開いているライブラリのページだけインポートします".into());
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

pub fn classification_done_line(
    processed: u32,
    booth_settled: u32,
    rule_filled: u32,
    cancelled: bool,
) -> String {
    let head = if cancelled { "分類中断" } else { "分類完了" };
    format!("{head} 処理{processed} カテゴリとタグ{booth_settled} タイトルと説明{rule_filled}")
}

/// One log line for a category whose id and name are both outside the rule table.
/// Category fields only: no product title, description, tags, or shop.
pub fn unknown_category_line(id: Option<i64>, name: &str, parent: &str) -> Option<String> {
    let name = category_log_field(name);
    let parent = category_log_field(parent);
    if id.is_none() && name.is_empty() {
        return None;
    }
    let id = id.map(|value| value.to_string()).unwrap_or_default();
    Some(format!("未対応カテゴリ id={id} name={name} parent={parent}"))
}

fn category_log_field(value: &str) -> String {
    let flat: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    flat.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

pub(crate) fn seed_unknown_category_lines(text: &str, seen: &mut HashSet<String>) {
    for line in text.lines() {
        if let Some(index) = line.find("未対応カテゴリ ") {
            seen.insert(line[index..].to_string());
        }
    }
}

pub(crate) fn claim_unique(seen: &mut HashSet<String>, line: &str) -> bool {
    seen.insert(line.to_string())
}

pub fn log_unknown_category_once(app: &AppHandle, id: Option<i64>, name: &str, parent: &str) {
    let Some(line) = unknown_category_line(id, name, parent) else {
        return;
    };
    if !claim_unknown_category_line(app, &line) {
        return;
    }
    browser_log(app, &line);
}

fn claim_unknown_category_line(app: &AppHandle, line: &str) -> bool {
    static SEEN: Mutex<Option<HashSet<String>>> = Mutex::new(None);
    let mut guard = match SEEN.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.is_none() {
        let mut seen = HashSet::new();
        if let Ok(path) = log_path(app) {
            if let Ok(text) = std::fs::read_to_string(path) {
                seed_unknown_category_lines(&text, &mut seen);
            }
        }
        *guard = Some(seen);
    }
    claim_unique(guard.as_mut().expect("seeded"), line)
}

pub(crate) fn browser_log(app: &AppHandle, message: &str) {
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

pub(crate) fn browser_enabled(app: &AppHandle) -> Result<(), String> {
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
pub(crate) fn local_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
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
            .map_err(|_| "画面の処理を完了できませんでした。もう一度お試しください".to_string())?
    })
    .await
    .map_err(|err| format!("画面の処理を実行できません: {err}"))?
}

pub fn open_browser(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = crate::browser_surface::host_window(app) {
        let _ = window.show();
    }
    crate::browser_surface::open(app)
}

pub fn show_library(app: &AppHandle) -> Result<(), String> {
    crate::browser_surface::navigate(app, LIBRARY_URL)
}

pub fn browse_history(app: &AppHandle, forward: bool) -> Result<(), String> {
    crate::browser_surface::history(app, forward)
}

pub fn import_current_page(
    app: &AppHandle,
    caller: &str,
    tab_id: Option<String>,
) -> Result<ImportReport, String> {
    // ページの結果を待つ間、UI スレッドを止めない。待ちはコマンド側の spawn_blocking に置く。
    let result = import_current_page_inner(app, caller, tab_id);
    match &result {
        Ok(report) => browser_log(
            app,
            &format!(
                "インポート完了 追加 {} 更新 {} ダウンロード {}",
                report.inserted, report.updated, report.downloads
            ),
        ),
        Err(err) => browser_log(app, &format!("インポート失敗 {err}")),
    }
    result
}

fn import_current_page_inner(
    app: &AppHandle,
    caller: &str,
    tab_id: Option<String>,
) -> Result<ImportReport, String> {
    browser_enabled(app)?;
    if library_walk_running() { return Err("ライブラリのインポート中です".into()); }
    browser_log(app, "インポートを開始");
    let tab_id = crate::browser_surface::tab_for_caller(caller, tab_id, None)?;
    let window = app
        .get_webview(&tab_id)
        .ok_or_else(|| "選択中のタブが見つかりません".to_string())?;
    let current_url = crate::browser_url::current_url(&window)?;
    library_page_for_import(current_url.as_str())?;
    let (sender, receiver) = std::sync::mpsc::channel();
    window
        .eval_with_callback(page_script(), move |result| {
            let _ = sender.send(result);
        })
        .map_err(|err| format!("ページを読めません: {err}"))?;
    let raw = receiver
        .recv_timeout(Duration::from_secs(8))
        .map_err(|_| "ページの読み取りがタイムアウトしました。もう一度お試しください".to_string())?;
    let (href, products) = parse_page_read(&raw)?;
    if let Ok(url) = Url::parse(&href) {
        browser_log(app, &format!("インポート対象 {}", public_url(&url)));
    }
    library_page_for_import(&href)?;
    if crate::browser_url::current_url(&window)?.as_str() != href {
        return Err("読み取り中にページが変わりました。もう一度インポートしてください".into());
    }
    let state = app.state::<crate::AppState>();
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "データベースをロックできません".to_string())?;
    let report = db::import_export_json(&mut connection, &export_json(products))?;
    let _ = crate::integration::record_import(&connection, "browser", &report);
    Ok(report)
}

pub fn close_browser(app: &AppHandle) -> Result<(), String> {
    request_library_walk_cancel();
    crate::browser_surface::close(app)
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
    let mut closed = crate::browser_surface::host_window(app).is_some();
    request_library_walk_cancel();
    let _ = crate::browser_surface::close(app);
    for label in [WINDOW_LABEL, PROBE_LABEL] {
        if let Some(window) = app.get_window(label) {
            let _ = window.destroy();
            closed = true;
        }
    }
    if closed {
        browser_log(app, "ログアウトのため窓を閉じます");
    }
    closed
}

struct SessionClearGuard;

impl Drop for SessionClearGuard {
    fn drop(&mut self) {
        SESSION_CLEARING.store(false, Ordering::SeqCst);
    }
}

/// 内蔵ブラウザ専用プロファイルの全サイトのセッションを消す。
/// 同じユーザーデータに WebView2 が残ったまま次の窓を作ると固まるので、
/// 先に確認を止めて UI スレッドで窓を壊してから、保存してあるプロファイルを消す。
/// 消去中は確認用 WebView を作り直さない。
pub async fn clear_session(app: &AppHandle) -> Result<(), String> {
    if SESSION_CLEARING.swap(true, Ordering::SeqCst) {
        return Err("ログアウト中です".into());
    }
    let _guard = SessionClearGuard;
    let _turn = ProbeTurn::acquire().await.map_err(|_| {
        "ログイン状態を消せません。しばらくしてからもう一度押してください".to_string()
    })?;
    browser_log(app, "セッションを消します");
    let (existed, closed) = dispatch_ui(app.clone(), |handle| {
        let existed = crate::browser_surface::host_window(handle).is_some();
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
    #[cfg(target_os = "linux")]
    crate::browser_linux::clear_profile_cache(app).await?;
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

static SESSION_CLEARING: AtomicBool = AtomicBool::new(false);
static PROBE_PROCESS_DEAD: AtomicBool = AtomicBool::new(false);
static PROBE_LOAD_GEN: AtomicU64 = AtomicU64::new(0);
static PROBE_EPOCH: AtomicU64 = AtomicU64::new(0);
static PROBE_FINISHED_EPOCH: AtomicU64 = AtomicU64::new(0);
static PROBE_BUSY: AtomicBool = AtomicBool::new(false);

struct ProbeLoadHub {
    listeners: Vec<std::sync::mpsc::SyncSender<()>>,
}

static PROBE_LOADS: Mutex<ProbeLoadHub> = Mutex::new(ProbeLoadHub {
    listeners: Vec::new(),
});

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoothFileStatus {
    booth_id: String,
    variation: String,
    filename: String,
    phase: String,
    message: String,
    /// 完了時だけ、ライブラリへ移した先の絶対パス。それ以外は null。
    path: Option<String>,
    /// 受け取り中だけ、書いたバイト数。
    received: Option<u64>,
    /// 受け取り中で、サーバーが大きさを知らせたときだけ、全体のバイト数。
    total: Option<u64>,
}

/// 同時に受け取るファイルの数。超えた分は押した順に「順番待ち」で待つ。
pub(crate) const PARALLEL_DOWNLOADS: usize = 3;

/// 受け取り中または順番待ちのファイル（商品・バリエーション・ファイル名）。
static ACTIVE_DOWNLOADS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static DOWNLOAD_SLOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(PARALLEL_DOWNLOADS);

fn active_downloads() -> std::sync::MutexGuard<'static, Vec<String>> {
    ACTIVE_DOWNLOADS.lock().unwrap_or_else(|err| err.into_inner())
}

/// 1件でも受け取り中か順番待ちなら真。全取得、タブの変更、アプリ更新の適用を止める。
pub(crate) fn download_job_armed() -> bool {
    !active_downloads().is_empty()
}

fn download_key(booth_id: &str, variation: &str, filename: &str) -> String {
    format!("{}\u{1f}{}\u{1f}{}", booth_id.trim(), variation, filename)
}

/// 受け取り中の印。Drop で外す。
struct DownloadClaim(String);

impl Drop for DownloadClaim {
    fn drop(&mut self) {
        active_downloads().retain(|key| *key != self.0);
    }
}

/// 同じファイルの二重の受け取りだけを断る。別のファイルは並行してよい。
fn claim_download(key: String) -> Result<DownloadClaim, String> {
    let mut active = active_downloads();
    if active.iter().any(|existing| *existing == key) {
        return Err(db::BROWSER_SAME_FILE_MESSAGE.into());
    }
    active.push(key.clone());
    Ok(DownloadClaim(key))
}

/// 受け取りは Rust の HTTP で行う。その間と全取得の間は、ページが始めたダウンロードを保存しない。
pub(crate) fn handle_booth_download(event: DownloadEvent<'_>) -> bool {
    match event {
        DownloadEvent::Requested { .. } => !(download_job_armed() || library_walk_running()),
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
        "ファイルのダウンロードを開始しました 商品 {booth_id} 番号 {downloadable_id}"
    ))
}

pub fn host_log(host: &str) -> String {
    let ok = !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
    if ok {
        format!("ダウンロード先ホスト {host}")
    } else {
        "ダウンロードを開始しました".to_string()
    }
}

const FILE_EVENT: &str = "booth-file-download";

/// 受け取り中の進捗を送る間隔。最後のバイトは間隔によらず送る。
const FILE_PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// 1件の受け取りの状態を `booth-file-download` で送る。
/// 画面はこのイベントでサイドバーのダウンロード一覧とファイルの行を更新する。
/// done / error を送らずに Drop されたら（途中の `?` や panic）、error を送る。
struct FileReport {
    app: AppHandle,
    booth_id: String,
    variation: String,
    filename: String,
    finished: AtomicBool,
}

impl FileReport {
    fn new(app: AppHandle, booth_id: &str, variation: &str, filename: &str) -> Self {
        Self {
            app,
            booth_id: booth_id.to_string(),
            variation: variation.to_string(),
            filename: filename.to_string(),
            finished: AtomicBool::new(false),
        }
    }

    fn running(&self, phase: &str, message: &str, bytes: Option<(u64, Option<u64>)>) {
        if self.finished.load(Ordering::SeqCst) {
            return;
        }
        let mut status = file_status(
            &self.booth_id,
            &self.variation,
            &self.filename,
            phase,
            message,
            None,
        );
        if let Some((received, total)) = bytes {
            status.received = Some(received);
            status.total = total;
        }
        let _ = self.app.emit(FILE_EVENT, status);
    }

    fn finish(&self, phase: &str, message: &str, path: Option<&std::path::Path>) {
        if self.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.app.emit(
            FILE_EVENT,
            file_status(&self.booth_id, &self.variation, &self.filename, phase, message, path),
        );
    }
}

impl Drop for FileReport {
    fn drop(&mut self) {
        self.finish("error", crate::progress::INTERRUPTED_MESSAGE, None);
    }
}

fn file_status(
    booth_id: &str,
    variation: &str,
    filename: &str,
    phase: &str,
    message: &str,
    path: Option<&std::path::Path>,
) -> BoothFileStatus {
    BoothFileStatus {
        booth_id: booth_id.to_string(),
        variation: variation.to_string(),
        filename: filename.to_string(),
        phase: phase.to_string(),
        message: message.to_string(),
        path: path.map(|path| path.display().to_string()),
        received: None,
        total: None,
    }
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

/// アカウント確認の開き方。macOS だけ表示タブと別に確認用を残す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountProbePlan {
    /// 表示中のブラウザの BOOTH タブを読む。確認用は作らない。
    Visible,
    /// 確認用を新しく作る。
    Create,
    /// 残している確認用を、見ているページとは別に読み直す。
    Reuse,
    /// プロセス終了などで、同じデータストアの確認用を作り直す。
    Recreate,
    /// ログアウトの消去中。作らない。
    Blocked,
}

/// macOS では確認用を最大1つ保持する。他の OS は表示中に二本目を作らない。
fn account_probe_is_resident() -> bool {
    cfg!(target_os = "macos")
}

fn plan_account_probe(
    resident: bool,
    clearing: bool,
    host_open: bool,
    probe_present: bool,
    process_dead: bool,
) -> AccountProbePlan {
    if clearing {
        return AccountProbePlan::Blocked;
    }
    if !resident {
        return if host_open {
            AccountProbePlan::Visible
        } else if probe_present {
            AccountProbePlan::Recreate
        } else {
            AccountProbePlan::Create
        };
    }
    if process_dead && probe_present {
        AccountProbePlan::Recreate
    } else if probe_present {
        AccountProbePlan::Reuse
    } else {
        AccountProbePlan::Create
    }
}

fn account_probe_alongside_browser(resident: bool, host_open: bool) -> bool {
    resident || !host_open
}

fn drop_probe_before_visible_browser(resident: bool) -> bool {
    !resident
}

fn drop_probe_after_account_read(resident: bool) -> bool {
    !resident
}

fn probe_process_is_dead() -> bool {
    PROBE_PROCESS_DEAD.load(Ordering::SeqCst)
}

/// The global macOS termination hook replaces Tauri's default reload handler.
/// Only the probe waits for the next account check; every other view must reload.
pub fn recover_web_content_process(label: &str, reload: impl FnOnce()) {
    if label == PROBE_LABEL {
        PROBE_PROCESS_DEAD.store(true, Ordering::SeqCst);
    } else {
        reload();
    }
}

fn current_probe_load() -> u64 {
    PROBE_LOAD_GEN.load(Ordering::SeqCst)
}

#[derive(Clone, Copy)]
enum ProbeLoadWait {
    /// いまの確認用を読み直したあと。世代がこの値より新しい完了を待つ。
    NewerThan(u64),
    /// 新しく作った確認用。その世代の完了だけを待つ。
    Epoch(u64),
}

fn probe_load_arrived(wait: ProbeLoadWait) -> bool {
    match wait {
        ProbeLoadWait::NewerThan(after) => current_probe_load() > after,
        ProbeLoadWait::Epoch(epoch) => PROBE_FINISHED_EPOCH.load(Ordering::SeqCst) == epoch,
    }
}

fn publish_probe_load() {
    PROBE_LOAD_GEN.fetch_add(1, Ordering::SeqCst);
    let mut hub = PROBE_LOADS.lock().unwrap_or_else(|err| err.into_inner());
    hub.listeners.retain(|sender| sender.try_send(()).is_ok());
}

fn subscribe_probe_load() -> std::sync::mpsc::Receiver<()> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(16);
    let mut hub = PROBE_LOADS.lock().unwrap_or_else(|err| err.into_inner());
    hub.listeners.push(sender);
    receiver
}

/// 表示用の窓を開く直前。macOS の確認用は残し、それ以外はデータフォルダのロックを避けるため捨てる。
pub fn release_probe_for_displayed_browser(app: &AppHandle) {
    if !drop_probe_before_visible_browser(account_probe_is_resident()) {
        return;
    }
    if let Some(probe) = app.get_webview_window(PROBE_LABEL) {
        let _ = probe.destroy();
    }
}

const PROBE_MARK_JS: &str = "(function(){var root=document.documentElement;if(!root){return false;}root.dataset.vrclibmanProbe='1';return true;})()";

fn probe_reload_script() -> String {
    format!(
        "location.replace({})",
        serde_json::to_string(LIBRARY_URL).unwrap_or_else(|_| "\"\"".to_string())
    )
}

/// 隠した窓。visible は false、タスクバーに出さない、フォーカスしない。タブ一覧には入れない。
/// macOS では既に生きている確認用を二本目として作り直さない。
/// 新しく作ったときは、その世代の読み込み完了だけを区別する番号を返す。
fn open_profile_probe(app: &AppHandle, url: Url) -> Result<Option<u64>, String> {
    if SESSION_CLEARING.load(Ordering::SeqCst) {
        return Err(ACCOUNT_UNCONFIRMED.into());
    }
    let host_open = crate::browser_surface::host_window(app).is_some();
    if !account_probe_alongside_browser(account_probe_is_resident(), host_open) {
        return Err("表示中のブラウザがあるので、別の窓は作りません".into());
    }
    if let Some(window) = app.get_webview_window(PROBE_LABEL) {
        let resident_live = account_probe_is_resident() && !probe_process_is_dead();
        if resident_live {
            return Ok(None);
        }
        let _ = window.destroy();
    }
    let epoch = PROBE_EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    let mut builder = WebviewWindowBuilder::new(app, PROBE_LABEL, WebviewUrl::External(url))
        .title("BOOTH")
        .inner_size(16.0, 16.0)
        .position(-20000.0, -20000.0)
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .decorations(false)
        .always_on_bottom(true)
        .on_download(|_webview, event| handle_booth_download(event))
        .on_page_load(move |_window, payload| {
            if PROBE_EPOCH.load(Ordering::SeqCst) != epoch {
                return;
            }
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                PROBE_FINISHED_EPOCH.store(epoch, Ordering::SeqCst);
                publish_probe_load();
            }
        });
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
        // 隠した WebView は既定だと停止まで進むことがある。確認用だけ処理を絞り、完全には止めない。
        // https://developer.apple.com/documentation/webkit/wkpreferences/inactiveschedulingpolicy-swift.enum
        builder = builder
            .data_store_identifier(BOOTH_DATA_STORE_ID)
            .background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Throttle);
    }
    let window = builder
        .build()
        .map_err(|_| "ログイン状態を確認できません".to_string())?;
    PROBE_PROCESS_DEAD.store(false, Ordering::SeqCst);
    #[cfg(target_os = "linux")]
    crate::browser_linux::remember_profile(window.as_ref())?;
    let _ = window.hide();
    Ok(Some(epoch))
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

#[derive(Clone, Copy)]
enum ProbeCookieWindow {
    Created,
    Existing,
    Visible,
}

/// 見えている窓があればそれを使い、無ければ隠した窓を作る。
/// macOS の確認用は読み取り後も残す。クッキーの読み取りは dispatch_ui の外、ワーカーで行う。
async fn booth_cookie_header(
    app: &AppHandle,
    url: Url,
) -> Result<String, crate::quiet::QuietError> {
    let _turn = ProbeTurn::acquire()
        .await
        .map_err(|_| crate::quiet::QuietError::Failed)?;
    if SESSION_CLEARING.load(Ordering::SeqCst) {
        return Err(crate::quiet::QuietError::Failed);
    }
    let kind = dispatch_ui(app.clone(), move |handle| {
        if SESSION_CLEARING.load(Ordering::SeqCst) {
            return Err(ACCOUNT_UNCONFIRMED.into());
        }
        if crate::browser_surface::host_window(handle).is_some() {
            return Ok(ProbeCookieWindow::Visible);
        }
        if account_probe_is_resident()
            && handle.get_webview(PROBE_LABEL).is_some()
            && !probe_process_is_dead()
        {
            return Ok(ProbeCookieWindow::Existing);
        }
        let blank =
            Url::parse("about:blank").map_err(|_| "ログイン状態を確認できません".to_string())?;
        let _ = open_profile_probe(handle, blank)?;
        Ok(ProbeCookieWindow::Created)
    })
    .await
    .map_err(|_| crate::quiet::QuietError::Failed)?;
    let app_for_cookies = app.clone();
    let url_for_cookies = url.clone();
    let header = tauri::async_runtime::spawn_blocking(move || {
        let window = match kind {
            ProbeCookieWindow::Created | ProbeCookieWindow::Existing => app_for_cookies
                .get_webview(PROBE_LABEL)
                .ok_or_else(|| "ログイン状態を確認できません".to_string())?,
            ProbeCookieWindow::Visible => crate::browser_surface::active_view(&app_for_cookies)?,
        };
        crate::quiet::session_header(&window, &url_for_cookies)
    })
    .await;
    if matches!(kind, ProbeCookieWindow::Created)
        && drop_probe_after_account_read(account_probe_is_resident())
    {
        destroy_probe(app).await;
    }
    header
        .map_err(|_| crate::quiet::QuietError::Failed)?
        .map_err(|_| crate::quiet::QuietError::Failed)
}

async fn receive_without_window<F: FnMut(u64, Option<u64>) + Send>(
    app: &AppHandle,
    download_url: Url,
    dir: PathBuf,
    row_filename: String,
    on_progress: F,
) -> Result<PathBuf, crate::quiet::QuietError> {
    let header = booth_cookie_header(app, download_url.clone()).await?;
    if header.is_empty() {
        return Err(crate::quiet::QuietError::Login);
    }
    let fetched = crate::quiet::fetch_staged_file(
        &header,
        &download_url,
        &dir,
        &row_filename,
        on_progress,
    )
    .await?;
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
) -> Result<String, String> {
    if library_walk_running() {
        return Err("ライブラリのインポート中です。終わってからダウンロードしてください".into());
    }
    // 別のファイルとは並行して受け取る。同じファイルの二重の押下だけを断る。
    let _claim = claim_download(download_key(&booth_id, &variation, &filename))?;
    let report = FileReport::new(app.clone(), &booth_id, &variation, &filename);
    let outcome: Result<PathBuf, String> = async {
        let app_for_plan = app.clone();
        let booth_for_plan = booth_id.clone();
        let variation_for_plan = variation.clone();
        let filename_for_plan = filename.clone();
        let (request, dir) = tauri::async_runtime::spawn_blocking(move || {
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
        .map_err(|err| format!("ダウンロードを準備できません: {err}"))??;
        let url = downloadable_url(&request.downloadable_id)?;
        let slot = match DOWNLOAD_SLOTS.try_acquire() {
            Ok(slot) => slot,
            Err(_) => {
                report.running("queued", "順番待ち", None);
                DOWNLOAD_SLOTS
                    .acquire()
                    .await
                    .map_err(|_| db::BROWSER_DOWNLOAD_FAILED.to_string())?
            }
        };
        report.running("receiving", "ダウンロード中", Some((0, None)));
        if let Some(line) = receipt_log(&request.booth_id, &request.downloadable_id) {
            browser_log(&app, &line);
        }
        let mut last_sent: Option<std::time::Instant> = None;
        let received = receive_without_window(
            &app,
            url,
            dir,
            request.filename.clone(),
            |received, total| {
                let due = last_sent.map_or(true, |at| at.elapsed() >= FILE_PROGRESS_INTERVAL);
                if due || total == Some(received) {
                    last_sent = Some(std::time::Instant::now());
                    report.running("receiving", "ダウンロード中", Some((received, total)));
                }
            },
        )
        .await;
        // 受け取りが終わったら、ライブラリへ移す前に次のファイルへ枠を渡す。
        drop(slot);
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
                    browser_log(&app, "ダウンロードに失敗しました");
                }
                return Err(message);
            }
        };
        report.running("moving", "ライブラリに移しています", None);
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
        if let Err(err) = &committed {
            if err == db::BROWSER_LOGIN_MESSAGE {
                let _ = dispatch_ui(app.clone(), |handle| open_browser(handle)).await;
                browser_log(&app, "ログインが必要です");
            } else {
                browser_log(&app, "ダウンロードに失敗しました");
            }
        }
        committed
    }
    .await;
    match outcome {
        Ok(placed) => {
            report.finish("done", "ライブラリに移しました", Some(&placed));
            browser_log(&app, "ファイルをライブラリへ移しました");
            let _ = app.emit("library-changed", ());
            Ok(placed.display().to_string())
        }
        Err(err) => {
            report.finish("error", &err, None);
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
        clear_walk_tab();
    }
}

/// The page the library walk is reading. Later tab changes do not move the walk.
static WALK_TAB: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn walk_uses_tab(id: &str) -> bool {
    WALK_TAB
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_deref()
        == Some(id)
}

fn pinned_walk_tab(app: &AppHandle) -> Result<String, String> {
    let mut slot = WALK_TAB.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(id) = slot.clone() {
        if app.get_webview(&id).is_some() {
            return Ok(id);
        }
    }
    let view = crate::browser_surface::active_view(app)?;
    let id = view.label().to_string();
    *slot = Some(id.clone());
    Ok(id)
}

fn clear_walk_tab() {
    *WALK_TAB.lock().unwrap_or_else(|error| error.into_inner()) = None;
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

fn emit_walk_progress(progress: &ProgressGuard, current: u32, total: u32, message: &str) {
    progress.update(current, total, message);
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
    let label = pinned_walk_tab(app)?;
    eval_label(app, &label, script).await
}

async fn eval_label(app: &AppHandle, label: &str, script: String) -> Result<String, String> {
    eval_label_within(app, label, script, Duration::from_secs(15)).await
}

async fn eval_label_within(
    app: &AppHandle,
    label: &str,
    script: String,
    timeout: Duration,
) -> Result<String, String> {
    let label = label.to_string();
    let (sender, receiver) = std::sync::mpsc::channel();
    dispatch_ui(app.clone(), move |handle| {
        let window = if label == WINDOW_LABEL {
            crate::browser_surface::active_view(handle)?
        } else {
            handle
                .get_webview(&label)
                .ok_or("ブラウザが開いていません")?
        };
        window
            .eval_with_callback(script, move |result| {
                let _ = sender.send(result);
            })
            .map_err(|err| format!("ページを読めません: {err}"))
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver.recv_timeout(timeout).map_err(|_| {
            "ページの読み取りがタイムアウトしました。もう一度お試しください".to_string()
        })
    })
    .await
    .map_err(|err| format!("ページを読めません: {err}"))?
}

const ACCOUNT_UNCONFIRMED: &str = "ログイン状態を確認できません";
const ACCOUNT_READ_LIMIT: Duration = Duration::from_secs(12);

fn account_budget(started: std::time::Instant) -> Duration {
    ACCOUNT_READ_LIMIT.saturating_sub(started.elapsed())
}

/// 見えている BOOTH のタブがあれば、macOS 以外はその場で読む。
/// macOS は表示タブと別の確認用でライブラリを読む。見ているページは動かさない。
/// 新しく作った確認用は、読み込み完了のあとで DOM を見る。生成した直後の結果付きスクリプトは実行しない。
/// Wry の macOS 実装は初回ナビゲーション確定前のスクリプトを結果コールバックなしで保留する。
/// https://github.com/tauri-apps/wry/blob/wry-v0.57.0/src/wkwebview/mod.rs#L723
/// macOS 以外は読み終わったら隠した窓を捨てる。クッキーは読まない。
pub async fn read_booth_account(app: AppHandle) -> Result<crate::quiet::BoothAccount, String> {
    browser_enabled(&app)?;
    let _turn = ProbeTurn::acquire()
        .await
        .map_err(|_| ACCOUNT_UNCONFIRMED.to_string())?;
    if SESSION_CLEARING.load(Ordering::SeqCst) {
        return Err(ACCOUNT_UNCONFIRMED.into());
    }
    let started = std::time::Instant::now();
    let resident = account_probe_is_resident();
    let plan = dispatch_ui(app.clone(), move |handle| {
        Ok(plan_account_probe(
            resident,
            SESSION_CLEARING.load(Ordering::SeqCst),
            crate::browser_surface::host_window(handle).is_some(),
            handle.get_webview(PROBE_LABEL).is_some(),
            probe_process_is_dead(),
        ))
    })
    .await?;
    let result = match plan {
        AccountProbePlan::Blocked => Err(ACCOUNT_UNCONFIRMED.into()),
        AccountProbePlan::Visible => read_visible_account(&app, started).await,
        AccountProbePlan::Create => read_created_probe(&app, started).await,
        AccountProbePlan::Reuse => read_reused_probe(&app, started).await,
        AccountProbePlan::Recreate => rebuild_probe_and_read(&app, started).await,
    };
    if drop_probe_after_account_read(resident)
        && matches!(plan, AccountProbePlan::Create | AccountProbePlan::Recreate)
    {
        destroy_probe(&app).await;
    }
    match &result {
        Ok(_) => browser_log(&app, "アカウントを確認しました"),
        Err(_) => browser_log(&app, "アカウントを確認できません"),
    }
    result
}

async fn read_visible_account(
    app: &AppHandle,
    started: std::time::Instant,
) -> Result<crate::quiet::BoothAccount, String> {
    let label = dispatch_ui(app.clone(), |handle| {
        crate::browser_surface::booth_view(handle)
            .map(|view| view.label().to_string())
            .ok_or_else(|| ACCOUNT_UNCONFIRMED.to_string())
    })
    .await?;
    read_settled_account(app, &label, started, false).await
}

async fn read_created_probe(
    app: &AppHandle,
    started: std::time::Instant,
) -> Result<crate::quiet::BoothAccount, String> {
    let result = open_and_read_once(app, started).await;
    if result.is_err() && account_probe_is_resident() && probe_process_is_dead() {
        browser_log(app, "確認用のプロセスが終了したので作り直します");
        return rebuild_probe_and_read(app, started).await;
    }
    result
}

async fn open_and_read_once(
    app: &AppHandle,
    started: std::time::Instant,
) -> Result<crate::quiet::BoothAccount, String> {
    if SESSION_CLEARING.load(Ordering::SeqCst) {
        return Err(ACCOUNT_UNCONFIRMED.into());
    }
    let library = Url::parse(LIBRARY_URL).map_err(|_| "ページのURLを読めません".to_string())?;
    let opened = dispatch_ui(app.clone(), move |handle| open_profile_probe(handle, library)).await?;
    let Some(epoch) = opened else {
        return Err(ACCOUNT_UNCONFIRMED.into());
    };
    wait_for_probe_load(ProbeLoadWait::Epoch(epoch), account_budget(started)).await?;
    read_settled_account(app, PROBE_LABEL, started, false).await
}

async fn read_reused_probe(
    app: &AppHandle,
    started: std::time::Instant,
) -> Result<crate::quiet::BoothAccount, String> {
    let result = reload_resident_probe(app, started).await;
    if result.is_err() && (probe_process_is_dead() || probe_window_is_gone(app).await) {
        browser_log(app, "確認用のプロセスが終了したので作り直します");
        return rebuild_probe_and_read(app, started).await;
    }
    result
}

async fn reload_resident_probe(
    app: &AppHandle,
    started: std::time::Instant,
) -> Result<crate::quiet::BoothAccount, String> {
    let budget = account_budget(started);
    let marked = eval_label_within(app, PROBE_LABEL, PROBE_MARK_JS.to_string(), budget.min(Duration::from_secs(3))).await;
    if !matches!(marked, Ok(ref value) if value.contains("true")) {
        return Err(ACCOUNT_UNCONFIRMED.into());
    }
    let after = current_probe_load();
    let script = probe_reload_script();
    dispatch_ui(app.clone(), move |handle| {
        let window = handle
            .get_webview(PROBE_LABEL)
            .ok_or_else(|| "ブラウザが開いていません".to_string())?;
        window
            .eval(script)
            .map_err(|err| format!("ページを開けません: {err}"))
    })
    .await?;
    wait_for_probe_load(ProbeLoadWait::NewerThan(after), account_budget(started)).await?;
    read_settled_account(app, PROBE_LABEL, started, true).await
}

async fn rebuild_probe_and_read(
    app: &AppHandle,
    started: std::time::Instant,
) -> Result<crate::quiet::BoothAccount, String> {
    destroy_probe(app).await;
    open_and_read_once(app, started).await
}

async fn probe_window_is_gone(app: &AppHandle) -> bool {
    dispatch_ui(app.clone(), |handle| {
        Ok(handle.get_webview(PROBE_LABEL).is_none())
    })
    .await
    .unwrap_or(true)
}

/// 読み込み完了まで待つ。UI スレッドでは待たない。URL はログに残さない。
/// 新しく作った窓は、その世代の完了だけを見る。古い窓の完了ではスクリプトを走らせない。
async fn wait_for_probe_load(wait: ProbeLoadWait, timeout: Duration) -> Result<(), String> {
    if timeout.is_zero() {
        return Err(ACCOUNT_UNCONFIRMED.into());
    }
    if probe_load_arrived(wait) {
        return Ok(());
    }
    let receiver = subscribe_probe_load();
    if probe_load_arrived(wait) {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        loop {
            if probe_load_arrived(wait) {
                return Ok(());
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(ACCOUNT_UNCONFIRMED.to_string());
            }
            match receiver.recv_timeout(remaining) {
                Ok(()) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    if probe_load_arrived(wait) {
                        return Ok(());
                    }
                    return Err(ACCOUNT_UNCONFIRMED.to_string());
                }
            }
        }
    })
    .await
    .map_err(|_| ACCOUNT_UNCONFIRMED.to_string())?
}

/// 読み込み後の DOM を、期限まで確認する。未準備と失敗は未ログインにしない。
/// `require_fresh` のときは、読み直す前の印が残った文書を確定に使わない。
async fn read_settled_account(
    app: &AppHandle,
    label: &str,
    started: std::time::Instant,
    require_fresh: bool,
) -> Result<crate::quiet::BoothAccount, String> {
    let script = crate::quiet::account_script().to_string();
    let mut login_streak = 0u8;
    loop {
        let budget = account_budget(started);
        if budget < Duration::from_millis(50) {
            return Err(ACCOUNT_UNCONFIRMED.into());
        }
        let timeout = budget.min(Duration::from_secs(3));
        let reading = match eval_label_within(app, label, script.clone(), timeout).await {
            Ok(raw) if require_fresh && !crate::quiet::account_document_is_fresh(&raw) => {
                Ok(crate::quiet::AccountReading::Pending)
            }
            Ok(raw) => crate::quiet::classify_account_eval(&raw),
            Err(_) => Err(ACCOUNT_UNCONFIRMED.to_string()),
        };
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let limit = u64::try_from(ACCOUNT_READ_LIMIT.as_millis()).unwrap_or(u64::MAX);
        match crate::quiet::advance_account_poll(reading, &mut login_streak, elapsed, limit) {
            crate::quiet::AccountPoll::Ready(account) => return Ok(account),
            crate::quiet::AccountPoll::Stop => return Err(ACCOUNT_UNCONFIRMED.into()),
            crate::quiet::AccountPoll::Wait => {
                let pause = Duration::from_millis(200).min(account_budget(started));
                if pause.is_zero() {
                    return Err(ACCOUNT_UNCONFIRMED.into());
                }
                let _ =
                    tauri::async_runtime::spawn_blocking(move || std::thread::sleep(pause)).await;
            }
        }
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
        if crate::browser_surface::host_window(handle).is_none() {
            open_browser(handle)?;
        }
        browser_enabled(handle)?;
        let id = pinned_walk_tab(handle)?;
        let window = handle
            .get_webview(&id)
            .ok_or_else(|| "ブラウザが開いていません".to_string())?;
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
            return Err("ページの読み込みがタイムアウトしました。もう一度お試しください".into());
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

/// ライブラリ、ギフト、無料ダウンロードを 1 ページずつ読む。移動と eval は IPC の外で行う。ファイルは受け取らない。
pub async fn import_library(app: AppHandle) -> Result<LibraryWalkReport, String> {
    browser_enabled(&app)?;
    if download_job_armed() {
        return Err(db::BROWSER_BUSY_MESSAGE.into());
    }
    if WALK_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("ライブラリのインポート中です".into());
    }
    let _guard = WalkRunningGuard;
    let progress = ProgressGuard::new(app.clone(), "browser-library");
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
        emit_walk_progress(&progress, 0, 0, &line);
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
        let authoritative = collected.stop == WalkStop::Finished;
        let json = export_json(serde_json::Value::Array(collected.products));
        let app_for_db = app.clone();
        let imported = tauri::async_runtime::spawn_blocking(move || {
            let state = app_for_db.state::<crate::AppState>();
            let mut connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            let report =
                db::import_export_json_with_mode(&mut connection, &json, authoritative)?;
            let _ = crate::integration::record_import(&connection, "browser", &report);
            Ok::<_, String>(report)
        })
        .await
        .map_err(|err| format!("インポートを実行できません: {err}"))??;
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
    let skip_detail = report.cancelled || report.notice == db::BROWSER_AGE_MESSAGE;
    if !skip_detail {
        fill_public_details(&app, &progress).await?;
    }
    if report.cancelled {
        progress.cancel(summary);
    } else if !report.notice.is_empty() {
        progress.fail(summary);
    } else {
        progress.done_with(1, summary);
    }
    Ok(report)
}

async fn fill_public_details(app: &AppHandle, progress: &ProgressGuard) -> Result<(), String> {
    if walk_cancel_requested() {
        return Ok(());
    }
    let ids = {
        let state = app.state::<crate::AppState>();
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        db::products_needing_booth_detail(&connection)?
    };
    let total = ids.len() as u32;
    if total == 0 {
        return Ok(());
    }
    let pause = {
        let state = app.state::<crate::AppState>();
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        library_walk::detail_pause(Duration::from_millis(db::booth_page_interval_ms(
            &connection,
        )?))
    };
    for (index, booth_id) in ids.iter().enumerate() {
        if walk_cancel_requested() {
            return Ok(());
        }
        pause_for(pause).await;
        if walk_cancel_requested() {
            return Ok(());
        }
        let current = (index as u32) + 1;
        emit_walk_progress(
            progress,
            current,
            total,
            &library_walk::detail_progress_line(current, total),
        );
        let interval_ms = pause.as_millis() as u64;
        let outcome = match crate::booth_detail::fetch_public_item(booth_id).await {
            Ok(fetched) => {
                browser_log(
                    app,
                    &crate::booth_detail::detail_log_line(
                        booth_id,
                        fetched.http_status,
                        &fetched.outcome,
                        interval_ms,
                    ),
                );
                fetched.outcome
            }
            Err(_) => {
                browser_log(
                    app,
                    &crate::booth_detail::detail_log_line(
                        booth_id,
                        None,
                        &crate::booth_detail::DetailOutcome::Again,
                        interval_ms,
                    ),
                );
                browser_log(app, "商品情報の確認を中断しました");
                return Ok(());
            }
        };
        let app_for_db = app.clone();
        let booth_id = booth_id.clone();
        let stop = tauri::async_runtime::spawn_blocking(move || {
            let state = app_for_db.state::<crate::AppState>();
            let mut connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::store_booth_detail(&mut connection, &booth_id, &outcome)
        })
        .await
        .map_err(|_| "商品情報を保存できません".to_string())??;
        if stop {
            browser_log(app, "商品情報の確認を中断しました");
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn only_the_open_library_page_can_be_imported() {
        assert!(library_page_for_import("https://accounts.booth.pm/library").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library?page=2").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library/gifts").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library/gifts?page=2").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library/free_downloads").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library/free_downloads?page=2").is_ok());
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
    fn booth_operation_logs_skip_url_secrets() {
        let login = Url::parse("https://accounts.pixiv.net/login?return_to=secret-token").unwrap();
        assert_eq!(public_url(&login), "https://accounts.pixiv.net/login");
        assert!(!public_url(&login).contains("secret"));
        let library = Url::parse("https://accounts.booth.pm/library?page=2").unwrap();
        assert_eq!(public_url(&library), "https://accounts.booth.pm/library");
        assert_eq!(
            classification_done_line(117, 89, 28, false),
            "分類完了 処理117 カテゴリとタグ89 タイトルと説明28"
        );
    }

    #[test]
    fn unknown_category_lines_are_unique_and_omit_product_text() {
        let line = unknown_category_line(Some(157), "架空カテゴリ", "架空の親").unwrap();
        assert_eq!(line, "未対応カテゴリ id=157 name=架空カテゴリ parent=架空の親");
        assert!(unknown_category_line(None, "  \n  ", "親だけ").is_none());
        let broken = unknown_category_line(Some(6), "架空\n改行", "親\t名").unwrap();
        assert!(!broken.contains('\n'));
        assert!(!broken.contains('\t'));
        assert!(broken.contains("name=架空 改行"));
        let mut seen = HashSet::new();
        let prior = "2026-10-29 12:00:00 未対応カテゴリ id=142 name=架空カテゴリ parent=架空の親\n分類完了 処理1 失敗0 カテゴリとタグ0 モデル1\n";
        seed_unknown_category_lines(prior, &mut seen);
        let again = unknown_category_line(Some(142), "架空カテゴリ", "架空の親").unwrap();
        assert!(!claim_unique(&mut seen, &again));
        let other = unknown_category_line(Some(142), "別の架空", "架空の親").unwrap();
        assert!(claim_unique(&mut seen, &other));
        assert!(!claim_unique(&mut seen, &other));
        assert!(!again.contains("商品"));
        assert!(!again.contains("タグ"));
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
            "ダウンロード先ホスト cdn.example.test"
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
    fn parallel_downloads_refuse_only_the_same_file() {
        let first = claim_download(download_key("424242", "", "body.unitypackage")).unwrap();
        let other = claim_download(download_key("424242", "", "hair.unitypackage")).unwrap();
        assert!(download_job_armed());
        assert_eq!(
            claim_download(download_key(" 424242 ", "", "body.unitypackage")).err().as_deref(),
            Some(db::BROWSER_SAME_FILE_MESSAGE)
        );
        let quest = claim_download(download_key("424242", "Quest", "body.unitypackage")).unwrap();
        drop(first);
        let again = claim_download(download_key("424242", "", "body.unitypackage")).unwrap();
        drop(again);
        drop(quest);
        drop(other);
        assert_eq!(DOWNLOAD_SLOTS.available_permits(), PARALLEL_DOWNLOADS);
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
        assert!(body.contains("fill_public_details"));
        assert!(body.contains("detail_pause"));
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
        let free = library_walk::library_walk_url(library_walk::WalkSection::Free, 1).unwrap();
        let free_line = format!("無料ダウンロードを開きます {}", public_url(&free));
        assert!(free_line.contains("https://accounts.booth.pm/library/free_downloads"));
        assert!(!free_line.contains('?'));
        let summary = library_walk::final_counts_line(2, 4, 3, 1);
        assert_eq!(summary, "商品 2 件、ファイル 4 件");
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
    fn finished_download_status_carries_the_library_path() {
        let placed = std::path::Path::new("/lib/4242/pack.zip");
        let done = serde_json::to_value(file_status("4242", "", "pack.zip", "done", "ライブラリに移しました", Some(placed))).unwrap();
        assert_eq!(done["phase"], "done");
        assert_eq!(done["boothId"], "4242");
        assert_eq!(done["path"], placed.display().to_string());
        let receiving = serde_json::to_value(file_status("4242", "", "pack.zip", "receiving", "ダウンロード中", None)).unwrap();
        assert!(receiving["path"].is_null());
        let production = include_str!("browser.rs").split("mod tests").next().unwrap();
        let download = function_body(production, "fn download_one_file");
        assert!(download.contains("Ok(placed) =>"));
        assert!(download.contains("Some(&placed)"));
        assert!(!download.contains("Ok(_) =>"));
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

    #[test]
    fn account_probe_waits_for_load_before_a_script_with_a_result() {
        let production = include_str!("browser.rs")
            .split("mod tests")
            .next()
            .unwrap();
        let created = bounded_function(production, "fn open_and_read_once");
        let wait = created.find("wait_for_probe_load").expect("load event");
        let settled = created.find("read_settled_account").expect("dom read");
        assert!(wait < settled, "DOM は読み込み完了のあとで見る");
        assert!(created.contains("open_profile_probe"));
        assert!(!created.contains("eval_with_callback"));
        assert!(!created.contains("account_script"));
        assert!(!created.contains("thread::sleep"));
        let read = bounded_function(production, "fn read_booth_account");
        assert!(read.contains("open_and_read_once") || read.contains("read_created_probe"));
        assert!(!read.contains("wait_for_account_markup"));
        assert!(!read.contains("thread::sleep"));
        assert!(read.find("SESSION_CLEARING").unwrap() < read.find("plan_account_probe").unwrap());
        let probe = bounded_function(production, "fn open_profile_probe");
        assert!(probe.contains("PageLoadEvent::Finished"));
        assert!(probe.contains("on_page_load"));
        assert!(!probe.contains("eval_with_callback"));
        assert!(!probe.contains("account_script"));
        assert!(!probe.contains("thread::sleep"));
        assert!(probe.contains("focused(false)"));
        assert!(probe.contains("visible(false)"));
        assert!(!probe.contains("set_focus"));
        assert!(!probe.contains("session.add"));
        assert!(probe.contains("SESSION_CLEARING"));
        assert!(probe.contains("BackgroundThrottlingPolicy::Throttle"));
        assert_eq!(probe.matches("background_throttling").count(), 1);
        let settled_body = bounded_function(production, "fn read_settled_account");
        assert!(settled_body.contains("advance_account_poll"));
        assert!(settled_body.contains("account_document_is_fresh"));
        assert!(settled_body.contains("spawn_blocking"));
        assert!(!settled_body.contains("logged_in: false"));
        let wait_body = bounded_function(production, "fn wait_for_probe_load");
        assert!(wait_body.contains("spawn_blocking"));
        assert!(wait_body.contains("recv_timeout"));
        assert!(!wait_body.contains("eval_with_callback"));
        assert!(!wait_body.contains("thread::sleep"));
    }

    #[test]
    fn macos_reuses_one_hidden_probe_and_other_platforms_keep_the_old_window_rule() {
        assert_eq!(account_probe_is_resident(), cfg!(target_os = "macos"));
        assert_eq!(
            plan_account_probe(true, false, true, false, false),
            AccountProbePlan::Create
        );
        assert_eq!(
            plan_account_probe(true, false, true, true, false),
            AccountProbePlan::Reuse
        );
        assert_eq!(
            plan_account_probe(true, false, false, true, true),
            AccountProbePlan::Recreate
        );
        assert_eq!(
            plan_account_probe(true, true, false, true, false),
            AccountProbePlan::Blocked
        );
        assert_eq!(
            plan_account_probe(false, true, false, false, false),
            AccountProbePlan::Blocked
        );
        assert_eq!(
            plan_account_probe(false, false, true, true, true),
            AccountProbePlan::Visible
        );
        assert_eq!(
            plan_account_probe(false, false, false, false, false),
            AccountProbePlan::Create
        );
        assert_eq!(
            plan_account_probe(false, false, false, true, false),
            AccountProbePlan::Recreate
        );
        assert!(account_probe_alongside_browser(true, true));
        assert!(!account_probe_alongside_browser(false, true));
        assert!(account_probe_alongside_browser(false, false));
        assert!(!drop_probe_before_visible_browser(true));
        assert!(drop_probe_before_visible_browser(false));
        assert!(!drop_probe_after_account_read(true));
        assert!(drop_probe_after_account_read(false));

        let production = include_str!("browser.rs").split("mod tests").next().unwrap();
        let reused = bounded_function(production, "fn reload_resident_probe");
        assert!(reused.contains("PROBE_LABEL"));
        assert!(reused.contains("probe_reload_script"));
        assert!(reused.contains("wait_for_probe_load"));
        assert!(reused.contains("read_settled_account"));
        let reload_at = reused.find("probe_reload_script").unwrap();
        let wait_at = reused.find("wait_for_probe_load").unwrap();
        let read_at = reused.find("read_settled_account").unwrap();
        assert!(reload_at < wait_at && wait_at < read_at);
        assert!(!reused.contains("booth_view"));
        assert!(!reused.contains("active_view"));
        assert!(!reused.contains("set_focus"));
        assert!(!reused.contains(".show("));
        assert_eq!(
            probe_reload_script(),
            "location.replace(\"https://accounts.booth.pm/library\")"
        );

        let release = bounded_function(production, "fn release_probe_for_displayed_browser");
        assert!(release.find("return").unwrap() < release.find("destroy()").unwrap());
        assert!(release.contains("drop_probe_before_visible_browser"));
        let surface = include_str!("browser_surface.rs");
        assert!(surface.contains("release_probe_for_displayed_browser"));
        assert!(!surface.contains("background_throttling"));
        let opened = bounded_function(surface, "fn build_browser_window");
        assert!(opened.contains("release_probe_for_displayed_browser"));
        assert!(!opened.contains("booth-probe"));
        let panel = bounded_function(surface, "fn open_panel");
        assert!(panel.contains("release_probe_for_displayed_browser"));
        let close = bounded_function(surface, "pub fn close");
        assert!(!close.contains("booth-probe"));
        assert!(!close.contains("PROBE_LABEL"));
        assert!(!close.contains("release_probe_for_displayed_browser"));
        let lib = include_str!("lib.rs");
        assert!(lib.contains("on_web_content_process_terminate"));
        assert!(lib.contains("recover_web_content_process"));
        let hook = lib.find("on_web_content_process_terminate").unwrap();
        let macos = lib[..hook].rfind("target_os = \"macos\"").unwrap();
        assert!(macos < hook);
    }

    #[test]
    fn terminated_views_reload_but_the_probe_waits_for_an_account_check() {
        let mut reloads = Vec::new();
        for label in [
            "main",
            "booth-toolbar-1",
            "booth-tab-1",
            "booth-probe-other",
        ] {
            recover_web_content_process(label, || reloads.push(label));
            assert!(!probe_process_is_dead());
        }
        assert_eq!(
            reloads,
            ["main", "booth-toolbar-1", "booth-tab-1", "booth-probe-other"]
        );

        recover_web_content_process(PROBE_LABEL, || {
            panic!("the probe must be recreated, not reloaded")
        });
        assert!(probe_process_is_dead());
        assert_eq!(
            plan_account_probe(true, false, true, true, probe_process_is_dead()),
            AccountProbePlan::Recreate
        );
        recover_web_content_process("main", || reloads.push("main"));
        assert_eq!(reloads.last(), Some(&"main"));
        assert!(probe_process_is_dead());
        PROBE_PROCESS_DEAD.store(false, Ordering::SeqCst);
    }

    fn bounded_function<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let rest = &source[start..];
        let after = rest.find('\n').unwrap() + 1;
        let tail = &rest[after..];
        let end = [
            "\nfn ",
            "\npub fn ",
            "\npub(crate) fn ",
            "\npub async fn ",
            "\nasync fn ",
            "\nstruct ",
            "\nenum ",
            "\nimpl ",
        ]
        .iter()
        .filter_map(|marker| tail.find(marker))
        .min()
        .unwrap_or(tail.len());
        &rest[..after + end]
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
