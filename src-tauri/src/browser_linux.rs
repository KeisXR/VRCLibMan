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
    view.with_webview(move |native| {
        let child = native.inner();
        let Some(parent) = child.parent() else {
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
