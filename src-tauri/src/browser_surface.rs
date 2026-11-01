//! Native browser surface. Each page tab belongs to one host: the right panel or its own window.
//! Several hosts can be open at once. Moving a tab reparents that webview, so history and sign-in stay.
use crate::{
    browser,
    browser_tabs::{self, BrowserState, HostKind, MoveToken, TabSession, PANEL_HOST_ID},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Webview, WebviewBuilder, WebviewUrl,
    Window, WindowBuilder,
};
use url::Url;

const CHANGED: &str = "browser-state-changed";
/// Asks the main window to show the page tab in its right panel.
pub const PANEL_REQUESTED: &str = "browser-panel-requested";
/// The docked host is the main window; it never owns or destroys it.
pub const PANEL_WINDOW_LABEL: &str = crate::presence::MAIN_WINDOW;
pub const PANEL_UNSUPPORTED: &str =
    "この環境ではブラウザを右パネルに表示できません。別ウィンドウで開きます";
/// Largest panel rectangle accepted from the main window, in logical pixels.
const MAX_PANEL_EDGE: f64 = 16384.0;

/// Docking is implemented and checked against WebView2 first. macOS and Linux keep the
/// separate window until the docked layout is confirmed on real machines.
pub const fn panel_supported() -> bool {
    cfg!(windows)
}

/// The user moved a page to its own window in this session, so a later empty open uses a window.
/// Kept in memory only.
static PREFER_WINDOW: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
pub struct SurfaceArea {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}
impl SurfaceArea {
    /// A rectangle measured by the main window's right panel. Rejects non-finite values and
    /// keeps the rectangle inside a sane range.
    pub fn from_panel(left: f64, top: f64, width: f64, height: f64) -> Result<Self, String> {
        if ![left, top, width, height].iter().all(|value| value.is_finite()) {
            return Err("ブラウザの表示位置が不正です".into());
        }
        Ok(Self {
            left: left.clamp(0.0, MAX_PANEL_EDGE),
            top: top.clamp(0.0, MAX_PANEL_EDGE),
            width: width.clamp(0.0, MAX_PANEL_EDGE),
            height: height.clamp(0.0, MAX_PANEL_EDGE),
        })
    }
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

/// Where the active tab goes in a docked host. None hides every tab of that host.
fn panel_content(area: Option<SurfaceArea>, visible: bool) -> Option<SurfaceArea> {
    let area = area?;
    (visible && area.width >= 2.0 && area.height >= 2.0).then_some(area)
}

/// Which host a new browser session starts in.
fn start_in_panel(supported: bool, prefer_window: bool, main_visible: bool) -> bool {
    supported && !prefer_window && main_visible
}

#[derive(Clone)]
struct LiveHost {
    id: String,
    kind: HostKind,
    window_label: String,
    toolbar_label: Option<String>,
    generation: u64,
    area: Option<SurfaceArea>,
    visible: bool,
    toolbar_height: f64,
}

struct Surface {
    session: TabSession,
    live: Vec<LiveHost>,
    next_generation: u64,
}
static SURFACE: Mutex<Surface> = Mutex::new(Surface {
    session: TabSession::new(),
    live: Vec::new(),
    next_generation: 1,
});
fn surface() -> std::sync::MutexGuard<'static, Surface> {
    SURFACE.lock().unwrap_or_else(|error| error.into_inner())
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

fn live_label(state: &Surface, host_id: &str) -> Option<String> {
    state
        .live
        .iter()
        .find(|host| host.id == host_id)
        .map(|host| host.window_label.clone())
        .or_else(|| (host_id == PANEL_HOST_ID).then(|| PANEL_WINDOW_LABEL.to_string()))
}

/// The window of the focused host, or of the panel when that is where the pages are.
pub fn host_window(app: &AppHandle) -> Option<Window> {
    let label = {
        let state = surface();
        let host_id = state
            .session
            .focused_host()
            .filter(|id| state.session.active_of(id).is_some())
            .or_else(|| state.session.primary_host().map(str::to_string))?;
        live_label(&state, &host_id)?
    };
    app.get_window(&label)
}

pub fn active_view(app: &AppHandle) -> Result<Webview, String> {
    let id = surface()
        .session
        .focused_tab()
        .ok_or("ブラウザを開いてから操作してください")?;
    app.get_webview(&id)
        .ok_or_else(|| "選択中のタブが見つかりません".into())
}

pub fn view_for(app: &AppHandle, tab_id: &str) -> Result<Webview, String> {
    if !surface().session.has(tab_id) {
        return Err("タブが見つかりません".into());
    }
    app.get_webview(tab_id)
        .ok_or_else(|| "選択中のタブが見つかりません".into())
}

/// Host implied by the webview that sent the command. The main window means the right panel.
pub fn host_id_for_label(label: &str) -> Option<String> {
    let state = surface();
    if let Some(host) = state
        .live
        .iter()
        .find(|host| host.toolbar_label.as_deref() == Some(label))
    {
        return Some(host.id.clone());
    }
    if label == PANEL_WINDOW_LABEL {
        return Some(PANEL_HOST_ID.to_string());
    }
    None
}

pub fn tab_for_caller(
    caller: &str,
    explicit_tab: Option<String>,
    explicit_host: Option<String>,
) -> Result<String, String> {
    if let Some(id) = explicit_tab {
        if surface().session.has(&id) {
            return Ok(id);
        }
        return Err("タブが見つかりません".into());
    }
    let host = explicit_host.or_else(|| host_id_for_label(caller));
    let state = surface();
    if let Some(host) = host.as_deref() {
        if let Some(id) = state.session.active_of(host) {
            return Ok(id);
        }
    }
    state
        .session
        .focused_tab()
        .ok_or_else(|| "ブラウザを開いてから操作してください".into())
}

pub fn booth_view(app: &AppHandle) -> Option<Webview> {
    let ids = {
        let state = surface();
        let mut ids = Vec::new();
        if let Some(id) = state.session.focused_tab() {
            ids.push(id);
        }
        for tab in state.session.tabs.iter().rev() {
            if !ids.iter().any(|id| id == &tab.id) {
                ids.push(tab.id.clone());
            }
        }
        ids
    };
    ids.into_iter()
        .filter_map(|id| app.get_webview(&id))
        .find(|view| {
            crate::browser_url::current_url(view).ok().is_some_and(|url| {
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
            .map_err(|error| format!("履歴を確認できません: {error}"))?;
        if let Some(forward) = action {
            if forward {
                view.GoForward()
            } else {
                view.GoBack()
            }
            .map_err(|error| format!("履歴を移動できません: {error}"))?;
        }
        let mut back = Default::default();
        let mut forward = Default::default();
        view.CanGoBack(&mut back).map_err(|error| error.to_string())?;
        view.CanGoForward(&mut forward)
            .map_err(|error| error.to_string())?;
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
    let ids = surface()
        .session
        .tabs
        .iter()
        .map(|tab| tab.id.clone())
        .collect::<Vec<_>>();
    for id in ids {
        if let Some(view) = app.get_webview(&id) {
            if let Ok(url) = crate::browser_url::current_url(&view) {
                let mut state = surface();
                state
                    .session
                    .update(view.label(), |tab| tab.url = url.to_string());
                drop(state);
                refresh_history(&view);
            }
        }
    }
    let (tabs, active_id, hosts, coarse) = {
        let state = surface();
        (
            state.session.tabs.clone(),
            state.session.focused_tab(),
            state.session.host_states(),
            state.session.coarse_host(),
        )
    };
    let app_state = app.state::<crate::AppState>();
    let connection = app_state
        .db
        .lock()
        .map_err(|_| "データベースをロックできません".to_string())?;
    let bookmarks = crate::db::browser_bookmarks(&connection)?;
    Ok(BrowserState {
        open: !tabs.is_empty(),
        tabs,
        active_id,
        hosts,
        bookmarks,
        busy: browser::library_walk_running() || browser::download_job_armed(),
        host: coarse,
        panel_supported: panel_supported(),
    })
}

#[cfg(not(target_os = "linux"))]
fn set_view_bounds(
    view: &Webview,
    area: SurfaceArea,
    _toolbar: bool,
    _fill_host: bool,
) -> Result<(), String> {
    view.set_bounds(area.rect()).map_err(|error| error.to_string())
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
    let (live, tabs) = {
        let state = surface();
        (state.live.clone(), state.session.tabs.clone())
    };
    for host in live {
        let mine = tabs
            .iter()
            .filter(|tab| tab.host_id == host.id)
            .cloned()
            .collect::<Vec<_>>();
        let active = surface().session.active_of(&host.id);
        match host.kind {
            HostKind::Window => layout_window(app, &host, &mine, active.as_deref())?,
            HostKind::Panel => layout_panel(app, &host, &mine, active.as_deref())?,
        }
    }
    Ok(())
}

fn layout_window(
    app: &AppHandle,
    host: &LiveHost,
    tabs: &[browser_tabs::BrowserTab],
    active: Option<&str>,
) -> Result<(), String> {
    let Some(window) = app.get_window(&host.window_label) else {
        return Ok(());
    };
    let size = window
        .inner_size()
        .map_err(|error| error.to_string())?
        .to_logical::<f64>(window.scale_factor().map_err(|error| error.to_string())?);
    let area = SurfaceArea {
        left: 0.0,
        top: 0.0,
        width: size.width,
        height: size.height,
    };
    let (toolbar, content) = area.split(host.toolbar_height);
    if let Some(label) = &host.toolbar_label {
        if let Some(view) = app.get_webview(label) {
            set_view_bounds(&view, toolbar, true, true)?;
        }
    }
    place_tabs(app, tabs, Some(content), true, active)
}

fn layout_panel(
    app: &AppHandle,
    host: &LiveHost,
    tabs: &[browser_tabs::BrowserTab],
    active: Option<&str>,
) -> Result<(), String> {
    place_tabs(
        app,
        tabs,
        panel_content(host.area, host.visible),
        false,
        active,
    )
}

fn place_tabs(
    app: &AppHandle,
    tabs: &[browser_tabs::BrowserTab],
    content: Option<SurfaceArea>,
    fill_host: bool,
    active: Option<&str>,
) -> Result<(), String> {
    for tab in tabs {
        let Some(view) = app.get_webview(&tab.id) else {
            continue;
        };
        match content {
            Some(content) => {
                set_view_bounds(&view, content, false, fill_host)?;
                if active == Some(tab.id.as_str()) {
                    view.show().map_err(|error| error.to_string())?;
                } else {
                    view.hide().map_err(|error| error.to_string())?;
                }
            }
            None => view.hide().map_err(|error| error.to_string())?,
        }
    }
    Ok(())
}

pub fn toolbar_height(app: &AppHandle, height: f64, host_id: Option<String>) -> Result<(), String> {
    if !height.is_finite() {
        return Err("操作バーの高さが不正です".into());
    }
    let height = height.clamp(browser_tabs::DEFAULT_TOOLBAR_HEIGHT, 420.0);
    let host_id = host_id.or_else(|| {
        surface()
            .session
            .focused_host()
            .filter(|id| surface().session.kind_of(id) == Some(HostKind::Window))
    });
    if let Some(host_id) = host_id {
        if let Some(host) = surface().live.iter_mut().find(|host| host.id == host_id) {
            host.toolbar_height = height;
        }
    }
    layout(app)
}

fn register_live(host: LiveHost) -> u64 {
    let mut state = surface();
    let generation = state.next_generation;
    state.next_generation = state.next_generation.wrapping_add(1);
    state.live.push(LiveHost { generation, ..host });
    generation
}

fn ensure_panel_live(area: Option<SurfaceArea>, visible: bool) -> u64 {
    {
        let mut state = surface();
        state.session.ensure_panel();
        if let Some(host) = state.live.iter_mut().find(|host| host.id == PANEL_HOST_ID) {
            if let Some(area) = area {
                host.area = Some(area);
                host.visible = visible;
            }
            return host.generation;
        }
    }
    register_live(LiveHost {
        id: PANEL_HOST_ID.into(),
        kind: HostKind::Panel,
        window_label: PANEL_WINDOW_LABEL.into(),
        toolbar_label: None,
        generation: 0,
        area,
        visible: visible && area.is_some(),
        toolbar_height: browser_tabs::DEFAULT_TOOLBAR_HEIGHT,
    })
}

fn forget_live(host_id: &str) -> Option<LiveHost> {
    let mut state = surface();
    state
        .live
        .iter()
        .position(|host| host.id == host_id)
        .map(|index| state.live.remove(index))
}

/// Drops an empty host. A window is destroyed only after it is no longer registered,
/// so a late Destroyed event cannot remove tabs that already moved.
fn retire_if_empty(app: &AppHandle, host_id: &str) {
    let retired = {
        let mut state = surface();
        if !state.session.drop_host_if_empty(host_id) {
            return;
        }
        state
            .live
            .iter()
            .position(|host| host.id == host_id)
            .map(|index| state.live.remove(index))
    };
    if let Some(host) = retired {
        if host.kind == HostKind::Window {
            if let Some(source) = app.get_window(&host.window_label) {
                let _ = source.destroy();
            }
        }
    }
}

fn close_webviews(app: &AppHandle, ids: &[String]) {
    for id in ids {
        if let Some(view) = app.get_webview(id) {
            let _ = view.close();
        }
    }
}

fn mount_tab(app: &AppHandle, host_id: &str, window: &Window, url: Url) -> Result<String, String> {
    let id = surface().session.add(host_id, &url)?;
    let navigation_app = app.clone();
    let navigation_id = id.clone();
    let popup_app = app.clone();
    let popup_source = id.clone();
    let mut builder = WebviewBuilder::new(&id, WebviewUrl::External(url.clone()))
        .on_navigation(move |url| {
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
                    if loading { "読み始め" } else { "読み終わり" },
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
                                    .map_err(|error| error.to_string());
                            }
                        }
                        let host = surface().session.host_of(&source);
                        new_tab_on(app, Some(&input), host)
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
            let _ = surface().session.remove(&id);
            return Err("セッションの場所が不正です".into());
        }
        if let Err(error) = std::fs::create_dir_all(&dir) {
            let _ = surface().session.remove(&id);
            return Err(format!("セッション用のフォルダを作れません: {error}"));
        }
        builder = builder.data_directory(dir);
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder.data_store_identifier(browser::BOOTH_DATA_STORE_ID);
    }
    let view = match window.add_child(
        builder,
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(1100.0, 650.0),
    ) {
        Ok(view) => view,
        Err(error) => {
            let _ = surface().session.remove(&id);
            return Err(format!("タブを開けません: {error}"));
        }
    };
    #[cfg(target_os = "linux")]
    if let Err(error) = crate::browser_linux::remember_profile(&view) {
        let _ = view.close();
        let _ = surface().session.remove(&id);
        return Err(error);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = view;
    layout(app)?;
    changed(app);
    Ok(id)
}

fn shortcut_name(id: &str) -> Option<&str> {
    const NAMES: &[&str] = &[
        "browser-new-tab",
        "browser-close-tab",
        "browser-address",
        "browser-reload",
        "browser-back",
        "browser-forward",
    ];
    NAMES.iter().copied().find(|name| {
        id == *name
            || id
                .strip_prefix(name)
                .is_some_and(|rest| rest.starts_with('-'))
    })
}

fn install_browser_menu(app: &AppHandle, window: &Window, host_id: &str, slot: u64) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let item = |name: &str, title: &str, key: &str| {
        MenuItem::with_id(app, format!("{name}-{slot}"), title, true, Some(key))
    };
    let new_item = item("browser-new-tab", "新しいタブ", "CmdOrCtrl+T").map_err(|error| error.to_string())?;
    let close_item =
        item("browser-close-tab", "タブを閉じる", "CmdOrCtrl+W").map_err(|error| error.to_string())?;
    let address = item("browser-address", "アドレスバー", "CmdOrCtrl+L").map_err(|error| error.to_string())?;
    let reload_item =
        item("browser-reload", "再読み込み", "CmdOrCtrl+R").map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    let keys = ("Cmd+[", "Cmd+]");
    #[cfg(not(target_os = "macos"))]
    let keys = ("Alt+Left", "Alt+Right");
    let back = item("browser-back", "戻る", keys.0).map_err(|error| error.to_string())?;
    let forward = item("browser-forward", "進む", keys.1).map_err(|error| error.to_string())?;
    let menu = Menu::new(app).map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    menu.append(
        &Submenu::with_items(
            app,
            "VRCLibMan",
            true,
            &[
                &PredefinedMenuItem::about(app, None, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::services(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::hide(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::hide_others(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::quit(app, None).map_err(|error| error.to_string())?,
            ],
        )
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    menu.append(
        &Submenu::with_items(
            app,
            "編集",
            true,
            &[
                &PredefinedMenuItem::undo(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::redo(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::cut(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::copy(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::paste(app, None).map_err(|error| error.to_string())?,
                &PredefinedMenuItem::select_all(app, None).map_err(|error| error.to_string())?,
            ],
        )
        .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let controls = Submenu::with_items(
        app,
        "ブラウザ",
        true,
        &[&new_item, &close_item, &address, &reload_item, &back, &forward],
    )
    .map_err(|error| error.to_string())?;
    menu.append(&controls).map_err(|error| error.to_string())?;
    window.set_menu(menu).map_err(|error| error.to_string())?;
    #[cfg(not(target_os = "macos"))]
    window.hide_menu().map_err(|error| error.to_string())?;
    let host_id = host_id.to_string();
    let app_handle = app.clone();
    window.on_menu_event(move |window, event| {
        if !window.is_focused().unwrap_or(false) {
            return;
        }
        let Some(action) = shortcut_name(event.id().as_ref()) else {
            return;
        };
        dispatch_shortcut_on(&app_handle, &host_id, action);
    });
    Ok(())
}

pub(crate) fn dispatch_shortcut(app: &AppHandle, action: &str) {
    let host = surface()
        .session
        .focused_host()
        .unwrap_or_else(|| PANEL_HOST_ID.to_string());
    dispatch_shortcut_on(app, &host, action);
}

fn dispatch_shortcut_on(app: &AppHandle, host_id: &str, action: &str) {
    let app = app.clone();
    let action = action.to_string();
    let host_id = host_id.to_string();
    tauri::async_runtime::spawn(async move {
        let _ = browser::dispatch_ui(app, move |handle| {
            surface().session.focus_host(&host_id);
            let result = match action.as_str() {
                "browser-new-tab" => new_tab_on(handle, None, Some(host_id.clone())),
                "browser-close-tab" => surface()
                    .session
                    .active_of(&host_id)
                    .ok_or_else(|| "タブが見つかりません".to_string())
                    .and_then(|id| close_tab(handle, &id)),
                "browser-reload" => reload_host(handle, &host_id),
                "browser-back" => history_host(handle, &host_id, false),
                "browser-forward" => history_host(handle, &host_id, true),
                "browser-address" => Ok(()),
                _ => Ok(()),
            };
            if action == "browser-address" || action == "browser-new-tab" {
                if let Some(label) = surface()
                    .live
                    .iter()
                    .find(|host| host.id == host_id)
                    .and_then(|host| host.toolbar_label.clone())
                {
                    if let Some(view) = handle.get_webview(&label) {
                        let _ = view.set_focus();
                    }
                    let _ = handle.emit_to(&label, "browser-focus-address", ());
                }
            }
            if let Err(error) = result {
                let _ = handle.emit("browser-operation-error", error);
            }
            Ok(())
        })
        .await;
    });
}

fn raise(window: &Window) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

fn request_panel(app: &AppHandle, tab_id: &str) {
    let _ = app.emit_to(PANEL_WINDOW_LABEL, PANEL_REQUESTED, tab_id.to_string());
}

fn raise_focused(app: &AppHandle) {
    let (label, panel_tab) = {
        let state = surface();
        let host_id = state
            .session
            .focused_host()
            .filter(|id| state.session.active_of(id).is_some())
            .or_else(|| state.session.primary_host().map(str::to_string));
        let Some(host_id) = host_id else {
            return;
        };
        let label = live_label(&state, &host_id);
        let panel_tab = (host_id == PANEL_HOST_ID)
            .then(|| state.session.active_of(PANEL_HOST_ID))
            .flatten();
        (label, panel_tab)
    };
    if let Some(label) = label {
        if let Some(window) = app.get_window(&label) {
            raise(&window);
        }
    }
    if let Some(id) = panel_tab {
        request_panel(app, &id);
    }
}

fn build_browser_window(
    app: &AppHandle,
    slot: u64,
    position: Option<(f64, f64)>,
) -> Result<(Window, String), String> {
    if let Some(probe) = app.get_webview_window("booth-probe") {
        let _ = probe.destroy();
    }
    let label = browser_tabs::window_label_for(slot);
    let toolbar = browser_tabs::toolbar_label_for(slot);
    let mut builder = WindowBuilder::new(app, &label)
        .title("ブラウザ — VRCLibMan")
        .inner_size(1200.0, 850.0)
        .min_inner_size(640.0, 480.0);
    if let Some((x, y)) = position {
        builder = builder.position(x, y);
    }
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let window = builder
        .build()
        .map_err(|error| format!("ブラウザを開けません: {error}"))?;
    let toolbar_url = format!("index.html?browserToolbar=1&browserHost=window-{slot}");
    let result = (|| {
        window
            .add_child(
                WebviewBuilder::new(&toolbar, WebviewUrl::App(toolbar_url.into())),
                LogicalPosition::new(0.0, 0.0),
                LogicalSize::new(1200.0, browser_tabs::DEFAULT_TOOLBAR_HEIGHT),
            )
            .map_err(|error| format!("操作バーを開けません: {error}"))?;
        #[cfg(windows)]
        window.set_resizable(true).map_err(|error| error.to_string())?;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        let _ = window.destroy();
        return Err(error);
    }
    Ok((window, toolbar))
}

fn watch_browser_window(app: &AppHandle, window: &Window, host_id: &str, generation: u64) {
    let handle = app.clone();
    let host_id = host_id.to_string();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. } => {
            let _ = layout(&handle);
        }
        tauri::WindowEvent::Focused(true) => {
            surface().session.focus_host(&host_id);
            changed(&handle);
        }
        tauri::WindowEvent::Destroyed => {
            let ids = {
                let mut state = surface();
                let current = state
                    .live
                    .iter()
                    .find(|host| host.id == host_id)
                    .map(|host| host.generation);
                if !browser_tabs::destroy_matches(current, generation) {
                    return;
                }
                state.live.retain(|host| host.id != host_id);
                state.session.take_host(&host_id)
            };
            close_webviews(&handle, &ids);
            if ids.iter().any(|id| browser::walk_uses_tab(id)) {
                browser::request_library_walk_cancel();
            }
            changed(&handle);
            let _ = handle.emit("booth-browser-used", ());
        }
        _ => {}
    });
}

/// The main window went away. Only the pages docked in it go with it.
pub fn handle_window_event(window: &Window, event: &tauri::WindowEvent) {
    if window.label() != PANEL_WINDOW_LABEL {
        return;
    }
    match event {
        tauri::WindowEvent::Destroyed => {
            let ids = {
                let mut state = surface();
                state.live.retain(|host| host.id != PANEL_HOST_ID);
                state.session.take_host(PANEL_HOST_ID)
            };
            if ids.iter().any(|id| browser::walk_uses_tab(id)) {
                browser::request_library_walk_cancel();
            }
            if !ids.is_empty() {
                changed(window.app_handle());
            }
        }
        tauri::WindowEvent::Focused(true) => {
            let mut state = surface();
            if state
                .session
                .kind_of(PANEL_HOST_ID)
                .is_some_and(|_| state.session.active_of(PANEL_HOST_ID).is_some())
            {
                state.session.focus_host(PANEL_HOST_ID);
            }
        }
        _ => {}
    }
}

fn discard_host(app: &AppHandle, host_id: &str, window: Option<&Window>) {
    let ids = surface().session.take_host(host_id);
    close_webviews(app, &ids);
    if let Some(host) = forget_live(host_id) {
        if let Some(label) = host.toolbar_label {
            if let Some(view) = app.get_webview(&label) {
                let _ = view.close();
            }
        }
    }
    if let Some(window) = window {
        let _ = window.destroy();
    }
}

fn open_window_at(app: &AppHandle, position: Option<(f64, f64)>) -> Result<String, String> {
    let host_id = surface().session.add_window();
    let slot = surface()
        .session
        .slot_of(&host_id)
        .ok_or("表示先が見つかりません")?;
    let (window, toolbar) = match build_browser_window(app, slot, position) {
        Ok(built) => built,
        Err(error) => {
            surface().session.take_host(&host_id);
            return Err(error);
        }
    };
    let generation = register_live(LiveHost {
        id: host_id.clone(),
        kind: HostKind::Window,
        window_label: window.label().to_string(),
        toolbar_label: Some(toolbar),
        generation: 0,
        area: None,
        visible: true,
        toolbar_height: browser_tabs::DEFAULT_TOOLBAR_HEIGHT,
    });
    let result = (|| {
        mount_tab(
            app,
            &host_id,
            &window,
            checked_url(app, browser::LIBRARY_URL)?,
        )?;
        install_browser_menu(app, &window, &host_id, slot)?;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        discard_host(app, &host_id, Some(&window));
        return Err(error);
    }
    watch_browser_window(app, &window, &host_id, generation);
    Ok(host_id)
}

fn open_window(app: &AppHandle) -> Result<(), String> {
    open_window_at(app, None).map(|_| ())
}

fn open_panel(app: &AppHandle, area: Option<SurfaceArea>) -> Result<(), String> {
    let main = app
        .get_window(PANEL_WINDOW_LABEL)
        .ok_or("メインウィンドウが見つかりません")?;
    if let Some(probe) = app.get_webview_window("booth-probe") {
        let _ = probe.destroy();
    }
    ensure_panel_live(area, area.is_some());
    if let Err(error) = checked_url(app, browser::LIBRARY_URL).and_then(|url| mount_tab(app, PANEL_HOST_ID, &main, url))
    {
        let ids = surface().session.take_host(PANEL_HOST_ID);
        forget_live(PANEL_HOST_ID);
        close_webviews(app, &ids);
        changed(app);
        return Err(error);
    }
    if let Some(id) = surface().session.active_of(PANEL_HOST_ID) {
        request_panel(app, &id);
    }
    Ok(())
}

pub fn open(app: &AppHandle) -> Result<(), String> {
    browser::browser_enabled(app)?;
    if surface()
        .session
        .tabs
        .iter()
        .any(|tab| app.get_webview(&tab.id).is_some())
    {
        raise_focused(app);
        return Ok(());
    }
    if !surface().session.tabs.is_empty() {
        close(app)?;
    }
    let main_visible = app
        .get_window(PANEL_WINDOW_LABEL)
        .and_then(|main| main.is_visible().ok())
        .unwrap_or(false);
    if start_in_panel(
        panel_supported(),
        PREFER_WINDOW.load(Ordering::Relaxed),
        main_visible,
    ) {
        return open_panel(app, None);
    }
    open_window(app)
}

fn reparent_tab(app: &AppHandle, tab_id: &str, target: &Window) -> Result<(), String> {
    let view = app
        .get_webview(tab_id)
        .ok_or("タブが見つかりません")?;
    view.reparent(target)
        .map_err(|error| format!("ブラウザを移せません: {error}"))
}

fn restore_moves(tokens: Vec<MoveToken>) {
    for token in tokens.into_iter().rev() {
        let _ = surface().session.restore(token);
    }
}

fn move_ids(
    tab_ids: &[String],
    to_host: &str,
) -> Result<Vec<MoveToken>, String> {
    let mut tokens = Vec::new();
    for id in tab_ids {
        match surface().session.move_tab(id, to_host, usize::MAX) {
            Ok(token) => tokens.push(token),
            Err(error) => {
                restore_moves(tokens);
                return Err(error);
            }
        }
    }
    Ok(tokens)
}

fn reparent_ids(
    app: &AppHandle,
    tab_ids: &[String],
    target: &Window,
    source: Option<&Window>,
    tokens: Vec<MoveToken>,
) -> Result<(), String> {
    let mut moved: Vec<String> = Vec::new();
    for id in tab_ids {
        if let Err(error) = reparent_tab(app, id, target) {
            if let Some(source) = source {
                for id in moved.iter().rev() {
                    let _ = reparent_tab(app, id, source);
                }
            }
            restore_moves(tokens);
            return Err(error);
        }
        moved.push(id.clone());
    }
    Ok(())
}

/// Moves every page of one window onto the right panel, or starts a panel session.
pub fn dock(app: &AppHandle, area: Option<SurfaceArea>, from_host: Option<String>) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    if !panel_supported() {
        return Err(PANEL_UNSUPPORTED.into());
    }
    PREFER_WINDOW.store(false, Ordering::Relaxed);
    let sources = {
        let state = surface();
        if let Some(id) = from_host {
            vec![id]
        } else {
            state
                .session
                .hosts
                .iter()
                .filter(|host| host.kind == HostKind::Window && !host.order.is_empty())
                .map(|host| host.id.clone())
                .collect::<Vec<_>>()
        }
    };
    let pending = sources
        .iter()
        .filter(|id| surface().session.active_of(id).is_some())
        .cloned()
        .collect::<Vec<_>>();
    if pending.is_empty() {
        if surface().session.coarse_host() == Some("panel") {
            if let Some(area) = area {
                if let Some(host) = surface().live.iter_mut().find(|host| host.id == PANEL_HOST_ID) {
                    host.area = Some(area);
                    host.visible = true;
                }
            }
            layout(app)?;
            if let Some(id) = surface().session.active_of(PANEL_HOST_ID) {
                request_panel(app, &id);
            }
            changed(app);
            return Ok(());
        }
        return open_panel(app, area);
    }
    let main = app
        .get_window(PANEL_WINDOW_LABEL)
        .ok_or("メインウィンドウが見つかりません")?;
    ensure_panel_live(area, area.is_some());
    for source_id in pending {
        let ids = surface()
            .session
            .hosts
            .iter()
            .find(|host| host.id == source_id)
            .map(|host| host.order.clone())
            .unwrap_or_default();
        let window_label = live_label(&surface(), &source_id);
        let source = window_label.as_ref().and_then(|label| app.get_window(label));
        let tokens = move_tab_ids(app, &ids, PANEL_HOST_ID)?;
        if let Err(error) = reparent_ids(app, &ids, &main, source.as_ref(), tokens) {
            layout(app)?;
            changed(app);
            return Err(error);
        }
        // The session already lists these tabs on the panel. Forget the window
        // before destroy so its Destroyed handler cannot drop the moved tabs.
        {
            let mut state = surface();
            state.live.retain(|host| host.id != source_id);
            let _ = state.session.drop_host_if_empty(&source_id);
        }
        if let Some(source) = source {
            let _ = source.destroy();
        }
    }
    layout(app)?;
    raise(&main);
    if let Some(id) = surface().session.active_of(PANEL_HOST_ID) {
        request_panel(app, &id);
    }
    changed(app);
    Ok(())
}

fn move_tab_ids(app: &AppHandle, ids: &[String], to_host: &str) -> Result<Vec<MoveToken>, String> {
    let _ = app;
    move_ids(ids, to_host)
}

/// Moves every page that is in the right panel into one new window.
pub fn undock(app: &AppHandle) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    PREFER_WINDOW.store(true, Ordering::Relaxed);
    let ids = surface()
        .session
        .hosts
        .iter()
        .find(|host| host.id == PANEL_HOST_ID)
        .map(|host| host.order.clone())
        .unwrap_or_default();
    if ids.is_empty() {
        if surface().session.tabs.is_empty() {
            return open_window(app);
        }
        raise_focused(app);
        return Ok(());
    }
    let host_id = surface().session.add_window();
    let slot = surface()
        .session
        .slot_of(&host_id)
        .ok_or("表示先が見つかりません")?;
    let (window, toolbar) = match build_browser_window(app, slot, None) {
        Ok(built) => built,
        Err(error) => {
            surface().session.take_host(&host_id);
            return Err(error);
        }
    };
    let generation = register_live(LiveHost {
        id: host_id.clone(),
        kind: HostKind::Window,
        window_label: window.label().to_string(),
        toolbar_label: Some(toolbar),
        generation: 0,
        area: None,
        visible: true,
        toolbar_height: browser_tabs::DEFAULT_TOOLBAR_HEIGHT,
    });
    let main = app.get_window(PANEL_WINDOW_LABEL);
    let tokens = match move_ids(&ids, &host_id) {
        Ok(tokens) => tokens,
        Err(error) => {
            discard_host(app, &host_id, Some(&window));
            return Err(error);
        }
    };
    if let Err(error) = reparent_ids(app, &ids, &window, main.as_ref(), tokens) {
        discard_host(app, &host_id, Some(&window));
        layout(app)?;
        changed(app);
        return Err(error);
    }
    if let Err(error) = install_browser_menu(app, &window, &host_id, slot) {
        browser::browser_log(app, &format!("メニューを設定できません {error}"));
    }
    watch_browser_window(app, &window, &host_id, generation);
    {
        let mut state = surface();
        let _ = state.session.drop_host_if_empty(PANEL_HOST_ID);
        if state.session.kind_of(PANEL_HOST_ID).is_none() {
            state.live.retain(|host| host.id != PANEL_HOST_ID);
        }
    }
    layout(app)?;
    raise(&window);
    changed(app);
    Ok(())
}

pub fn panel_area(app: &AppHandle, area: Option<SurfaceArea>, visible: bool) -> Result<(), String> {
    let docked = {
        let mut state = surface();
        let Some(host) = state.live.iter_mut().find(|host| host.id == PANEL_HOST_ID) else {
            return Ok(());
        };
        if area.is_some() {
            host.area = area;
        }
        host.visible = visible && area.is_some();
        true
    };
    if docked {
        layout(app)?;
    }
    Ok(())
}

pub fn new_tab_on(app: &AppHandle, url: Option<&str>, host_id: Option<String>) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let target = checked_url(app, url.unwrap_or(browser::LIBRARY_URL))?;
    if surface().session.tabs.is_empty() {
        match host_id.as_deref() {
            Some(PANEL_HOST_ID) if !panel_supported() => return Err(PANEL_UNSUPPORTED.into()),
            Some(PANEL_HOST_ID) => open_panel(app, None)?,
            Some(_) => return Err("表示先が見つかりません".into()),
            None => open(app)?,
        }
        if target.as_str() != browser::LIBRARY_URL {
            let id = surface()
                .session
                .focused_tab()
                .ok_or("タブがありません")?;
            view_for(app, &id)?
                .navigate(target)
                .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let host = host_id.unwrap_or_else(|| {
        surface()
            .session
            .focused_host()
            .filter(|id| surface().session.kind_of(id).is_some())
            .or_else(|| surface().session.primary_host().map(str::to_string))
            .unwrap_or_else(|| PANEL_HOST_ID.to_string())
    });
    if surface().session.kind_of(&host).is_none() {
        if host == PANEL_HOST_ID {
            if !panel_supported() {
                return Err(PANEL_UNSUPPORTED.into());
            }
            ensure_panel_live(None, false);
        } else {
            return Err("表示先が見つかりません".into());
        }
    } else if host == PANEL_HOST_ID {
        ensure_panel_live(None, false);
    }
    let label = live_label(&surface(), &host).ok_or("表示先が見つかりません")?;
    let window = app
        .get_window(&label)
        .ok_or("表示先が見つかりません")?;
    mount_tab(app, &host, &window, target)?;
    if host == PANEL_HOST_ID {
        if let Some(id) = surface().session.active_of(PANEL_HOST_ID) {
            request_panel(app, &id);
        }
    }
    Ok(())
}

pub fn new_tab(app: &AppHandle, url: Option<&str>) -> Result<(), String> {
    new_tab_on(app, url, None)?;
    // An already-open window does not come forward when a tab is added.
    // Match open(): show and focus. Do not navigate to an age-gate URL from here.
    if let Some(window) = host_window(app) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    Ok(())
}

pub fn navigate_tab(app: &AppHandle, tab_id: &str, input: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let url = checked_url(app, input)?;
    view_for(app, tab_id)?
        .navigate(url)
        .map_err(|error| format!("ページを開けません: {error}"))
}

pub fn navigate(app: &AppHandle, input: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let url = checked_url(app, input)?;
    if host_window(app).is_none() && surface().session.tabs.is_empty() {
        open(app)?;
    }
    active_view(app)?
        .navigate(url)
        .map_err(|error| format!("ページを開けません: {error}"))
}

pub fn select(app: &AppHandle, id: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    surface().session.select(id)?;
    layout(app)?;
    if let Ok(view) = view_for(app, id) {
        let _ = view.set_focus();
    }
    changed(app);
    Ok(())
}

pub fn close_tab(app: &AppHandle, id: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    if !surface().session.has(id) {
        return Err("タブが見つかりません".into());
    }
    if let Some(view) = app.get_webview(id) {
        view.close().map_err(|error| error.to_string())?;
    }
    let emptied = surface().session.remove(id)?;
    if let Some(host_id) = emptied {
        // The tab is already gone. Drop the window only when it was the last page there.
        retire_if_empty(app, &host_id);
        if surface().session.kind_of(&host_id).is_none() {
            if let Some(host) = forget_live(&host_id) {
                if host.kind == HostKind::Window {
                    if let Some(source) = app.get_window(&host.window_label) {
                        let _ = source.destroy();
                    }
                }
            }
        }
    } else {
        layout(app)?;
    }
    changed(app);
    Ok(())
}

pub fn reorder(app: &AppHandle, host_id: &str, ids: &[String]) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    surface().session.reorder(host_id, ids)?;
    changed(app);
    Ok(())
}

pub fn detach_tab(app: &AppHandle, tab_id: &str, x: f64, y: f64) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let (x, y) = browser_tabs::detach_position(x, y)?;
    let from = surface()
        .session
        .host_of(tab_id)
        .ok_or("タブが見つかりません")?;
    let host_id = surface().session.add_window();
    let slot = surface()
        .session
        .slot_of(&host_id)
        .ok_or("表示先が見つかりません")?;
    let (window, toolbar) = match build_browser_window(app, slot, Some((x, y))) {
        Ok(built) => built,
        Err(error) => {
            surface().session.take_host(&host_id);
            return Err(error);
        }
    };
    let generation = register_live(LiveHost {
        id: host_id.clone(),
        kind: HostKind::Window,
        window_label: window.label().to_string(),
        toolbar_label: Some(toolbar),
        generation: 0,
        area: None,
        visible: true,
        toolbar_height: browser_tabs::DEFAULT_TOOLBAR_HEIGHT,
    });
    let token = match surface().session.move_tab(tab_id, &host_id, 0) {
        Ok(token) => token,
        Err(error) => {
            discard_host(app, &host_id, Some(&window));
            return Err(error);
        }
    };
    if let Err(error) = reparent_tab(app, tab_id, &window) {
        let _ = surface().session.restore(token);
        discard_host(app, &host_id, Some(&window));
        layout(app)?;
        changed(app);
        return Err(error);
    }
    if let Err(error) = install_browser_menu(app, &window, &host_id, slot) {
        browser::browser_log(app, &format!("メニューを設定できません {error}"));
    }
    watch_browser_window(app, &window, &host_id, generation);
    retire_if_empty(app, &from);
    PREFER_WINDOW.store(true, Ordering::Relaxed);
    layout(app)?;
    raise(&window);
    changed(app);
    Ok(())
}

pub fn dock_tab(app: &AppHandle, tab_id: &str, index: Option<usize>) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    if !panel_supported() {
        return Err(PANEL_UNSUPPORTED.into());
    }
    let from = surface()
        .session
        .host_of(tab_id)
        .ok_or("タブが見つかりません")?;
    if from == PANEL_HOST_ID {
        if let Some(id) = Some(tab_id.to_string()) {
            request_panel(app, &id);
        }
        return Ok(());
    }
    PREFER_WINDOW.store(false, Ordering::Relaxed);
    let main = app
        .get_window(PANEL_WINDOW_LABEL)
        .ok_or("メインウィンドウが見つかりません")?;
    ensure_panel_live(None, false);
    let at = index.unwrap_or(usize::MAX);
    let token = match surface().session.move_tab(tab_id, PANEL_HOST_ID, at) {
        Ok(token) => token,
        Err(error) => return Err(error),
    };
    let source_label = live_label(&surface(), &from);
    let source = source_label.as_ref().and_then(|label| app.get_window(label));
    if let Err(error) = reparent_tab(app, tab_id, &main) {
        let _ = surface().session.restore(token);
        layout(app)?;
        changed(app);
        return Err(error);
    }
    retire_if_empty(app, &from);
    let _ = source;
    layout(app)?;
    raise(&main);
    request_panel(app, tab_id);
    changed(app);
    Ok(())
}

pub fn close(app: &AppHandle) -> Result<(), String> {
    let (hosts, ids) = {
        let mut state = surface();
        let hosts = std::mem::take(&mut state.live);
        let ids = state
            .session
            .tabs
            .iter()
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        state.session.clear();
        (hosts, ids)
    };
    close_webviews(app, &ids);
    for host in hosts {
        if let Some(label) = host.toolbar_label {
            if let Some(view) = app.get_webview(&label) {
                let _ = view.close();
            }
        }
        if host.kind == HostKind::Window {
            if let Some(source) = app.get_window(&host.window_label) {
                let _ = source.destroy();
            }
        }
    }
    changed(app);
    Ok(())
}

fn history_on(app: &AppHandle, view: Webview, forward: bool) -> Result<(), String> {
    let handle = app.clone();
    view.with_webview(move |native| {
        if let Err(error) = native_history(&native, Some(forward)) {
            let _ = handle.emit("browser-operation-error", error);
        }
    })
    .map_err(|error| format!("履歴を移動できません: {error}"))
}

pub fn history_tab(app: &AppHandle, tab_id: &str, forward: bool) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    history_on(app, view_for(app, tab_id)?, forward)
}

fn history_host(app: &AppHandle, host_id: &str, forward: bool) -> Result<(), String> {
    let id = surface()
        .session
        .active_of(host_id)
        .ok_or("ブラウザを開いてから操作してください")?;
    history_tab(app, &id, forward)
}

pub fn history(app: &AppHandle, forward: bool) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    history_on(app, active_view(app)?, forward)
}

pub fn reload_tab(app: &AppHandle, tab_id: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    view_for(app, tab_id)?
        .reload()
        .map_err(|error| format!("ページを読み直せません: {error}"))
}

fn reload_host(app: &AppHandle, host_id: &str) -> Result<(), String> {
    let id = surface()
        .session
        .active_of(host_id)
        .ok_or("ブラウザを開いてから操作してください")?;
    reload_tab(app, &id)
}

pub fn reload(app: &AppHandle) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    active_view(app)?
        .reload()
        .map_err(|error| format!("ページを読み直せません: {error}"))
}

pub fn stop_tab(app: &AppHandle, tab_id: &str) -> Result<(), String> {
    browser::browser_enabled(app)?;
    mutable()?;
    let view = view_for(app, tab_id)?;
    view.eval("window.stop() ").map_err(|error| error.to_string())?;
    update_page(app, view.label(), None, None, Some(false));
    Ok(())
}

pub fn stop(app: &AppHandle) -> Result<(), String> {
    let id = surface()
        .session
        .focused_tab()
        .ok_or("ブラウザを開いてから操作してください")?;
    stop_tab(app, &id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser_tabs::HostKind;

    fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source.find(name).unwrap_or_else(|| panic!("missing {name}"));
        let rest = &source[start..];
        let after = rest.find('\n').unwrap() + 1;
        let tail = &rest[after..];
        let end = tail
            .find("\nfn ")
            .or_else(|| tail.find("\npub fn "))
            .or_else(|| tail.find("\npub(crate) fn "))
            .unwrap_or(tail.len());
        &rest[..after + end]
    }

    #[test]
    fn layout_uses_host_relative_coordinates_for_a_docked_panel() {
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

    #[test]
    fn docked_tabs_hide_until_the_panel_is_measured_and_uncovered() {
        let area = SurfaceArea::from_panel(820.0, 96.0, 420.0, 600.0).unwrap();
        assert!(panel_content(None, true).is_none());
        assert!(panel_content(Some(area), false).is_none());
        let shown = panel_content(Some(area), true).expect("shown");
        assert_eq!(
            (shown.left, shown.top, shown.width, shown.height),
            (820.0, 96.0, 420.0, 600.0)
        );
        let collapsed = SurfaceArea::from_panel(820.0, 96.0, 0.0, 600.0).unwrap();
        assert!(panel_content(Some(collapsed), true).is_none());
    }

    #[test]
    fn panel_rectangles_reject_non_finite_values_and_stay_in_range() {
        assert!(SurfaceArea::from_panel(f64::NAN, 0.0, 10.0, 10.0).is_err());
        assert!(SurfaceArea::from_panel(0.0, 0.0, f64::INFINITY, 10.0).is_err());
        let area = SurfaceArea::from_panel(-5.0, -1.0, 1e9, 30.0).unwrap();
        assert_eq!((area.left, area.top, area.width), (0.0, 0.0, MAX_PANEL_EDGE));
    }

    #[test]
    fn new_sessions_start_in_the_panel_only_where_it_is_supported() {
        assert!(start_in_panel(true, false, true));
        assert!(!start_in_panel(false, false, true));
        assert!(!start_in_panel(true, true, true));
        assert!(!start_in_panel(true, false, false));
        assert_eq!(panel_supported(), cfg!(windows));
        assert_eq!(HostKind::Window.name(), "window");
        assert_eq!(HostKind::Panel.name(), "panel");
    }

    #[test]
    fn the_panel_host_never_owns_or_destroys_the_main_window() {
        assert_eq!(PANEL_WINDOW_LABEL, crate::presence::MAIN_WINDOW);
        let source = include_str!("browser_surface.rs");
        let retire = function_body(source, "fn retire_if_empty");
        let destroyed = retire.find("source.destroy()").expect("destroy empty window");
        let kind = retire.find("HostKind::Window").expect("only windows");
        assert!(kind < destroyed);
    }

    #[test]
    fn docking_moves_the_session_before_the_old_window_is_destroyed() {
        let source = include_str!("browser_surface.rs");
        let body = function_body(source, "pub fn dock");
        assert!(body.contains("mutable()?"));
        let moved = body.find("move_tab_ids(").expect("move tabs");
        let destroyed = body.find("source.destroy()").expect("close old window");
        assert!(moved < destroyed);
        assert!(body.contains("state.live.retain"));
        assert!(body.find("state.live.retain").unwrap() < destroyed);
    }

    #[test]
    fn detaching_reparents_before_retiring_and_restores_when_that_fails() {
        let source = include_str!("browser_surface.rs");
        let body = function_body(source, "pub fn detach_tab");
        let reparent = body.find("reparent_tab(").expect("reparent");
        let restore = body.find("session.restore").expect("restore");
        let retire = body.find("retire_if_empty").expect("retire");
        assert!(restore < reparent || body.find("if let Err(error) = reparent_tab").unwrap() < retire);
        assert!(reparent < retire);
        assert!(body.contains("session.restore"));
        assert!(!body.contains("age_verification"));
    }

    #[test]
    fn adding_a_tab_raises_an_existing_window() {
        let source = include_str!("browser_surface.rs");
        let start = source.find("pub fn new_tab(").expect("new_tab");
        let rest = &source[start..];
        let end = rest.find("\npub fn navigate_tab").expect("navigate");
        let body = &rest[..end];
        let raised = body.rfind("host_window(app)").expect("focus the existing window");
        let focus = &body[raised..];
        assert!(focus.contains(".show()"));
        assert!(focus.contains("unminimize()"));
        assert!(focus.contains("set_focus()"));
        assert!(!body.contains("age_verification"));
    }
}
