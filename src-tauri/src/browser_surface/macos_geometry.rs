//! Logical-point geometry shared by native placement and the AppKit regression test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `area` is a top-left DOM rectangle. `layout` is NSWindow.contentLayoutRect,
/// converted into the page's superview coordinates (not the full contentView).
/// AppKit reserves the visible titlebar even when Tauri enables FullSizeContentView;
/// WKWebView's DOM viewport starts below it. No backing-scale conversion is needed.
pub fn panel_frame(area: Frame, layout: Frame, flipped: bool) -> Frame {
    Frame {
        x: layout.x + area.x,
        y: if flipped {
            layout.y + area.y
        } else {
            layout.y + layout.height - area.y - area.height
        },
        width: area.width,
        height: area.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_titlebar_is_excluded_from_the_dom_origin() {
        // Measured in native macOS CI: a 674pt contentView includes a 32pt
        // titlebar, while JS innerHeight and contentLayoutRect are 642pt.
        let area = Frame { x: 780., y: 77., width: 400., height: 565. };
        let layout = Frame { x: 0., y: 0., width: 1024., height: 642. };
        let frame = panel_frame(area, layout, false);
        assert_eq!(frame, Frame { x: 780., y: 0., width: 400., height: 565. });
        // The old parent-height conversion put the page 32pt too high,
        // directly over the 32pt bookmark row.
        assert_eq!(674. - area.y - area.height - frame.y, 32.);
    }

    #[test]
    fn layout_origin_and_flipped_views_are_respected() {
        let area = Frame { x: 10., y: 80., width: 300., height: 200. };
        let layout = Frame { x: 3., y: 5., width: 640., height: 480. };
        assert_eq!(panel_frame(area, layout, false), Frame { x: 13., y: 205., width: 300., height: 200. });
        assert_eq!(panel_frame(area, layout, true), Frame { x: 13., y: 85., width: 300., height: 200. });
    }

    #[test]
    fn fullscreen_without_a_titlebar_needs_no_inset() {
        let area = Frame { x: 500., y: 100., width: 400., height: 600. };
        let layout = Frame { x: 0., y: 0., width: 900., height: 700. };
        assert_eq!(panel_frame(area, layout, false).y, 0.);
    }
}
