use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager, Runtime};

pub(crate) const AUTOSTART_ARG: &str = "--vrclibman-autostart";
pub(crate) const MAIN_WINDOW: &str = "main";
const TRAY_ID: &str = "vrclibman";

pub(crate) fn background_supported() -> bool {
    cfg!(any(windows, target_os = "macos", target_os = "linux"))
}

/// What closing the main window should do.
/// `Quit` ends the process. `DestroyWindowKeepProcess` drops the webview and leaves the server up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseDecision {
    Quit,
    DestroyWindowKeepProcess,
}

pub(crate) fn decide_close(background_mode: bool) -> CloseDecision {
    if background_supported() && background_mode {
        CloseDecision::DestroyWindowKeepProcess
    } else {
        CloseDecision::Quit
    }
}

pub(crate) fn normalize_presence(background_mode: bool, launch_at_login: bool) -> (bool, bool) {
    if background_supported() && background_mode {
        (true, launch_at_login)
    } else {
        (false, false)
    }
}

pub(crate) fn autostart_should_enable(background_mode: bool, launch_at_login: bool) -> bool {
    let (background_mode, launch_at_login) = normalize_presence(background_mode, launch_at_login);
    background_mode && launch_at_login
}

pub(crate) fn launched_by_login_item<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> bool {
    args.into_iter().any(|arg| arg.as_ref() == AUTOSTART_ARG)
}

/// A windowless start is only for a login launch while background mode is on.
/// Background mode off always opens a window, including a leftover login launch.
pub(crate) fn should_open_window_on_start(background_mode: bool, launched_by_login: bool) -> bool {
    !(background_supported() && background_mode && launched_by_login)
}

pub(crate) fn remember_background<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        state
            .background_mode
            .store(background_supported() && enabled, Ordering::SeqCst);
    }
}

pub(crate) fn background_mode_enabled<R: Runtime>(app: &AppHandle<R>) -> bool {
    can_background(background_requested(app), tray_ready(app))
}

pub(crate) fn background_requested<R: Runtime>(app: &AppHandle<R>) -> bool {
    background_supported()
        && app
            .try_state::<crate::AppState>()
            .map(|state| state.background_mode.load(Ordering::SeqCst))
            .unwrap_or(false)
}

fn can_background(requested: bool, ready: bool) -> bool {
    requested && ready
}

fn tray_ready<R: Runtime>(app: &AppHandle<R>) -> bool {
    #[cfg(target_os = "linux")]
    if !crate::presence_linux::host_available() || !crate::presence_linux::tray_visible() {
        return false;
    }
    app.tray_by_id(TRAY_ID).is_some()
}

pub(crate) fn start(
    app: &AppHandle,
    background_mode: bool,
    launch_at_login: bool,
) -> Result<(), String> {
    if let Err(error) = sync_login_item(
        app,
        autostart_should_enable(background_mode, launch_at_login),
    ) {
        eprintln!("{error}");
    }
    let login_launch = launched_by_login_item(std::env::args());
    #[cfg(target_os = "linux")]
    {
        crate::presence_linux::start(app.clone(), login_launch);
        // An ordinary launch never waits for the desktop's tray service.
        if !background_mode || !login_launch {
            show_main_window(app)?;
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if background_mode {
            if let Err(error) = ensure_tray(app) {
                eprintln!("{error}");
            }
        }
        if should_open_window_on_start(background_mode_enabled(app), login_launch) {
            show_main_window(app)?;
        }
    }
    Ok(())
}

pub(crate) fn show_main_later<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    // Queue onto the main thread from another thread. A direct call, or
    // `run_on_main_thread` from the main thread, runs inside the current
    // callback and can deadlock WebView2 while a window is closing.
    std::thread::spawn(move || {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = show_main_window(&handle);
        });
    });
}

pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    // A docked browser adds child webviews, so main is no longer a WebviewWindow.
    if let Some(window) = app.get_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == MAIN_WINDOW)
        .ok_or("メインウィンドウの設定がありません")?;
    tauri::WebviewWindowBuilder::from_config(app, config)
        .map_err(|err| format!("ウィンドウを用意できません: {err}"))?
        .build()
        .map_err(|err| format!("ウィンドウを開けません: {err}"))?;
    Ok(())
}

pub(crate) fn handle_main_close<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    let tauri::WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    let app = window.app_handle();
    match decide_close(background_mode_enabled(app)) {
        CloseDecision::Quit => {
            let app = app.clone();
            std::thread::spawn(move || {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    remove_tray(&handle);
                    handle.exit(0);
                });
            });
        }
        CloseDecision::DestroyWindowKeepProcess => {
            api.prevent_close();
            let window = window.clone();
            let app = app.clone();
            std::thread::spawn(move || {
                let _ = app.run_on_main_thread(move || {
                    let _ = window.destroy();
                });
            });
        }
    }
}

pub(crate) fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: tauri::RunEvent) {
    match event {
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() && background_mode_enabled(app) {
                api.prevent_exit();
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            if background_mode_enabled(app) {
                show_main_later(app);
            }
        }
        _ => {}
    }
}

pub(crate) fn apply_presence<R: Runtime>(
    app: &AppHandle<R>,
    background_mode: bool,
    launch_at_login: bool,
) -> Result<(), String> {
    let (background_mode, launch_at_login) = normalize_presence(background_mode, launch_at_login);
    // Establish a recovery path before enabling windowless login launch.
    if background_mode {
        #[cfg(target_os = "linux")]
        let waiting_for_host =
            background_requested(app) && !crate::presence_linux::host_available();
        #[cfg(not(target_os = "linux"))]
        let waiting_for_host = false;
        // A saved preference survives a missing host; in that state the user
        // must still be able to turn off login launch without a tray error.
        if !waiting_for_host {
            ensure_tray(app)?;
        }
    } else {
        // Turning off the only recovery path must not leave an invisible process.
        if background_mode_enabled(app) {
            show_main_window(app)?;
        }
        remove_tray(app);
    }
    sync_login_item(
        app,
        autostart_should_enable(background_mode, launch_at_login),
    )
}

pub(crate) fn sync_login_item<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
    let enabled = background_supported() && enabled;
    #[cfg(target_os = "linux")]
    {
        return crate::presence_linux::sync_login_item(app, enabled);
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        let current = manager
            .is_enabled()
            .map_err(|_| "ログイン項目を確認できません".to_string())?;
        if current == enabled {
            return Ok(());
        }
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|_| "ログイン項目を更新できません".to_string())
    }
}

pub(crate) fn ensure_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

    #[cfg(target_os = "linux")]
    if !crate::presence_linux::host_available() {
        return Err("トレイの表示先が見つかりません。GNOME では AppIndicator 対応の拡張を有効にしてください。ウィンドウを表示して動作します".into());
    }
    if let Some(_tray) = app.tray_by_id(TRAY_ID) {
        #[cfg(target_os = "linux")]
        {
            _tray
                .set_visible(true)
                .map_err(|_| "トレイを表示できません".to_string())?;
            crate::presence_linux::set_tray_visible(true);
        }
        return Ok(());
    }
    let open = MenuItem::with_id(app, "tray-open", "開く", true, None::<&str>)
        .map_err(|_| "トレイのメニューを作れません".to_string())?;
    let quit = MenuItem::with_id(app, "tray-quit", "終了", true, None::<&str>)
        .map_err(|_| "トレイのメニューを作れません".to_string())?;
    let menu = Menu::with_items(app, &[&open, &quit])
        .map_err(|_| "トレイのメニューを作れません".to_string())?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("VRCLibMan")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-open" => show_main_later(app),
            "tray-quit" => {
                let app = app.clone();
                std::thread::spawn(move || {
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        remove_tray(&handle);
                        handle.exit(0);
                    });
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main_later(tray.app_handle());
            }
        });
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon(menu_bar_icon()).icon_as_template(true);
    }
    #[cfg(any(windows, target_os = "linux"))]
    {
        builder = if let Some(icon) = app.default_window_icon() {
            builder.icon(icon.clone())
        } else {
            builder.icon(menu_bar_icon())
        };
    }
    builder
        .build(app)
        .map_err(|_| "トレイを出せませんでした".to_string())?;
    #[cfg(target_os = "linux")]
    crate::presence_linux::set_tray_visible(true);
    Ok(())
}

pub(crate) fn remove_tray<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "linux")]
    {
        // libappindicator 0.9 retains its exported D-Bus object after drop. Keep
        // one native indicator and reuse it rather than colliding on its path.
        crate::presence_linux::set_tray_visible(false);
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_visible(false);
        }
    }
    #[cfg(not(target_os = "linux"))]
    drop(app.remove_tray_by_id(TRAY_ID));
}

/// The menu bar template image: the app icon's tile with the cards and the V cut out, the same
/// shape as `BrandMark` in `src/components/navIcons.tsx`. macOS shows tray icons 18 pt tall, so
/// 36 px is sharp at 2x; the tile is 16 pt with 1 pt of margin. Only the alpha matters.
fn menu_bar_icon() -> tauri::image::Image<'static> {
    const N: u32 = 36;
    const MARGIN: f32 = 2.0;
    const SAMPLES: u32 = 4;
    // Pixels per unit of the icon's 512 grid, whose tile spans 16..496.
    let scale = (N as f32 - 2.0 * MARGIN) / 480.0;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let mut hits = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = x as f32 + (sx as f32 + 0.5) / SAMPLES as f32 - MARGIN;
                    let py = y as f32 + (sy as f32 + 0.5) / SAMPLES as f32 - MARGIN;
                    if brand_mark_covers(px / scale + 16.0, py / scale + 16.0) {
                        hits += 1;
                    }
                }
            }
            let index = ((y * N + x) * 4) as usize;
            rgba[index + 3] = (hits * 255 / (SAMPLES * SAMPLES)) as u8;
        }
    }
    tauri::image::Image::new_owned(rgba, N, N)
}

/// Whether a point on the icon's 512 grid is painted: inside the tile, outside the back card's
/// outline and the front card, or on the V. Geometry from `public/vrclibman.svg`, with the
/// thicker small-size strokes (28 and 40).
fn brand_mark_covers(x: f32, y: f32) -> bool {
    let tile = rounded_rect_distance(x, y, [16.0, 16.0, 480.0, 480.0], 104.0) <= 0.0;
    let back = rounded_rect_distance(x, y, [112.0, 104.0, 248.0, 272.0], 32.0).abs() <= 14.0;
    let front = rounded_rect_distance(x, y, [152.0, 152.0, 248.0, 272.0], 32.0) <= 0.0;
    let left = segment_distance(x, y, [210.0, 236.0], [276.0, 336.0]);
    let right = segment_distance(x, y, [276.0, 336.0], [342.0, 236.0]);
    let v = left.min(right) <= 20.0;
    tile && (v || !(back || front))
}

/// Signed distance to a rounded rectangle given as `[x, y, width, height]`; negative inside.
fn rounded_rect_distance(x: f32, y: f32, rect: [f32; 4], radius: f32) -> f32 {
    let [left, top, width, height] = rect;
    let qx = (x - (left + width / 2.0)).abs() - (width / 2.0 - radius);
    let qy = (y - (top + height / 2.0)).abs() - (height / 2.0 - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn segment_distance(x: f32, y: f32, a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let t = (((x - a[0]) * dx + (y - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (x - a[0] - t * dx).hypot(y - a[1] - t * dy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showing_main_reuses_a_window_with_docked_browser_tabs() {
        let app = tauri::test::mock_app();
        let _main = tauri::WebviewWindowBuilder::new(
            &app,
            MAIN_WINDOW,
            tauri::WebviewUrl::App("index.html".into()),
        )
        .build()
        .unwrap();
        app.get_window(MAIN_WINDOW)
            .unwrap()
            .add_child(
                tauri::WebviewBuilder::new(
                    "booth-tab-1",
                    tauri::WebviewUrl::External("https://example.invalid/".parse().unwrap()),
                ),
                tauri::LogicalPosition::new(800.0, 100.0),
                tauri::LogicalSize::new(400.0, 600.0),
            )
            .unwrap();
        assert!(app.get_webview_window(MAIN_WINDOW).is_none());
        let window_count = app.windows().len();

        show_main_window(app.handle()).unwrap();

        assert_eq!(app.windows().len(), window_count);
        assert!(app.get_webview(MAIN_WINDOW).is_some());
        assert_eq!(
            app.get_webview("booth-tab-1").unwrap().window().label(),
            MAIN_WINDOW
        );
    }

    #[test]
    fn background_off_close_quits_the_process() {
        assert_eq!(decide_close(false), CloseDecision::Quit);
    }

    #[test]
    fn background_on_close_destroys_the_window_and_keeps_the_process() {
        assert_eq!(
            decide_close(true),
            if background_supported() {
                CloseDecision::DestroyWindowKeepProcess
            } else {
                CloseDecision::Quit
            }
        );
    }

    #[test]
    fn autostart_stays_off_unless_background_mode_is_on() {
        assert!(!autostart_should_enable(false, false));
        assert!(!autostart_should_enable(false, true));
        assert!(!autostart_should_enable(true, false));
        assert_eq!(autostart_should_enable(true, true), background_supported());
        assert_eq!(normalize_presence(false, true), (false, false));
        assert_eq!(
            normalize_presence(true, true),
            (background_supported(), background_supported())
        );
        assert_eq!(
            normalize_presence(true, false),
            (background_supported(), false)
        );
    }

    #[test]
    fn login_launch_without_background_mode_still_opens_a_window() {
        assert!(should_open_window_on_start(false, true));
        assert!(should_open_window_on_start(false, false));
        assert!(should_open_window_on_start(true, false));
        assert_eq!(
            should_open_window_on_start(true, true),
            !background_supported()
        );
        assert!(launched_by_login_item(["VRCLibMan", AUTOSTART_ARG]));
        assert!(!launched_by_login_item(["VRCLibMan"]));
        assert!(!launched_by_login_item(["VRCLibMan", "--other"]));
    }

    #[test]
    fn a_missing_recovery_path_opens_login_launch_and_does_not_keep_the_process() {
        // Startup and close handling must receive effective readiness, never
        // just the persisted preference to run in the background.
        for requested in [false, true] {
            let ready = can_background(requested, false);
            assert!(!ready);
            assert!(should_open_window_on_start(ready, true));
            assert_eq!(decide_close(ready), CloseDecision::Quit);
        }
        assert!(!can_background(false, true));
        let ready = can_background(true, true);
        assert!(ready);
        assert!(!should_open_window_on_start(ready, true));
        assert_eq!(decide_close(ready), CloseDecision::DestroyWindowKeepProcess);
    }
}
