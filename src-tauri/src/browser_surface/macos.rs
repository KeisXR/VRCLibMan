//! Native placement of a docked page below the main window's HTML controls.
use super::{SurfaceArea, macos_geometry::{Frame, panel_frame}};
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
        let layout: NSRect = msg_send![parent, convertRect: content fromView: std::ptr::null::<AnyObject>()];
        let flipped: bool = msg_send![parent, isFlipped];
        let frame = panel_frame(
            Frame { x: area.left, y: area.top, width: area.width, height: area.height },
            Frame { x: layout.origin.x, y: layout.origin.y, width: layout.size.width, height: layout.size.height },
            flipped,
        );
        let rect = NSRect::new(NSPoint::new(frame.x, frame.y), NSSize::new(frame.width, frame.height));
        // Do not first call wry::set_bounds: its full-parent origin would briefly
        // place the page on top of the bookmarks before this corrected frame.
        let _: () = msg_send![page, setFrame: rect];
        let _: () = msg_send![page, setClipsToBounds: true];
    })
    .map_err(|error| format!("ブラウザの表示領域を設定できません: {error}"))
}
