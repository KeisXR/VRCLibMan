use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager, Runtime};

pub(crate) const AUTOSTART_ARG: &str = "--vrclibman-autostart";
pub(crate) const MAIN_WINDOW: &str = "main";
#[cfg(any(windows, target_os = "macos"))]
const TRAY_ID: &str = "vrclibman";

/// What closing the main window should do.
/// `Quit` ends the process. `DestroyWindowKeepProcess` drops the webview and leaves the server up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseDecision {
    Quit,
    DestroyWindowKeepProcess,
}

pub(crate) fn decide_close(background_mode: bool) -> CloseDecision {
    if background_mode {
        CloseDecision::DestroyWindowKeepProcess
    } else {
        CloseDecision::Quit
    }
}

pub(crate) fn normalize_presence(background_mode: bool, launch_at_login: bool) -> (bool, bool) {
    if background_mode {
        (true, launch_at_login)
    } else {
        (false, false)
    }
}

pub(crate) fn autostart_should_enable(background_mode: bool, launch_at_login: bool) -> bool {
    background_mode && launch_at_login
}

pub(crate) fn launched_by_login_item<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> bool {
    args.into_iter()
        .any(|arg| arg.as_ref() == AUTOSTART_ARG)
}

/// A windowless start is only for a login launch while background mode is on.
/// Background mode off always opens a window, including a leftover login launch.
pub(crate) fn should_open_window_on_start(background_mode: bool, launched_by_login: bool) -> bool {
    !(background_mode && launched_by_login)
}

pub(crate) fn remember_background<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        state.background_mode.store(enabled, Ordering::SeqCst);
    }
}

pub(crate) fn background_mode_enabled<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<crate::AppState>()
        .map(|state| state.background_mode.load(Ordering::SeqCst))
        .unwrap_or(false)
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
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
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
    let login = sync_login_item(
        app,
        autostart_should_enable(background_mode, launch_at_login),
    );
    let tray = if background_mode {
        ensure_tray(app)
    } else {
        remove_tray(app);
        Ok(())
    };
    login?;
    tray
}

pub(crate) fn sync_login_item<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> Result<(), String> {
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

#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn ensure_tray<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};

    if app.tray_by_id(TRAY_ID).is_some() {
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
    #[cfg(windows)]
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
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn ensure_tray<R: Runtime>(_app: &AppHandle<R>) -> Result<(), String> {
    Ok(())
}

#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn remove_tray<R: Runtime>(app: &AppHandle<R>) {
    drop(app.remove_tray_by_id(TRAY_ID));
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn remove_tray<R: Runtime>(_app: &AppHandle<R>) {}

#[cfg(any(windows, target_os = "macos"))]
fn menu_bar_icon() -> tauri::image::Image<'static> {
    const N: u32 = 22;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let border = x == 1 || x == 2 || y == 1 || y == 2 || x + 2 >= N || y + 2 >= N;
            let mark = x > 7 && x < N - 8 && y > 7 && y < N - 8;
            if border || mark {
                let index = ((y * N + x) * 4) as usize;
                rgba[index + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, N, N)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_off_close_quits_the_process() {
        assert_eq!(decide_close(false), CloseDecision::Quit);
    }

    #[test]
    fn background_on_close_destroys_the_window_and_keeps_the_process() {
        assert_eq!(
            decide_close(true),
            CloseDecision::DestroyWindowKeepProcess
        );
    }

    #[test]
    fn autostart_stays_off_unless_background_mode_is_on() {
        assert!(!autostart_should_enable(false, false));
        assert!(!autostart_should_enable(false, true));
        assert!(!autostart_should_enable(true, false));
        assert!(autostart_should_enable(true, true));
        assert_eq!(normalize_presence(false, true), (false, false));
        assert_eq!(normalize_presence(true, true), (true, true));
        assert_eq!(normalize_presence(true, false), (true, false));
    }

    #[test]
    fn login_launch_without_background_mode_still_opens_a_window() {
        assert!(should_open_window_on_start(false, true));
        assert!(should_open_window_on_start(false, false));
        assert!(should_open_window_on_start(true, false));
        assert!(!should_open_window_on_start(true, true));
        assert!(launched_by_login_item(["VRCLibMan", AUTOSTART_ARG]));
        assert!(!launched_by_login_item(["VRCLibMan"]));
        assert!(!launched_by_login_item(["VRCLibMan", "--other"]));
    }
}
