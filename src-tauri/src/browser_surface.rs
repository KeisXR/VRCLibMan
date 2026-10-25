//! Native browser surface. Tab data is independent of its host window and layout rectangle.
//! A future docked panel can use the same child webviews and commands in a different host.
use crate::{
    browser,
    browser_tabs::{self, BrowserState, TabSession, TOOLBAR_LABEL},
};
use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Webview, WebviewBuilder, WebviewUrl,
    Window, WindowBuilder,
};
use url::Url;

const CHANGED: &str = "browser-state-changed";

#[derive(Debug, Clone, Copy)]
pub struct SurfaceArea {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}
impl SurfaceArea {
    pub fn split(self, toolbar_height: f64) -> (Self, Self) {
        let height = toolbar_height.max(0.0).min((self.height - 1.0).max(0.0));
        (
            Self { height, ..self },
            Self {
                top: self.top + height,
                height: (self.height - height).max(1.0),
                ..self
            },
        )
    }
    #[cfg(not(target_os = "linux"))]
    fn rect(self) -> tauri::Rect {
        tauri::Rect {
            position: LogicalPosition::new(self.left, self.top).into(),
            size: LogicalSize::new(self.width.max(1.0), self.height.max(1.0)).into(),
        }
    }
}

#[derive(Clone)]
struct BrowserHost {
    window_label: String,
    owns_window: bool,
    // None fills the host. A docked host can supply a panel rectangle in logical pixels.
    area: Option<SurfaceArea>,
}
struct Surface {
    session: TabSession,
    host: Option<BrowserHost>,
}
static SURFACE: Mutex<Surface> = Mutex::new(Surface {
    session: TabSession::new(),
    host: None,
});
fn surface() -> std::sync::MutexGuard<'static, Surface> {
    SURFACE.lock().unwrap_or_else(|e| e.into_inner())
}
fn changed(app: &AppHandle) {
    let _ = app.emit(CHANGED, ());
}
fn mutable() -> Result<(), String> {
    if browser::library_walk_running() || browser::download_job_armed() {
        return Err(crate::db::BROWSER_BUSY_MESSAGE.into());
    }
    Ok(())
}

pub fn host_window(app: &AppHandle) -> Option<Window> {
    let label = surface()
        .host
        .as_ref()
        .map(|host| host.window_label.clone())?;
    app.get_window(&label)
}
pub fn active_view(app: &AppHandle) -> Result<Webview, String> {
    let id = surface()
        .session
        .active_id
        .clone()
        .ok_or("ブラウザを開いてから操作してください")?;
    app.get_webview(&id)
        .ok_or_else(|| "選択中のタブが見つかりません".into())
}
pub fn booth_view(app: &AppHandle) -> Option<Webview> {
    let ids = {
        let state = surface();
        state
            .session
            .active_id
            .iter()
            .cloned()
            .chain(state.session.tabs.iter().rev().map(|tab| tab.id.clone()))
            .collect::<Vec<_>>()
    };
    ids.into_iter()
        .filter_map(|id| app.get_webview(&id))
        .find(|view| {
            view.url().ok().is_some_and(|url| {
                let host = url.host_str().unwrap_or("");
                url.scheme() == "https" && (host == "booth.pm" || host.ends_with(".booth.pm"))
            })
        })
}

fn checked_url(app: &AppHandle, input: &str) -> Result<Url, String> {
    let url = browser_tabs::navigation_url(input)?;
    if app.config().build.dev_url.as_ref().is_some_and(|local| {
        let loopback = |url: &Url| {
            url.host_str()
                .is_some_and(|host| host == "localhost" || host == "127.0.0.1" || host == "[::1]")
        };
        local.origin() == url.origin()
            || (loopback(local)
                && loopback(&url)
                && local.port_or_known_default() == url.port_or_known_default())
    }) {
        return Err("アプリ内部のURLはブラウザでは開けません".into());
    }
    Ok(url)
}

fn update_page(
    app: &AppHandle,
    id: &str,
    url: Option<&Url>,
    title: Option<String>,
    loading: Option<bool>,
) {
    let updated = {
        let mut state = surface();
        state.session.update(id, |tab| {
            if let Some(url) = url {
                tab.url = url.to_string();
            }
            if let Some(title) = title {
                tab.title = title.chars().take(100).collect();
            }
            if let Some(loading) = loading {
                tab.loading = loading;
            }
        })
    };
    if updated {
        changed(app);
    }
}

fn refresh_history(view: &Webview) {
    let app = view.app_handle().clone();
    let id = view.label().to_string();
    let _ = view.with_webview(move |native| {
        if let Ok((back, forward)) = native_history(&native, None) {
            let updated = {
                let mut state = surface();
                if let Some(tab) = state.session.tabs.iter_mut().find(|tab| tab.id == id) {
                    let different = tab.can_go_back != back || tab.can_go_forward != forward;
                    tab.can_go_back = back;
                    tab.can_go_forward = forward;
                    different
                } else {
                    false
                }
            };
            if updated {
                changed(&app);
            }
        }
    });
}

fn native_history(
    native: &tauri::webview::PlatformWebview,
    action: Option<bool>,
) -> Result<(bool, bool), String> {
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::WebViewExt;
        let view = native.inner();
        if let Some(forward) = action {
            if forward {
                view.go_forward();
            } else {
                view.go_back();
            }
        }
        Ok((view.can_go_back(), view.can_go_forward()))
    }
    #[cfg(windows)]
    unsafe {
        let view = native
            .controller()
            .CoreWebView2()
            .map_err(|e| format!("履歴を確認できません: {e}"))?;
        if let Some(forward) = action {
            if forward {
                view.GoForward()
            } else {
                view.GoBack()
            }
            .map_err(|e| format!("履歴を移動できません: {e}"))?;
        }
        let mut back = Default::default();
        let mut forward = Default::default();
        view.CanGoBack(&mut back).map_err(|e| e.to_string())?;
        view.CanGoForward(&mut forward).map_err(|e| e.to_string())?;
        Ok((back.as_bool(), forward.as_bool()))
    }
    #[cfg(target_os = "macos")]
    unsafe {
        let view = native.inner().cast::<objc2::runtime::AnyObject>();
        if let Some(forward) = action {
            let _: *mut objc2::runtime::AnyObject = if forward {
                objc2::msg_send![view, goForward]
            } else {
                objc2::msg_send![view, goBack]
            };
        }
        let back: objc2::runtime::Bool = objc2::msg_send![view, canGoBack];
        let forward: objc2::runtime::Bool = objc2::msg_send![view, canGoForward];
        Ok((back.as_bool(), forward.as_bool()))
    }
}

pub fn state(app: &AppHandle) -> Result<BrowserState, String> {
    if let Ok(view) = active_view(app) {
        if let Ok(url) = view.url() {
            let mut state = surface();
            state
                .session
                .update(view.label(), |tab| tab.url = url.to_string());
        }
        refresh_history(&view);
    }
    let (tabs, active_id) = {
        let state = surface();
        (state.session.tabs.clone(), state.session.active_id.clone())
    };
    let app_state = app.state::<crate::AppState>();
    let connection = app_state
        .db
        .lock()
        .map_err(|_| "データベースをロックできません".to_string())?;
    let bookmarks = crate::db::browser_bookmarks(&connection)?;
    Ok(BrowserState {
        open: host_window(app).is_some(),
        tabs,
        active_id,
        bookmarks,
        busy: browser::library_walk_running() || browser::download_job_armed(),
    })
}

#[cfg(not(target_os = "linux"))]
fn set_view_bounds(
    view: &Webview,
    area: SurfaceArea,
    _toolbar: bool,
    _fill_host: bool,
) -> Result<(), String> {
    view.set_bounds(area.rect()).map_err(|e| e.to_string())
}

#[cfg(target_os = "linux")]
fn set_view_bounds(
    view: &Webview,
    area: SurfaceArea,
    toolbar: bool,
    fill_host: bool,
) -> Result<(), String> {
    crate::browser_linux::set_view_bounds(view, area, toolbar, fill_host)
}

pub fn layout(app: &AppHandle) -> Result<(), String> {
    let (host, toolbar_height, tabs, active) = {
        let state = surface();
        (
            state.host.clone(),
            state.session.toolbar_height,
            state.session.tabs.clone(),
            state.session.active_id.clone(),
        )
    };
    let Some(host) = host else {
        return Ok(());
    };
    let Some(window) = app.get_window(&host.window_label) else {
        return Ok(());
    };
    let size = window
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical::<f64>(window.scale_factor().map_err(|e| e.to_string())?);
    let area = host.area.unwrap_or(SurfaceArea {
        left: 0.0,
        top: 0.0,
        width: size.width,
        height: size.height,
    });
    let (toolbar, content) = area.split(toolbar_height);
    if let Some(view) = app.get_webview(TOOLBAR_LABEL) {
        set_view_bounds(&view, toolbar, true, host.area.is_none())?;
    }
    for tab in tabs {
        if let Some(view) = app.get_webview(&tab.id) {
            set_view_bounds(&view, content, false, host.area.is_none())?;
            if active.as_deref() == Some(&tab.id) {
                view.show().map_err(|e| e.to_string())?;
            } else {
                view.hide().map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

pub fn toolbar_height(app: &AppHandle, height: f64) -> Result<(), String> {
    if !height.is_finite() {
        return Err("操作バーの高さが不正です".into());
    }
    surface().session.toolbar_height = height.clamp(browser_tabs::DEFAULT_TOOLBAR_HEIGHT, 420.0);
    layout(app)
}

/// The parent window and rectangle are the only placement concerns; tab behavior is unchanged.
fn mount_tab(app: &AppHandle, host: &Window, url: Url) -> Result<String, String> {
    let previous = surface().session.active_id.clone();
    let id = surface().session.add(&url)?;
    let navigation_app = app.clone();
    let navigation_id = id.clone();
    let popup_app = app.clone();
    let popup_source = id.clone();
    let mut builder = WebviewBuilder::new(&id, WebviewUrl::External(url))
        .on_navigation(move |url| {
            // Empty documents are also used by ordinary sites for frames.
            if url.as_str() == "about:blank" {
                return true;
            }
            if checked_url(&navigation_app, url.as_str()).is_err() {
                return false;
            }
            update_page(&navigation_app, &navigation_id, Some(url), None, Some(true));
            true
        })
        .on_document_title_changed(|view, title| {
            update_page(view.app_handle(), view.label(), None, Some(title), None)
        })
        .on_page_load(|view, payload| {
            let loading = matches!(payload.event(), tauri::webview::PageLoadEvent::Started);
            update_page(
                view.app_handle(),
                view.label(),
                Some(payload.url()),
                None,
                Some(loading),
            );
            browser::browser_log(
                view.app_handle(),
                &format!(
                    "{} {}",
                    if loading {
                        "読み始め"
                    } else {
                        "読み終わり"
                    },
                    payload.url().origin().ascii_serialization()
                ),
            );
            if !loading {
                refresh_history(&view);
                if !browser::library_walk_running()
                    && payload
                        .url()
                        .host_str()
                        .is_some_and(|host| host == "booth.pm" || host.ends_with(".booth.pm"))
                {
                    let _ = view.app_handle().emit("booth-browser-used", ());
                }
            }
        })
        .on_new_window(move |url, _| {
            // Queue creation after the current WebView callback, as required by WebView2.
            if checked_url(&popup_app, url.as_str()).is_ok() && mutable().is_ok() {
                let handle = popup_app.clone();
                let source = popup_source.clone();
                tauri::async_runtime::spawn(async move {
                    let input = url.to_string();
                    let login = browser_tabs::login_popup_in_same_tab(&url);
                    let _ = browser::dispatch_ui(handle, move |app| {
                        if login {
                            if let Some(view) = app.get_webview(&source) {
                                return view
                                    .navigate(checked_url(app, &input)?)
                                    .map_err(|e| e.to_string());
                            }
                        }
                        new_tab(app, Some(&input))
                    })
                    .await;
                });
            }
            tauri::webview::NewWindowResponse::Deny
        })
        .on_download(|_, event| browser::handle_booth_download(event));
    #[cfg(not(target_os = "macos"))]
    {
        let root = browser::local_data_dir(app)?;
        let dir = browser::session_dir(&root);
        if !browser::session_dir_is_safe(&root, &dir) {
            return Err("セッションの場所が不正です".into());
        }
        std::fs::create_dir_all(&dir)
            .map_err(|_| "セッション用のフォルダを作れません".to_string())?;
        builder = builder.data_directory(dir);
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.data_store_identifier(browser::BOOTH_DATA_STORE_ID);
    }
    let view = match host.add_child(
        builder,
        LogicalPosition::new(0.0, browser_tabs::DEFAULT_TOOLBAR_HEIGHT),
        LogicalSize::new(1100.0, 650.0),
    ) {
        Ok(view) => view,
        Err(error) => {
            let mut state = surface();
            let _ = state.session.remove(&id);
            if let Some(previous) = previous {
                let _ = state.session.select(&previous);
            }
            return Err(format!("タブを開けません: {error}"));
        }
    };
    #[cfg(target_os = "linux")]
    crate::browser_linux::remember_profile(&view)?;
    #[cfg(not(target_os = "linux"))]
    let _ = view;
    layout(app)?;
    changed(app);
    Ok(id)
}

fn install_browser_menu(app: &AppHandle, window: &Window) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let item =
        |id: &str, title: &str, key: &str| MenuItem::with_id(app, id, title, true, Some(key));
    let new_item =
        item("browser-new-tab", "新しいタブ", "CmdOrCtrl+T").map_err(|e| e.to_string())?;
    let close_item =
        item("browser-close-tab", "タブを閉じる", "CmdOrCtrl+W").map_err(|e| e.to_string())?;
    let address =
        item("browser-address", "アドレスバー", "CmdOrCtrl+L").map_err(|e| e.to_string())?;
    let reload_item =
        item("browser-reload", "再読み込み", "CmdOrCtrl+R").map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let keys = ("Cmd+[", "Cmd+]");
    #[cfg(not(target_os = "macos"))]
    let keys = ("Alt+Left", "Alt+Right");
    let back = item("browser-back", "戻る", keys.0).map_err(|e| e.to_string())?;
    let forward = item("browser-forward", "進む", keys.1).map_err(|e| e.to_string())?;
    let menu = Menu::new(app).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    menu.append(
        &Submenu::with_items(
            app,
            "VRCLibMan",
            true,
            &[
                &PredefinedMenuItem::about(app, None, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::services(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::hide(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::hide_others(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::quit(app, None).map_err(|e| e.to_string())?,
            ],
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    menu.append(
        &Submenu::with_items(
            app,
            "編集",
            true,
            &[
                &PredefinedMenuItem::undo(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::redo(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::cut(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::copy(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::paste(app, None).map_err(|e| e.to_string())?,
                &PredefinedMenuItem::select_all(app, None).map_err(|e| e.to_string())?,
            ],
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let controls = Submenu::with_items(
        app,
        "ブラウザ",
        true,
        &[
            &new_item,
            &close_item,
            &address,
            &reload_item,
            &back,
            &forward,
        ],
    )
    .map_err(|e| e.to_string())?;
    menu.append(&controls).map_err(|e| e.to_string())?;
    window.set_menu(menu).map_err(|e| e.to_string())?;
    // Keep native accelerators registered without an Alt-activated menu bar.
    #[cfg(not(target_os = "macos"))]
    window.hide_menu().map_err(|e| e.to_string())?;
    window.on_menu_event(|window, event| {
        let app = window.app_handle().clone();
        if !window.is_focused().unwrap_or(false)
            || host_window(&app).is_none_or(|host| host.label() != window.label())
        {
            return;
        }
        let action = event.id().as_ref().to_string();
        if !action.starts_with("browser-") {
            return;
        }
        dispatch_shortcut(&app, &action);
    });
    Ok(())
}

pub(crate) fn dispatch_shortcut(app: &AppHandle, action: &str) {
    let app = app.clone();
    let action = action.to_string();
    tauri::async_runtime::spawn(async move {
        let _ = browser::dispatch_ui(app.clone(), move |handle| {
            let result = match action.as_str() {
                "browser-new-tab" => new_tab(handle, None),
                "browser-close-tab" => {
                    active_view(handle).and_then(|view| close_tab(handle, view.label()))
                }
                "browser-reload" => reload(handle),
                "browser-back" => history(handle, false),
                "browser-forward" => history(handle, true),
                "browser-address" => Ok(()),
                _ => Ok(()),
            };
            if action == "browser-address" || action == "browser-new-tab" {
                if let Some(view) = handle.get_webview(TOOLBAR_LABEL) {
                    let _ = view.set_focus();
                }
                let _ = handle.emit_to(TOOLBAR_LABEL, "browser-focus-address", ());
            }
            if let Err(error) = result {
                let _ = handle.emit("browser-operation-error", error);
            }
            Ok(())
        })
        .await;
    });
}

pub fn open(app: &AppHandle) -> Result<(), String> {
    browser::browser_enabled(app)?;
    if let Some(window) = host_window(app) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    if let Some(probe) = app.get_webview_window("booth-probe") {
        let _ = probe.destroy();
    }
    let builder = WindowBuilder::new(app, browser::WINDOW_LABEL)
        .title("ブラウザ — VRCLibMan")
        .inner_size(1200.0, 850.0)
        .min_inner_size(640.0, 480.0);
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let window = builder
        .build()
        .map_err(|e| format!("ブラウザを開けません: {e}"))?;
    {
        let mut state = surface();
        state.session.clear();
        state.host = Some(BrowserHost {
            window_label: window.label().into(),
            owns_window: true,
            area: None,
        });
    }
    let result = (|| {
        window
            .add_child(
                WebviewBuilder::new(
                    TOOLBAR_LABEL,
                    WebviewUrl::App("index.html?browserToolbar=1".into()),
                ),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(1200.0, browser_tabs::DEFAULT_TOOLBAR_HEIGHT),
            )
            .map_err(|e| format!("操作バーを開けません: {e}"))?;
        mount_tab(app, &window, checked_url(app, browser::LIBRARY_URL)?)?;
        install_browser_menu(app, &window)?;
        // Child webviews do not install Tauri's Windows resize borders themselves.
        #[cfg(windows)]
        window.set_resizable(true).map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        let _ = window.destroy();
        surface().session.clear();
        surface().host = None;
        return Err(error);
    }
    let handle = app.clone();
    let host_label = window.label().to_string();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
            let _ = layout(&handle);
        }
        tauri::WindowEvent::Destroyed => {
            {
                let mut state = surface();
                if state
                    .host
                    .as_ref()
                    .is_some_and(|host| host.window_label == host_label)
                {
                    state.session.clear();
                    state.host = None;
                }
            }
            browser::request_library_walk_cancel();
            changed(&handle);
            let _ = handle.emit("booth-browser-used", ());
        }
        _ => {}
    });
    Ok(())
}

pub fn new_tab(app: &AppHandle, url: Option<&str>) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let target = checked_url(app, url.unwrap_or(browser::LIBRARY_URL))?;
    if host_window(app).is_none() {
        open(app)?;
        if target.as_str() != browser::LIBRARY_URL {
            active_view(app)?
                .navigate(target)
                .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    mount_tab(
        app,
        &host_window(app).ok_or("ブラウザが開いていません")?,
        target,
    )?;
    Ok(())
}
pub fn navigate(app: &AppHandle, input: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let url = checked_url(app, input)?;
    if host_window(app).is_none() {
        open(app)?;
    }
    active_view(app)?
        .navigate(url)
        .map_err(|e| format!("ページを開けません: {e}"))
}
pub fn select(app: &AppHandle, id: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    surface().session.select(id)?;
    layout(app)?;
    if let Ok(view) = active_view(app) {
        let _ = view.set_focus();
    }
    changed(app);
    Ok(())
}
pub fn close_tab(app: &AppHandle, id: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    if !surface().session.tabs.iter().any(|tab| tab.id == id) {
        return Err("タブが見つかりません".into());
    }
    if let Some(view) = app.get_webview(id) {
        view.close().map_err(|e| e.to_string())?;
    }
    surface().session.remove(id)?;
    let empty = surface().session.tabs.is_empty();
    if empty {
        close(app)?;
    } else {
        layout(app)?;
    }
    changed(app);
    Ok(())
}
pub fn close(app: &AppHandle) -> Result<(), String> {
    let (host, ids) = {
        let mut state = surface();
        let host = state.host.take();
        let ids = state
            .session
            .tabs
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        state.session.clear();
        (host, ids)
    };
    for id in ids {
        if let Some(view) = app.get_webview(&id) {
            let _ = view.close();
        }
    }
    if let Some(view) = app.get_webview(TOOLBAR_LABEL) {
        let _ = view.close();
    }
    if let Some(host) = host {
        if host.owns_window {
            if let Some(window) = app.get_window(&host.window_label) {
                let _ = window.destroy();
            }
        }
    }
    changed(app);
    Ok(())
}
pub fn history(app: &AppHandle, forward: bool) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let view = active_view(app)?;
    let handle = app.clone();
    view.with_webview(move |native| {
        if let Err(error) = native_history(&native, Some(forward)) {
            let _ = handle.emit("browser-operation-error", error);
        }
    })
    .map_err(|e| format!("履歴を移動できません: {e}"))
}
pub fn reload(app: &AppHandle) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    active_view(app)?
        .reload()
        .map_err(|e| format!("ページを読み直せません: {e}"))
}
pub fn stop(app: &AppHandle) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let view = active_view(app)?;
    view.eval("window.stop() ").map_err(|e| e.to_string())?;
    update_page(app, view.label(), None, None, Some(false));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_uses_host_relative_coordinates_for_a_future_docked_panel() {
        let area = SurfaceArea {
            left: 500.0,
            top: 20.0,
            width: 700.0,
            height: 600.0,
        };
        let (bar, content) = area.split(136.0);
        assert_eq!(
            (bar.left, bar.top, bar.width, bar.height),
            (500.0, 20.0, 700.0, 136.0)
        );
        assert_eq!(
            (content.left, content.top, content.width, content.height),
            (500.0, 156.0, 700.0, 464.0)
        );
        let (_, tiny) = SurfaceArea {
            height: 50.0,
            ..area
        }
        .split(136.0);
        assert_eq!(tiny.height, 1.0);
    }
}
