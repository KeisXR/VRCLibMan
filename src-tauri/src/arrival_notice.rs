//! Desktop notifications for what arrives from the browser while VRCLibMan is not in front:
//! another app is active, the window is minimized, or only the tray icon is left.
//!
//! A product that waits for its classification used to bring the main window forward. Windows
//! often refuses that to a background app and only flashes the taskbar, and a tray launch has no
//! window at all, so the request went unseen. While the app is not in front it now notifies
//! instead, and a click on the notification opens the window. The in-app banners and the
//! classification dialog still show once the window is open.
//!
//! On Windows the toast opens `vrclibman://open` when clicked (protocol activation). Windows
//! does that from the Action Center too, after the toast left the screen, and launches the app if
//! it has quit; the single-instance plugin hands the URL to a running app, which shows its window.
//! A click reported back to the process, as the notification plugin does, only arrives while the
//! toast is on screen.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;

/// Files and products delivered in a burst (the extension resends what it kept while the app was
/// closed) become one notification.
const GATHER: Duration = Duration::from_millis(2500);
/// Longest product title quoted in a notification.
const TITLE_CHARS: usize = 40;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Pending {
    /// One entry per file placed in the library: the product title, or `None` when it must not
    /// appear (age restricted, or unknown). Notification centers keep a history.
    files: Vec<Option<String>>,
    added: u32,
    updated: u32,
    classify: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.files.is_empty() && self.added == 0 && self.updated == 0 && !self.classify
    }
}

#[derive(Default)]
pub(crate) struct ArrivalNotices {
    pending: Mutex<Pending>,
    scheduled: Mutex<bool>,
}

/// Registers the state and opens the main window when a notification is clicked.
pub(crate) fn init(app: &AppHandle) {
    app.manage(ArrivalNotices::default());
    // macOS and Linux report the click to the process. Windows opens the URL instead.
    #[cfg(not(windows))]
    {
        let handle = app.clone();
        let _ = app.notification().on_action(move |_| {
            crate::presence::show_main_later(&handle);
        });
    }
}

/// Whether one of the app's windows is in front. The built-in browser may be a window of its own.
pub(crate) fn app_in_front<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.windows().values().any(|window| {
        window.is_focused().unwrap_or(false)
            && window.is_visible().unwrap_or(false)
            && !window.is_minimized().unwrap_or(false)
    })
}

/// A file from the extension was placed in the library.
pub(crate) fn file_added(app: &AppHandle, booth_id: &str) {
    if app_in_front(app) {
        return;
    }
    let title = app.try_state::<crate::AppState>().and_then(|state| {
        let connection = state.db.lock().ok()?;
        connection
            .query_row(
                "SELECT title FROM products WHERE booth_id = ?1 AND IFNULL(is_adult, 0) = 0",
                [booth_id],
                |row| row.get::<_, String>(0),
            )
            .ok()
    });
    queue(app, |pending| pending.files.push(title));
}

/// Product information from the extension was saved.
pub(crate) fn products_received(app: &AppHandle, added: u32, updated: u32) {
    if added + updated == 0 || app_in_front(app) {
        return;
    }
    queue(app, |pending| {
        pending.added += added;
        pending.updated += updated;
    });
}

/// A product waits for the user's classification. In front, the main window comes forward as
/// before; otherwise the notification asks for it and the window stays where it is.
pub(crate) fn ask_to_classify(app: &AppHandle) {
    if app_in_front(app) {
        crate::presence::show_main_later(app);
        return;
    }
    queue(app, |pending| pending.classify = true);
}

fn queue(app: &AppHandle, add: impl FnOnce(&mut Pending)) {
    let Some(notices) = app.try_state::<ArrivalNotices>() else {
        return;
    };
    if let Ok(mut pending) = notices.pending.lock() {
        add(&mut pending);
    }
    let Ok(mut scheduled) = notices.scheduled.lock() else {
        return;
    };
    if *scheduled {
        return;
    }
    *scheduled = true;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(GATHER);
        flush(&app);
    });
}

fn flush(app: &AppHandle) {
    let Some(notices) = app.try_state::<ArrivalNotices>() else {
        return;
    };
    // Unscheduled first: whatever arrives after the take starts a new gathering.
    if let Ok(mut scheduled) = notices.scheduled.lock() {
        *scheduled = false;
    }
    let pending = notices
        .pending
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    if app_in_front(app) {
        // The user came back meanwhile; the window shows the rest.
        if pending.classify {
            crate::presence::show_main_later(app);
        }
        return;
    }
    let Some((title, body)) = message(&pending) else {
        return;
    };
    if let Err(error) = show(app, &title, &body) {
        eprintln!("通知を出せません: {error}");
    }
}

#[cfg(not(windows))]
fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| error.to_string())
}

/// The URL a click on a Windows toast opens.
#[cfg_attr(not(windows), allow(dead_code))]
const OPEN_URL: &str = "vrclibman://open";
/// What Windows shows as the sender of a build that is not installed (the same fallback as the
/// notification plugin): only an installed app has a Start menu shortcut with its identifier.
#[cfg(windows)]
const POWERSHELL_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

#[cfg(windows)]
fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::HSTRING;

    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()))
        .is_some_and(|dir| !(dir.ends_with("target\\debug") || dir.ends_with("target\\release")));
    let app_id = if installed {
        app.config().identifier.clone()
    } else {
        POWERSHELL_APP_ID.to_string()
    };
    let failed = |error: windows::core::Error| error.message().to_string();
    let document = XmlDocument::new().map_err(failed)?;
    document
        .LoadXml(&HSTRING::from(toast_xml(title, body)))
        .map_err(failed)?;
    let toast = ToastNotification::CreateToastNotification(&document).map_err(failed)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
        .and_then(|notifier| notifier.Show(&toast))
        .map_err(failed)
}

/// A Windows toast that opens `OPEN_URL` when clicked, on screen or in the Action Center.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn toast_xml(title: &str, body: &str) -> String {
    let lines = std::iter::once(title)
        .chain(body.lines())
        .map(|line| format!("<text>{}</text>", escape(line)))
        .collect::<String>();
    format!(
        r#"<toast activationType="protocol" launch="{OPEN_URL}"><visual><binding template="ToastGeneric">{lines}</binding></visual></toast>"#
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The notification's title and body.
pub(crate) fn message(pending: &Pending) -> Option<(String, String)> {
    if pending.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    match pending.files.as_slice() {
        [] => {}
        [Some(title)] => lines.push(format!("「{}」をライブラリに追加しました", short(title))),
        files => lines.push(format!(
            "{} 件のファイルをライブラリに追加しました",
            files.len()
        )),
    }
    if pending.added > 0 {
        lines.push(format!("{} 件の商品を追加しました", pending.added));
    }
    if pending.updated > 0 {
        lines.push(format!("{} 件の商品情報を更新しました", pending.updated));
    }
    let title = if pending.classify {
        lines.push("クリックすると VRCLibMan を開きます".into());
        "取得した商品の分類を選んでください"
    } else if !pending.files.is_empty() {
        "ファイルを追加しました"
    } else {
        "商品を読み込みました"
    };
    Some((title.into(), lines.join("\n")))
}

fn short(title: &str) -> String {
    let title = title.trim();
    if title.chars().count() <= TITLE_CHARS {
        return title.into();
    }
    let mut cut: String = title.chars().take(TITLE_CHARS - 1).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_pending_shows_nothing() {
        assert_eq!(message(&Pending::default()), None);
    }

    #[test]
    fn one_file_names_its_product_and_hidden_titles_are_only_counted() {
        let one = Pending {
            files: vec![Some("ふわふわパーカー".into())],
            ..Default::default()
        };
        assert_eq!(
            message(&one),
            Some((
                "ファイルを追加しました".into(),
                "「ふわふわパーカー」をライブラリに追加しました".into()
            ))
        );
        let hidden = Pending {
            files: vec![None],
            ..Default::default()
        };
        assert_eq!(
            message(&hidden).unwrap().1,
            "1 件のファイルをライブラリに追加しました"
        );
        let many = Pending {
            files: vec![Some("A".into()), None, Some("B".into())],
            ..Default::default()
        };
        assert_eq!(
            message(&many).unwrap().1,
            "3 件のファイルをライブラリに追加しました"
        );
    }

    #[test]
    fn a_waiting_classification_leads_the_notification() {
        let pending = Pending {
            files: vec![None, None],
            added: 1,
            classify: true,
            ..Default::default()
        };
        let (title, body) = message(&pending).unwrap();
        assert_eq!(title, "取得した商品の分類を選んでください");
        assert_eq!(
            body,
            "2 件のファイルをライブラリに追加しました\n1 件の商品を追加しました\nクリックすると VRCLibMan を開きます"
        );
    }

    #[test]
    fn product_counts_without_files() {
        let pending = Pending {
            added: 3,
            updated: 2,
            ..Default::default()
        };
        assert_eq!(
            message(&pending),
            Some((
                "商品を読み込みました".into(),
                "3 件の商品を追加しました\n2 件の商品情報を更新しました".into()
            ))
        );
    }

    #[test]
    fn long_titles_are_shortened() {
        let long = "あ".repeat(60);
        let text = short(&long);
        assert_eq!(text.chars().count(), TITLE_CHARS);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn a_windows_toast_opens_the_app_url_and_escapes_titles() {
        let xml = toast_xml(
            "ファイルを追加しました",
            "「A&B <髪>」をライブラリに追加しました\n2 件の商品を追加しました",
        );
        assert!(xml.starts_with(r#"<toast activationType="protocol" launch="vrclibman://open">"#));
        assert!(xml.contains("<text>ファイルを追加しました</text>"));
        assert!(xml.contains("<text>「A&amp;B &lt;髪&gt;」をライブラリに追加しました</text>"));
        assert!(xml.contains("<text>2 件の商品を追加しました</text>"));
    }
}
