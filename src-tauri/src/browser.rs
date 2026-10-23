use crate::db;
use crate::model::ImportReport;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

pub const WINDOW_LABEL: &str = "booth";
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
    browser_enabled(app)?;
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let url = Url::parse(LIBRARY_URL).map_err(|err| err.to_string())?;
    let mut builder = WebviewWindowBuilder::new(app, WINDOW_LABEL, WebviewUrl::External(url))
        .title("BOOTH")
        .inner_size(1100.0, 800.0)
        .focused(true);
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
        builder = builder.data_directory(dir);
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.data_store_identifier(BOOTH_DATA_STORE_ID);
    }
    builder
        .build()
        .map_err(|err| format!("ブラウザを開けません: {err}"))?;
    Ok(())
}

pub fn show_library(app: &AppHandle) -> Result<(), String> {
    browser_enabled(app)?;
    open_browser(app)?;
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or("ブラウザを開いてから移動してください")?;
    let url = Url::parse(LIBRARY_URL).map_err(|err| err.to_string())?;
    window
        .navigate(url)
        .map_err(|err| format!("ライブラリを開けません: {err}"))
}

pub fn browse_history(app: &AppHandle, forward: bool) -> Result<(), String> {
    browser_enabled(app)?;
    let window = app
        .get_webview_window(WINDOW_LABEL)
        .ok_or("ブラウザを開いてから移動してください")?;
    let script = if forward {
        "history.forward()"
    } else {
        "history.back()"
    };
    window
        .eval(script)
        .map_err(|err| format!("移動できません: {err}"))
}

pub fn import_current_page(app: &AppHandle) -> Result<ImportReport, String> {
    // ページの結果を待つ間、UI スレッドを止めない。待ちはコマンド側の spawn_blocking に置く。
    browser_enabled(app)?;
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
    library_page_for_import(&href)?;
    let state = app.state::<crate::AppState>();
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "データベースをロックできません".to_string())?;
    db::import_export_json(&mut connection, &export_json(products))
}

pub fn close_browser(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.destroy();
    }
    Ok(())
}

pub async fn clear_session(app: &AppHandle) -> Result<(), String> {
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
    let handle = app.clone();
    dispatch_ui(handle, |app| {
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = window.destroy();
        }
        Ok(())
    })
    .await?;
    let root = local_data_dir(app)?;
    let dir = session_dir(&root);
    if !session_dir_is_safe(&root, &dir) {
        return Err("セッションの場所が不正です".into());
    }
    for _ in 0..10 {
        if !dir.exists() {
            return Ok(());
        }
        if std::fs::remove_dir_all(&dir).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err("セッションのフォルダを消せませんでした。ブラウザを閉じてからもう一度押してください".into())
}

#[cfg(target_os = "macos")]
async fn clear_macos_data_store(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.clear_all_browsing_data();
        let _ = window.destroy();
        std::thread::sleep(Duration::from_millis(300));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_open_library_page_can_be_imported() {
        assert!(library_page_for_import("https://accounts.booth.pm/library").is_ok());
        assert!(library_page_for_import("https://accounts.booth.pm/library?page=2").is_ok());
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
            let start = commands.find(&marker).unwrap_or_else(|| panic!("{name} は async のままにする"));
            let rest = &commands[start..];
            let end = rest[marker.len()..]
                .find("\n#[tauri::command]")
                .map(|index| marker.len() + index)
                .unwrap_or(rest.len());
            let body = &rest[..end];
            assert!(
                body.contains("dispatch_ui"),
                "{name} は IPC の外で窓を触る"
            );
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
}
