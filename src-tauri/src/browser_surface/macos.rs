//! Native placement of a docked page below the main window's HTML controls.
use super::{
    macos_geometry::{panel_frame, Frame},
    SurfaceArea,
};
use objc2::{msg_send, runtime::AnyObject};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::Webview;

pub fn set_panel_bounds(view: &Webview, area: SurfaceArea) -> Result<(), String> {
    view.with_webview(move |native| unsafe {
        let page = native.inner().cast::<AnyObject>();
        let parent: *mut AnyObject = msg_send![page, superview];
        let window: *mut AnyObject = msg_send![page, window];
        // A closing/reparenting view may already have left its window.
        if parent.is_null() || window.is_null() {
            return;
        }
        let content: NSRect = msg_send![window, contentLayoutRect];
        let layout: NSRect =
            msg_send![parent, convertRect: content fromView: std::ptr::null::<AnyObject>()];
        let flipped: bool = msg_send![parent, isFlipped];
        let frame = panel_frame(
            Frame {
                x: area.left,
                y: area.top,
                width: area.width,
                height: area.height,
            },
            Frame {
                x: layout.origin.x,
                y: layout.origin.y,
                width: layout.size.width,
                height: layout.size.height,
            },
            flipped,
        );
        let rect = NSRect::new(
            NSPoint::new(frame.x, frame.y),
            NSSize::new(frame.width, frame.height),
        );
        // Do not first call wry::set_bounds: its full-parent origin would briefly
        // place the page on top of the bookmarks before this corrected frame.
        let _: () = msg_send![page, setFrame: rect];
        let _: () = msg_send![page, setClipsToBounds: true];
    })
    .map_err(|error| format!("ブラウザの表示領域を設定できません: {error}"))
}

/// Captures a PNG data URL of the page's visible viewport so the right panel can keep showing
/// the page while a menu, dialog, or drag temporarily hides the native WKWebView.
pub async fn capture_snapshot(view: &Webview) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel::<Option<String>>();
    let sender = std::sync::Mutex::new(Some(sender));
    view.with_webview(move |native| unsafe {
        let send = move |value: Option<String>| {
            if let Ok(mut guard) = sender.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(value);
                }
            }
        };
        let page = native.inner().cast::<AnyObject>();
        let window: *mut AnyObject = msg_send![page, window];
        let bounds: NSRect = msg_send![page, bounds];
        if window.is_null() || bounds.size.width < 2.0 || bounds.size.height < 2.0 {
            send(None);
            return;
        }
        let handler = block2::RcBlock::new(move |image: *mut AnyObject, error: *mut AnyObject| {
            if image.is_null() || !error.is_null() {
                send(None);
                return;
            }
            let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
            if tiff.is_null() {
                send(None);
                return;
            }
            let Some(rep_cls) = objc2::runtime::AnyClass::get(c"NSBitmapImageRep") else {
                send(None);
                return;
            };
            let rep: *mut AnyObject = msg_send![rep_cls, imageRepWithData: tiff];
            if rep.is_null() {
                send(None);
                return;
            }
            let Some(dict_cls) = objc2::runtime::AnyClass::get(c"NSDictionary") else {
                send(None);
                return;
            };
            let props: *mut AnyObject = msg_send![dict_cls, dictionary];
            // NSBitmapImageFileTypePNG = 4
            let png: *mut AnyObject =
                msg_send![rep, representationUsingType: 4usize properties: props];
            if png.is_null() {
                send(None);
                return;
            }
            let b64: *mut AnyObject = msg_send![png, base64EncodedStringWithOptions: 0usize];
            if b64.is_null() {
                send(None);
                return;
            }
            let utf8: *const std::ffi::c_char = msg_send![b64, UTF8String];
            if utf8.is_null() {
                send(None);
                return;
            }
            let encoded = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
            send(Some(format!("data:image/png;base64,{encoded}")));
        });
        let config: *mut AnyObject = std::ptr::null_mut();
        let _: () =
            msg_send![page, takeSnapshotWithConfiguration: config completionHandler: &*handler];
    })
    .map_err(|error| format!("ブラウザの表示を取得できません: {error}"))?;
    Ok(
        tokio::time::timeout(std::time::Duration::from_millis(1500), receiver)
            .await
            .ok()
            .and_then(Result::ok)
            .flatten(),
    )
}
