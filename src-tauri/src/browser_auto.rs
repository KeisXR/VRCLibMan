//! Observe user purchases/downloads. This module never initiates a download or payment.
use crate::{AppState, browser, db};
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock, mpsc},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, Webview, webview::DownloadEvent};
use url::Url;

const SCRIPT: &str = include_str!("browser_auto.cjs");
const EXTRACT: &str = include_str!("../../extension/extract.js");

pub(crate) fn initialization_script() -> String {
    format!(
        r#"(function() {{
      document.addEventListener('click', function(event) {{
        if (!event.isTrusted) return;
        globalThis.__vrclibmanDownload = null;
        const anchor = event.target.closest?.('a[href]');
        if (!anchor) return;
        try {{
          const url = new URL(anchor.href, location.href);
          if (url.protocol !== 'https:' || !(url.hostname === 'booth.pm' || url.hostname.endsWith('.booth.pm'))) return;
          const match = url.pathname.match(/^\/downloadables\/(\d{{4,12}})$/);
          if (!match) return;
          globalThis.__vrclibmanDownload = {{ downloadableId: match[1], href: location.href, at: Date.now() }};
        }} catch (_) {{}}
      }}, true);
    }})()"#
    )
}

fn booth_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url
            .host_str()
            .is_some_and(|h| h == "booth.pm" || h.ends_with(".booth.pm"))
}

pub(crate) fn checkout_url(url: &Url) -> bool {
    booth_url(url)
        && url.host_str() == Some("checkout.booth.pm")
        && url
            .path()
            .strip_prefix("/orders/")
            .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()))
}

fn enabled(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .db
        .lock()
        .ok()
        .and_then(|c| db::settings(&c).ok())
        .is_some_and(|s| s.builtin_browser && s.browser_auto_register)
}

fn decoded(raw: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "ページの結果を読めません")?;
    if let Some(inner) = value.as_str() {
        serde_json::from_str(inner).map_err(|_| "ページの結果を読めません".into())
    } else {
        Ok(value)
    }
}

fn changed(app: &AppHandle, prompt: bool) {
    let _ = app.emit("library-changed", ());
    let _ = app.emit_to("main", "browser-auto-pending", ());
    if prompt {
        crate::presence::show_main_later(app);
    }
}

pub(crate) fn page_loaded(view: &Webview, url: &Url) {
    if crate::purchase_walk::orders_path(url).is_some() {
        orders_page_loaded(view, url);
        return;
    }
    if !checkout_url(url) || browser::library_walk_running() || !enabled(view.app_handle()) {
        return;
    }
    let href = url.as_str().to_string();
    let order_id = url
        .path()
        .strip_prefix("/orders/")
        .unwrap_or_default()
        .to_string();
    let app = view.app_handle().clone();
    let tab = view.label().to_string();
    let script = format!(
        "(function() {{ {SCRIPT} return {{ href: location.href, products: VrcLibManAuto.checkout(document, location.href) }}; }})()"
    );
    let _ = view.eval_with_callback(script, move |raw| {
        let Ok(value) = decoded(&raw) else {
            return;
        };
        if value.get("href").and_then(|v| v.as_str()) != Some(&href) {
            return;
        }
        if app
            .get_webview(&tab)
            .and_then(|v| crate::browser_url::current_url(&v).ok())
            .as_ref()
            .map(Url::as_str)
            != Some(&href)
        {
            return;
        }
        let app = app.clone();
        let tab = tab.clone();
        let order_id = order_id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let products: Vec<crate::purchases::CheckoutProduct> = value
                .get("products")
                .cloned()
                .and_then(|products| serde_json::from_value(products).ok())
                .unwrap_or_default();
            // 購入履歴への記録は、ライブラリへの登録とは別に行う。どちらかの失敗で他方を止めない。
            let recorded = app
                .state::<AppState>()
                .db
                .lock()
                .ok()
                .and_then(|mut c| crate::purchases::record_checkout(&mut c, &order_id, &products).ok())
                .unwrap_or(false);
            if recorded {
                let _ = app.emit("purchases-changed", ());
                browser::read_order_later(&app, &tab, &order_id);
            }
            let result = (|| {
                let state = app.state::<AppState>();
                let mut c = state
                    .db
                    .lock()
                    .map_err(|_| "データベースをロックできません")?;
                let before = db::browser_auto_pending(&c)?.len();
                for product in value
                    .get("products")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                {
                    db::browser_auto_register(&mut c, product)?;
                }
                Ok::<_, String>(db::browser_auto_pending(&c)?.len() > before)
            })();
            match result {
                Ok(prompt) => changed(&app, prompt),
                Err(_) => browser::browser_log(
                    &app,
                    "購入商品の自動登録に失敗しました。確認画面から登録してください",
                ),
            }
        });
    });
}

/// 本人が内蔵ブラウザで購入履歴か注文詳細を開いたとき、そのページだけを購入履歴に取り込む。
/// 読み込みの最中と自動登録がオフのときは読まない。ページの移動はしない。
fn orders_page_loaded(view: &Webview, url: &Url) {
    if browser::library_walk_running() || !enabled(view.app_handle()) {
        return;
    }
    let detail_id = match crate::purchase_walk::orders_path(url) {
        Some(crate::purchase_walk::OrdersPath::Detail(id)) => Some(id),
        Some(crate::purchase_walk::OrdersPath::List) => None,
        None => return,
    };
    let script = if detail_id.is_some() {
        browser::purchase_detail_script()
    } else {
        browser::purchase_list_script()
    };
    let href = url.as_str().to_string();
    let app = view.app_handle().clone();
    let tab = view.label().to_string();
    let _ = view.eval_with_callback(script, move |raw| {
        if app
            .get_webview(&tab)
            .and_then(|v| crate::browser_url::current_url(&v).ok())
            .as_ref()
            .map(Url::as_str)
            != Some(&href)
        {
            return;
        }
        let app = app.clone();
        let detail_id = detail_id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let state = app.state::<AppState>();
            let Ok(mut c) = state.db.lock() else {
                return;
            };
            let stored = match &detail_id {
                Some(id) => browser::parse_purchase_detail(&raw)
                    .ok()
                    .and_then(|snapshot| snapshot.detail)
                    .filter(|detail| &detail.order_id == id)
                    .is_some_and(|detail| crate::purchases::apply_detail(&mut c, &detail).is_ok()),
                None => browser::parse_purchase_list(&raw)
                    .ok()
                    .is_some_and(|snapshot| crate::purchases::apply_list_page(&mut c, &snapshot.orders).is_ok()),
            };
            drop(c);
            if stored {
                let _ = app.emit("purchases-changed", ());
            }
        });
    });
}

#[derive(Deserialize)]
struct Choice {
    product: serde_json::Value,
    href: String,
    #[serde(default)]
    variation: String,
    filename: String,
    #[serde(rename = "downloadableId")]
    downloadable_id: String,
}
struct Pending {
    finished: mpsc::Sender<Option<PathBuf>>,
    expected: PathBuf,
    token: u64,
}
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
type Key = (String, String);
static PENDING: OnceLock<Mutex<HashMap<Key, Pending>>> = OnceLock::new();
fn pending() -> &'static Mutex<HashMap<Key, Pending>> {
    PENDING.get_or_init(Default::default)
}

pub(crate) fn handle_download(view: &Webview, event: &mut DownloadEvent<'_>) {
    match event {
        DownloadEvent::Requested { url, destination } => {
            if browser::library_walk_running() || !enabled(view.app_handle()) {
                return;
            }
            let Ok(source) = crate::browser_url::current_url(view) else {
                return;
            };
            if !booth_url(&source) || url.scheme() != "https" {
                return;
            }
            let key = (view.label().to_string(), url.as_str().to_string());
            let filename = destination
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !crate::library_files::is_packaged_asset(&filename) {
                return;
            }
            let Ok(downloads) = crate::library_files::downloads_folder() else {
                return;
            };
            if std::fs::create_dir_all(&downloads).is_err() {
                return;
            }
            let Ok(dir) = tempfile::Builder::new()
                .prefix("VRCLibMan-")
                .tempdir_in(downloads)
            else {
                return;
            };
            let expected = dir.keep().join(&filename);
            let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let (tx, rx) = mpsc::channel();
            if let Ok(mut map) = pending().lock() {
                // An overlapping identical request is ambiguous; register neither.
                if map.remove(&key).is_some() {
                    return;
                }
                map.insert(
                    key.clone(),
                    Pending {
                        finished: tx,
                        expected: expected.clone(),
                        token,
                    },
                );
                **destination = expected.clone();
            } else {
                return;
            }
            let (snapshot_tx, snapshot_rx) = mpsc::channel();
            let script = format!(
                r#"(function() {{
              const click = globalThis.__vrclibmanDownload;
              globalThis.__vrclibmanDownload = null;
              if (!click || click.href !== location.href || Date.now() - click.at > 15000) return null;
              {EXTRACT}
              {SCRIPT}
              const choice = VrcLibManAuto.chooseDownload(VrcLibManExtract.extractPage(document, location.href), click.downloadableId);
              return choice ? {{ ...choice, href: location.href }} : null;
            }})()"#
            );
            let _ = view.eval_with_callback(script, move |raw| {
                let _ = snapshot_tx.send(raw);
            });
            let app = view.app_handle().clone();

            tauri::async_runtime::spawn_blocking(move || {
                let result = (|| {
                    let raw = snapshot_rx
                        .recv_timeout(Duration::from_secs(8))
                        .map_err(|_| "取得元を確認できません")?;
                    let choice: Choice = serde_json::from_value(decoded(&raw)?)
                        .map_err(|_| "取得元を特定できません")?;
                    if choice.href != source.as_str() {
                        return Err("取得元のページが変わりました".into());
                    }
                    let path = rx
                        .recv_timeout(Duration::from_secs(3600))
                        .ok()
                        .flatten()
                        .ok_or("保存が完了していません")?;
                    if path != expected {
                        return Err("保存先が変わりました".into());
                    }
                    let prompt = {
                        let state = app.state::<AppState>();
                        let mut c = state
                            .db
                            .lock()
                            .map_err(|_| "データベースをロックできません")?;
                        complete_download(&mut c, &choice, &path, token)?
                    };
                    changed(&app, prompt);
                    Ok::<_, String>(())
                })();
                if let Ok(mut map) = pending().lock() {
                    if map.get(&key).is_some_and(|p| p.token == token) {
                        map.remove(&key);
                    }
                }
                if result.is_err() {
                    browser::browser_log(
                        &app,
                        "ダウンロードの自動登録を見送りました。商品とファイルは確認画面から登録できます",
                    );
                }
            });
        }
        DownloadEvent::Finished {
            url, path, success, ..
        } => {
            if let Ok(mut map) = pending().lock() {
                if let Some(job) = map.remove(&(view.label().to_string(), url.as_str().to_string()))
                {
                    let _ = job
                        .finished
                        .send(completed_path(*success, path.clone(), job.expected));
                }
            }
        }
        _ => {}
    }
}

fn completed_path(success: bool, reported: Option<PathBuf>, expected: PathBuf) -> Option<PathBuf> {
    if !success {
        return None;
    }
    let path = reported.unwrap_or_else(|| expected.clone());
    (path == expected).then_some(path)
}

fn complete_download(
    c: &mut rusqlite::Connection,
    choice: &Choice,
    path: &PathBuf,
    token: u64,
) -> Result<bool, String> {
    let meta = std::fs::symlink_metadata(&path).map_err(|_| "保存ファイルを確認できません")?;
    if !meta.file_type().is_file()
        || !crate::library_files::is_packaged_asset(
            path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        )
        || crate::library_files::file_looks_like_login_page(&path)
    {
        return Err("アセットファイルではありません".into());
    }
    let before = db::browser_auto_pending(c)?.len();
    let Some(id) = db::browser_auto_register(c, &choice.product)? else {
        return Ok(false);
    };
    let saved = db::settings(c)?;
    let routed = saved.route_downloads && !saved.library_folder.trim().is_empty();
    let placed = if routed {
        let downloads = crate::library_files::downloads_folder()?;
        let dir = crate::library_files::prepare_staging_dir(&downloads, &id)?;
        let mut tmp =
            tempfile::NamedTempFile::new_in(&dir).map_err(|_| "整理用ファイルを作れません")?;
        let mut input = std::fs::File::open(&path).map_err(|_| "保存ファイルを読めません")?;
        std::io::copy(&mut input, tmp.as_file_mut())
            .map_err(|_| "整理用ファイルをコピーできません")?;
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("asset.zip");
        let mut staged = dir.join(filename);
        if staged.exists() {
            staged = dir.join(format!("{}-{filename}", token));
        }
        tmp.persist_noclobber(&staged)
            .map_err(|_| "整理用ファイルを保存できません")?;
        crate::library_files::place_into_library(
            &staged,
            Some(std::path::Path::new(&saved.library_folder)),
            &id,
        )?
    } else {
        path.clone()
    };
    db::adopt_download_filename(
        c,
        &id,
        &choice.variation,
        &choice.filename,
        placed.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        &choice.downloadable_id,
    )?;
    db::record_placed_download(c, &id, &choice.variation, &placed, Some(meta.len()))?;
    c.execute("UPDATE product_files SET downloadable_id = ?1 WHERE booth_id = ?2 AND variation = ?3 AND local_path = ?4",
    rusqlite::params![choice.downloadable_id, id, choice.variation, placed.display().to_string()]).map_err(|_| "ファイル番号を保存できません")?;
    if routed && placed != *path {
        let _ = std::fs::remove_file(&path);
    }
    Ok(db::browser_auto_pending(c)?.len() > before)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkout_scope_is_exact() {
        assert!(checkout_url(
            &Url::parse("https://checkout.booth.pm/orders/123456?uuid=unused").unwrap()
        ));
        for url in [
            "http://checkout.booth.pm/orders/1234",
            "https://evil.test/orders/1234",
            "https://checkout.booth.pm/orders/1234/other",
            "https://checkout.booth.pm/orders/",
            "https://user@checkout.booth.pm/orders/1234",
        ] {
            assert!(!checkout_url(&Url::parse(url).unwrap()), "{url}");
        }
    }
    #[test]
    fn completion_requires_success_and_our_own_destination_including_macos() {
        let expected = PathBuf::from("/Downloads/VRCLibMan-example/sample.zip");
        assert_eq!(
            completed_path(true, None, expected.clone()),
            Some(expected.clone())
        );
        assert_eq!(
            completed_path(false, Some(expected.clone()), expected.clone()),
            None
        );
        assert_eq!(
            completed_path(true, Some(PathBuf::from("/elsewhere/sample.zip")), expected),
            None
        );
    }
    #[test]
    fn completed_file_is_registered_in_place_with_its_file_id_and_variation() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = db::open(&dir.path().join("test.sqlite")).unwrap();
        db::update_settings(
            &mut c,
            &crate::model::SettingsPatch {
                builtin_browser: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let file = dir.path().join("sample.zip");
        std::fs::write(&file, b"PK\x03\x04example").unwrap();
        let choice = Choice {
            product: serde_json::json!({"boothId":"123456", "title":"見本の衣装"}),
            href: "https://booth.pm/ja/items/123456".into(),
            variation: "通常".into(),
            filename: "sample.zip".into(),
            downloadable_id: "345678".into(),
        };
        assert!(complete_download(&mut c, &choice, &file, 1).unwrap());
        assert!(file.exists(), "routing is off, so keep the original file");
        let row: (String, String, i64) = c.query_row("SELECT local_path, downloadable_id, downloaded FROM product_files WHERE booth_id = '123456' AND variation = '通常'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(row, (file.display().to_string(), "345678".into(), 1));
        assert!(!complete_download(&mut c, &choice, &file, 2).unwrap());
        assert_eq!(db::browser_auto_pending(&c).unwrap().len(), 1);
        db::update_settings(
            &mut c,
            &crate::model::SettingsPatch {
                browser_auto_register: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        let other = Choice {
            product: serde_json::json!({"boothId":"234567", "title":"未登録の見本"}),
            ..choice
        };
        assert!(!complete_download(&mut c, &other, &file, 3).unwrap());
        assert!(file.exists());
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM products WHERE booth_id = '234567'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
    #[test]
    fn login_html_saved_with_archive_name_is_not_registered() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = db::open(&dir.path().join("test.sqlite")).unwrap();
        db::update_settings(
            &mut c,
            &crate::model::SettingsPatch {
                builtin_browser: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let file = dir.path().join("login.zip");
        std::fs::write(&file, b"<!DOCTYPE html><html><body>Login</body></html>").unwrap();
        let choice = Choice {
            product: serde_json::json!({"boothId":"123456", "title":"見本"}),
            href: "https://booth.pm/ja/items/123456".into(),
            variation: String::new(),
            filename: "login.zip".into(),
            downloadable_id: "345678".into(),
        };
        assert!(complete_download(&mut c, &choice, &file, 1).is_err());
        assert!(db::browser_auto_pending(&c).unwrap().is_empty());
        assert!(file.exists());
    }
}
