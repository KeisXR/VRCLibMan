//! Linux native placement and the dedicated browser profile's in-memory data.
use crate::{
    browser,
    browser_surface::{self, SurfaceArea},
};
use tauri::{AppHandle, Manager, Webview};
use webkit2gtk::{WebContextExt, WebViewExt, WebsiteDataManagerExtManual};

thread_local! {
    // WebKit keeps its context alive after the last view closes. Retain only the
    // dedicated browser manager so logout also clears its cached session cookies.
    static PROFILE: std::cell::RefCell<Option<webkit2gtk::WebsiteDataManager>> = const { std::cell::RefCell::new(None) };
}

pub fn remember_profile(view: &Webview) -> Result<(), String> {
    view.with_webview(|native| {
        let manager = native
            .inner()
            .context()
            .and_then(|context| context.website_data_manager());
        PROFILE.with(|slot| *slot.borrow_mut() = manager);
    })
    .map_err(|e| e.to_string())
}

pub async fn clear_profile_cache(app: &AppHandle) -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    browser::dispatch_ui(app.clone(), move |_| {
        PROFILE.with(|slot| {
            if let Some(manager) = slot.borrow().as_ref() {
                manager.clear(
                    webkit2gtk::WebsiteDataTypes::ALL,
                    gtk::glib::TimeSpan::from_seconds(0),
                    None::<&webkit2gtk::gio::Cancellable>,
                    move |result| {
                        let _ = sender
                            .send(result.map_err(|_| "ログイン状態を消せませんでした".to_string()));
                    },
                );
            } else {
                let _ = sender.send(Ok(()));
            }
        });
        Ok(())
    })
    .await?;
    tauri::async_runtime::spawn_blocking(move || {
        receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|_| "ログイン状態の削除が完了しませんでした".to_string())?
    })
    .await
    .map_err(|_| "ログイン状態を消せませんでした".to_string())?
}

/// Child WebViews do not get Tauri's automatic borderless resize handlers.
fn resize_edge(
    width: f64,
    height: f64,
    x: f64,
    y: f64,
) -> Option<(gtk::gdk::WindowEdge, &'static str)> {
    use gtk::gdk::WindowEdge::*;
    if x < 0.0 || y < 0.0 || x >= width || y >= height {
        return None;
    }
    match (x < 5.0, x >= width - 5.0, y < 5.0, y >= height - 5.0) {
        (true, _, true, _) => Some((NorthWest, "nw-resize")),
        (_, true, true, _) => Some((NorthEast, "ne-resize")),
        (true, _, _, true) => Some((SouthWest, "sw-resize")),
        (_, true, _, true) => Some((SouthEast, "se-resize")),
        (true, _, _, _) => Some((West, "w-resize")),
        (_, true, _, _) => Some((East, "e-resize")),
        (_, _, true, _) => Some((North, "n-resize")),
        (_, _, _, true) => Some((South, "s-resize")),
        _ => None,
    }
}

fn install_resize_handlers(view: &webkit2gtk::WebView) {
    use gtk::{glib::Propagation, prelude::*};
    fn edge_at(
        view: &webkit2gtk::WebView,
        root: (f64, f64),
    ) -> Option<(gtk::Window, gtk::gdk::WindowEdge, &'static str)> {
        let window = view.toplevel()?.downcast::<gtk::Window>().ok()?;
        if window.is_decorated() || !window.is_resizable() || window.is_maximized() {
            return None;
        }
        let native = window.window()?;
        if native.state().contains(gtk::gdk::WindowState::FULLSCREEN) {
            return None;
        }
        let (left, top) = native.position();
        let (edge, cursor) = resize_edge(
            f64::from(native.width()),
            f64::from(native.height()),
            root.0 - f64::from(left),
            root.1 - f64::from(top),
        )?;
        Some((window, edge, cursor))
    }
    view.add_events(
        gtk::gdk::EventMask::POINTER_MOTION_MASK | gtk::gdk::EventMask::BUTTON_PRESS_MASK,
    );
    let had_cursor = std::cell::Cell::new(false);
    view.connect_motion_notify_event(move |view, event| {
        if let Some((_, _, cursor)) = edge_at(view, event.root()) {
            if let Some(native) = view.window() {
                native.set_cursor(gtk::gdk::Cursor::from_name(&native.display(), cursor).as_ref());
                had_cursor.set(true);
            }
            return Propagation::Stop;
        }
        if had_cursor.replace(false) {
            if let Some(native) = view.window() {
                native.set_cursor(None);
            }
        }
        Propagation::Proceed
    });
    view.connect_button_press_event(|view, event| {
        if event.button() != 1 {
            return Propagation::Proceed;
        }
        if let Some((window, edge, _)) = edge_at(view, event.root()) {
            let (x, y) = event.root();
            window.begin_resize_drag(edge, 1, x as i32, y as i32, event.time());
            return Propagation::Stop;
        }
        Propagation::Proceed
    });
}

fn shortcut(key: gtk::gdk::keys::Key, state: gtk::gdk::ModifierType) -> Option<&'static str> {
    use gtk::gdk::{ModifierType as M, keys::constants};
    let state = state
        & (M::SHIFT_MASK
            | M::CONTROL_MASK
            | M::MOD1_MASK
            | M::MOD4_MASK
            | M::MOD5_MASK
            | M::SUPER_MASK
            | M::HYPER_MASK
            | M::META_MASK);
    if state == (M::CONTROL_MASK | M::SHIFT_MASK) && key == constants::plus {
        return Some("browser-zoom-in");
    }
    if state == M::CONTROL_MASK {
        match key.to_lower() {
            constants::t => Some("browser-new-tab"),
            constants::w => Some("browser-close-tab"),
            constants::l => Some("browser-address"),
            constants::r => Some("browser-reload"),
            constants::plus | constants::equal | constants::KP_Add => Some("browser-zoom-in"),
            constants::minus | constants::KP_Subtract => Some("browser-zoom-out"),
            constants::_0 | constants::KP_0 => Some("browser-zoom-reset"),
            _ => None,
        }
    } else if state == M::MOD1_MASK {
        match key {
            constants::Left => Some("browser-back"),
            constants::Right => Some("browser-forward"),
            _ => None,
        }
    } else {
        None
    }
}

fn install_shortcuts(view: &webkit2gtk::WebView, app: &AppHandle, _host_label: &str) {
    use gtk::{glib::Propagation, prelude::*};
    let app = app.clone();
    view.connect_key_press_event(move |_, event| {
        let Some(action) = shortcut(event.keyval(), event.state()) else {
            return Propagation::Proceed;
        };
        // The key arrives on the focused page. Act on that host, not the window this view was created in.
        if browser_surface::host_window(&app).is_none_or(|host| !host.is_focused().unwrap_or(false))
        {
            return Propagation::Proceed;
        }
        browser_surface::dispatch_shortcut(&app, action);
        Propagation::Stop
    });
}

/// Keep the main webview filling the host. Pages are native overlay children,
/// rather than another expanding GtkBox row that would shrink the main UI.
pub fn prepare_panel(app: &AppHandle) -> Result<(), String> {
    let main = app
        .get_webview(crate::presence::MAIN_WINDOW)
        .ok_or("メイン画面が見つかりません")?;
    let (send, receive) = std::sync::mpsc::channel();
    main.with_webview(move |native| {
        use gtk::prelude::*;
        let child = native.inner();
        let result = (|| {
            let parent = child.parent().ok_or("メイン画面の表示先がありません")?;
            if parent.is::<gtk::Overlay>() {
                return Ok(());
            }
            let container = parent
                .downcast::<gtk::Box>()
                .map_err(|_| "メイン画面の表示先を変更できません")?;
            let overlay = gtk::Overlay::new();
            overlay.set_hexpand(true);
            overlay.set_vexpand(true);
            container.remove(&child);
            overlay.add(&child);
            container.pack_start(&overlay, true, true, 0);
            overlay.show();
            Ok::<_, &str>(())
        })();
        let _ = send.send(result.map_err(str::to_string));
    })
    .map_err(|error| error.to_string())?;
    receive
        .recv()
        .map_err(|_| "メイン画面の配置を確認できません".to_string())?
}

fn install_view_handlers(child: &webkit2gtk::WebView, app: &AppHandle, host_label: &str) {
    use gtk::prelude::*;
    // Reparenting returns the same view to a GtkBox. Install native callbacks
    // once per view, so repeated docking does not multiply handlers.
    let style = child.style_context();
    if !style.has_class("vrclibman-browser-handlers") {
        style.add_class("vrclibman-browser-handlers");
        install_resize_handlers(child);
        install_shortcuts(child, app, host_label);
    }
}

fn place_panel_view(child: &webkit2gtk::WebView, overlay: &gtk::Overlay, area: SurfaceArea) {
    use gtk::prelude::*;
    if child.parent().as_ref() != Some(overlay.upcast_ref()) {
        if let Some(parent) = child
            .parent()
            .and_then(|parent| parent.downcast::<gtk::Container>().ok())
        {
            parent.remove(child);
        }
        overlay.add_overlay(child);
    }
    child.set_halign(gtk::Align::Start);
    child.set_valign(gtk::Align::Start);
    child.set_margin_start(area.left as i32);
    child.set_margin_top(area.top as i32);
    child.set_size_request(area.width.max(1.0) as i32, area.height.max(1.0) as i32);
}

/// Tauri places Linux child views in a GtkBox, where WebView bounds are ignored.
/// Panels use the main GtkOverlay; separate windows keep their shared GtkLayout.
pub fn set_view_bounds(
    view: &Webview,
    area: SurfaceArea,
    toolbar: bool,
    fill_host: bool,
) -> Result<(), String> {
    use gtk::prelude::*;
    let app = view.app_handle().clone();
    let host_label = view.window().label().to_string();
    let (send, receive) = std::sync::mpsc::channel();
    view.with_webview(move |native| {
        let child = native.inner();
        install_view_handlers(&child, &app, &host_label);
        if !fill_host {
            let overlay = app
                .get_window(&host_label)
                .and_then(|window| window.default_vbox().ok())
                .and_then(|container| {
                    container
                        .children()
                        .into_iter()
                        .find_map(|widget| widget.downcast::<gtk::Overlay>().ok())
                });
            let result = if let Some(overlay) = overlay {
                place_panel_view(&child, &overlay, area);
                Ok(())
            } else {
                Err("ブラウザの表示先が見つかりません".to_string())
            };
            let _ = send.send(result);
            return;
        }
        // Panel offsets and alignments must not survive a move to a separate
        // window, where GtkLayout positions the toolbar and page explicitly.
        child.set_margin_start(0);
        child.set_margin_top(0);
        child.set_halign(gtk::Align::Fill);
        child.set_valign(gtk::Align::Fill);
        let Some(parent) = child.parent() else {
            let _ = send.send(Err("ブラウザの表示先がありません".into()));
            return;
        };
        let canvas = if let Ok(canvas) = parent.clone().downcast::<gtk::Layout>() {
            canvas
        } else if let Ok(container) = parent.downcast::<gtk::Box>() {
            let existing = container
                .children()
                .into_iter()
                .find_map(|widget| widget.downcast::<gtk::Layout>().ok());
            let canvas = existing.unwrap_or_else(|| {
                let canvas = gtk::Layout::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
                canvas.set_hexpand(true);
                canvas.set_vexpand(true);
                container.pack_start(&canvas, true, true, 0);
                let previous = std::cell::Cell::new((0, 0));
                let handle = app.clone();
                canvas.connect_size_allocate(move |_, allocation| {
                    let size = (allocation.width(), allocation.height());
                    if previous.replace(size) != size {
                        let handle = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = browser::dispatch_ui(handle, browser_surface::layout).await;
                        });
                    }
                });
                canvas.show();
                canvas
            });
            container.remove(&child);
            canvas.put(&child, area.left as i32, area.top as i32);
            canvas
        } else {
            let _ = send.send(Err("ブラウザの表示先を変更できません".into()));
            return;
        };
        let width = if fill_host && canvas.allocated_width() > 1 {
            f64::from(canvas.allocated_width())
        } else {
            area.width
        };
        let height = if fill_host && !toolbar && canvas.allocated_height() > 1 {
            f64::from(canvas.allocated_height()) - area.top
        } else {
            area.height
        };
        let (x, y, width, height) = (
            area.left as i32,
            area.top as i32,
            width.max(1.0) as i32,
            height.max(1.0) as i32,
        );
        child.set_size_request(width, height);
        canvas.move_(&child, x, y);
        child.size_allocate(&gtk::Allocation::new(x, y, width, height));
        let _ = send.send(Ok(()));
    })
    .map_err(|e| e.to_string())?;
    receive
        .recv()
        .map_err(|_| "ブラウザの配置を確認できません".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_shortcuts_require_their_exact_modifiers() {
        use gtk::gdk::{ModifierType as M, keys::constants as K};
        assert_eq!(shortcut(K::t, M::CONTROL_MASK), Some("browser-new-tab"));
        assert_eq!(
            shortcut(K::l, M::CONTROL_MASK | M::LOCK_MASK),
            Some("browser-address")
        );
        assert_eq!(shortcut(K::Left, M::MOD1_MASK), Some("browser-back"));
        for state in [
            M::empty(),
            M::CONTROL_MASK | M::MOD1_MASK,
            M::CONTROL_MASK | M::SHIFT_MASK,
        ] {
            assert_eq!(shortcut(K::t, state), None);
        }
        assert_eq!(shortcut(K::c, M::CONTROL_MASK), None);
        assert_eq!(shortcut(K::equal, M::CONTROL_MASK), Some("browser-zoom-in"));
        assert_eq!(
            shortcut(K::plus, M::CONTROL_MASK | M::SHIFT_MASK),
            Some("browser-zoom-in")
        );
        assert_eq!(
            shortcut(K::KP_Subtract, M::CONTROL_MASK),
            Some("browser-zoom-out")
        );
        assert_eq!(shortcut(K::_0, M::CONTROL_MASK), Some("browser-zoom-reset"));
        assert_eq!(shortcut(K::minus, M::CONTROL_MASK | M::MOD1_MASK), None);
    }
    #[test]
    fn resize_hits_only_the_outer_edges_of_the_host() {
        use gtk::gdk::WindowEdge::*;
        for (x, y, edge) in [
            (0.0, 0.0, NorthWest),
            (799.0, 0.0, NorthEast),
            (0.0, 599.0, SouthWest),
            (799.0, 599.0, SouthEast),
            (0.0, 300.0, West),
            (799.0, 300.0, East),
            (400.0, 0.0, North),
            (400.0, 599.0, South),
        ] {
            assert_eq!(resize_edge(800.0, 600.0, x, y).map(|hit| hit.0), Some(edge));
        }
        for (x, y) in [(5.0, 5.0), (400.0, 300.0), (-1.0, 0.0), (800.0, 300.0)] {
            assert!(resize_edge(800.0, 600.0, x, y).is_none());
        }
    }
}
