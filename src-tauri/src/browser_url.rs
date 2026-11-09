//! Read the current native URL without assuming that WKWebView has finished loading.
use tauri::Webview;
use url::Url;

const UNAVAILABLE: &str = "ページのURLをまだ取得できません。読み込み後にもう一度お試しください";

#[cfg(not(target_os = "macos"))]
pub fn current_url(view: &Webview) -> Result<Url, String> {
    view.url().map_err(|_| UNAVAILABLE.to_string())
}

#[cfg(target_os = "macos")]
pub fn current_url(view: &Webview) -> Result<Url, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    // Tauri runs this callback on the UI thread (inline when already there).
    // Wry 0.57's url() unwraps WKWebView.URL, which may be nil before navigation
    // or during teardown. With panic=abort, wrapping that getter in Result is insufficient.
    view.with_webview(move |native| {
        let result = unsafe {
            native
                .inner()
                .cast::<objc2::runtime::AnyObject>()
                .as_ref()
                .ok_or_else(|| UNAVAILABLE.to_string())
                .and_then(|webview| macos::read_url(webview))
        };
        let _ = sender.send(result);
    })
    .map_err(|_| UNAVAILABLE.to_string())?;
    receiver
        .recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| UNAVAILABLE.to_string())?
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::{rc::Retained, runtime::AnyObject};
    use objc2_foundation::NSURL;

    // SAFETY: The caller supplies a live WKWebView on the main thread. URL is
    // nullable; retain the NSURL while reading its nullable absoluteString.
    pub(super) unsafe fn read_url(webview: &AnyObject) -> Result<Url, String> {
        let native_url: Option<Retained<NSURL>> = unsafe { objc2::msg_send![webview, URL] };
        let text = native_url
            .and_then(|url| url.absoluteString())
            .ok_or_else(|| UNAVAILABLE.to_string())?;
        Url::parse(&text.to_string()).map_err(|_| UNAVAILABLE.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use objc2::runtime::NSObject;
        use objc2::{ClassType, define_class};
        use objc2_foundation::NSString;

        // No AppKit/WebKit window is needed to exercise the nullable Objective-C getter.
        define_class!(
            #[unsafe(super = NSObject)]
            struct NoUrlWebview;
            impl NoUrlWebview {
                #[unsafe(method_id(URL))]
                fn url(&self) -> Option<Retained<NSURL>> { None }
            }
        );
        define_class!(
            #[unsafe(super = NSObject)]
            struct ReadyWebview;
            impl ReadyWebview {
                #[unsafe(method_id(URL))]
                fn url(&self) -> Option<Retained<NSURL>> {
                    NSURL::URLWithString(&NSString::from_str("https://accounts.booth.pm/library?page=2"))
                }
            }
        );

        #[test]
        fn nil_native_url_returns_an_error_instead_of_panicking() {
            let view: Retained<NoUrlWebview> =
                unsafe { objc2::msg_send![NoUrlWebview::class(), new] };
            assert!(unsafe { read_url(&view) }.is_err());
        }

        #[test]
        fn ready_native_url_preserves_the_actual_location() {
            let view: Retained<ReadyWebview> =
                unsafe { objc2::msg_send![ReadyWebview::class(), new] };
            assert_eq!(
                unsafe { read_url(&view) }.unwrap().as_str(),
                "https://accounts.booth.pm/library?page=2"
            );
        }
    }
}
