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
    use gtk::gdk::{keys::constants, ModifierType as M};
    let state = state
        & (M::SHIFT_MASK
            | M::CONTROL_MASK
            | M::MOD1_MASK
            | M::MOD4_MASK
            | M::MOD5_MASK
            | M::SUPER_MASK
            | M::HYPER_MASK
            | M::META_MASK);
    if state == M::CONTROL_MASK {
        match key.to_lower() {
            constants::t => Some("browser-new-tab"),
            constants::w => Some("browser-close-tab"),
            constants::l => Some("browser-address"),
            constants::r => Some("browser-reload"),
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

fn install_shortcuts(view: &webkit2gtk::WebView, app: &AppHandle, host_label: &str) {
    use gtk::{glib::Propagation, prelude::*};
    let app = app.clone();
    let host_label = host_label.to_string();
    view.connect_key_press_event(move |_, event| {
        let Some(action) = shortcut(event.keyval(), event.state()) else {
            return Propagation::Proceed;
        };
        if browser_surface::host_window(&app)
            .is_none_or(|host| host.label() != host_label || !host.is_focused().unwrap_or(false))
        {
            return Propagation::Proceed;
        }
        browser_surface::dispatch_shortcut(&app, action);
        Propagation::Stop
    });
}

/// Tauri places Linux child views in a GtkBox, where WebView bounds are ignored.
/// A shared canvas gives them the same host-relative coordinates as other platforms.
pub fn set_view_bounds(
    view: &Webview,
    area: SurfaceArea,
    toolbar: bool,
    fill_host: bool,
) -> Result<(), String> {
    use gtk::prelude::*;
    let app = view.app_handle().clone();
    let host_label = view.window().label().to_string();
    view.with_webview(move |native| {
        let child = native.inner();
        let Some(parent) = child.parent() else {
            return;
        };
        let canvas = if let Ok(canvas) = parent.clone().downcast::<gtk::Layout>() {
            canvas
        } else if let Ok(container) = parent.downcast::<gtk::Box>() {
            install_resize_handlers(&child);
            // GtkMenuItem accelerators require visible ancestors, even when the
            // accelerator group remains installed. Handle browser keys on each view.
            install_shortcuts(&child, &app, &host_label);
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
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_shortcuts_require_their_exact_modifiers() {
        use gtk::gdk::{keys::constants as K, ModifierType as M};
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
