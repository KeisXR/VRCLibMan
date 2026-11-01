//! Force a dark appearance on in-app browser pages.
//!
//! WebView2's `WebContentsForceDark` flag is fixed when the browser environment
//! starts, applies to every WebView that shares it (including the hidden account
//! probe), and needs a restart to toggle. Chromium's algorithmic dark mode also
//! does not promise that photos stay their original colors.
//!
//! The same CSS is used on Windows, macOS, and Linux instead. `html` gets
//! `invert(1)`, so a light background becomes dark and dark text becomes light.
//! `img`, `video`, `canvas`, and the other replaced image-like elements get
//! `invert(1)` again. Invert is its own inverse, so those pixels come back
//! exactly. `hue-rotate(180deg)` is not used: after a channel clips to 0 or 255
//! it does not undo, and saturated colors (a blue canvas, a red photo) shift.
//! The sheet is attached with `adoptedStyleSheets` so the page DOM stays
//! unchanged; a `style` element is only the fallback.
//!
//! The hidden account probe, the toolbar, and the main window never receive it.
//! Cookies and page storage are not touched.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Applied to `html`. Light backgrounds become dark and dark text becomes light.
pub const PAGE_FILTER: &str = "invert(1)";
/// Applied again to image-like elements. `invert(1)` twice is the original color.
pub const MEDIA_FILTER: &str = "invert(1)";

/// User CSS. Image-like elements repeat [`PAGE_FILTER`], which restores their pixels.
pub const FORCE_DARK_CSS: &str = "html{background-color:#fff!important;filter:invert(1)!important;}img,video,canvas,embed,object,input[type='image'],svg image{filter:invert(1)!important;}";

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// Page webviews only. The probe is `booth-probe` and toolbars are `booth-toolbar*`.
pub fn is_page_webview(label: &str) -> bool {
    match label.strip_prefix("booth-tab-") {
        Some(rest) => !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()),
        None => false,
    }
}

/// Document-start script. `enabled` is the setting at the moment the string is built.
/// A later load evaluates a freshly built string so a toggle does not need a restart.
pub fn force_dark_script(enabled: bool) -> String {
    let css = serde_json::to_string(FORCE_DARK_CSS).unwrap_or_else(|_| "\"\"".to_string());
    let flag = if enabled { "true" } else { "false" };
    format!(
        r#"(function(){{var enabled={flag};var STYLE_ID="vrclibman-force-dark";var MARK="__vrclibmanForceDarkSheet";var CSS={css};function apply(on){{try{{var doc=document;var root=doc.documentElement;if(!root)return false;var current=root[MARK]||null;var fallback=doc.getElementById(STYLE_ID);if(!on){{if(current&&doc.adoptedStyleSheets){{var kept=[];var list=doc.adoptedStyleSheets;for(var i=0;i<list.length;i++)if(list[i]!==current)kept.push(list[i]);doc.adoptedStyleSheets=kept;}}try{{delete root[MARK];}}catch(ignore){{root[MARK]=null;}}if(fallback&&fallback.getAttribute("data-vrclibman")==="force-dark"&&fallback.parentNode)fallback.parentNode.removeChild(fallback);return true;}}if(current||(fallback&&fallback.getAttribute("data-vrclibman")==="force-dark"))return true;try{{if(typeof CSSStyleSheet==="function"&&doc.adoptedStyleSheets&&CSSStyleSheet.prototype.replaceSync){{var sheet=new CSSStyleSheet();sheet.replaceSync(CSS);var next=[];var existing=doc.adoptedStyleSheets;for(var j=0;j<existing.length;j++)next.push(existing[j]);next.push(sheet);doc.adoptedStyleSheets=next;root[MARK]=sheet;return true;}}}}catch(ignore2){{}}var style=doc.createElement("style");style.id=STYLE_ID;style.setAttribute("data-vrclibman","force-dark");style.textContent=CSS;(doc.head||root).appendChild(style);return true;}}catch(ignore3){{return false;}}}}apply(enabled);}})();"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_darkens_the_page_and_restores_images_with_a_second_invert() {
        assert_eq!(PAGE_FILTER, "invert(1)");
        assert_eq!(MEDIA_FILTER, PAGE_FILTER);
        assert_eq!(
            FORCE_DARK_CSS.matches("filter:invert(1)!important").count(),
            2
        );
        assert!(!FORCE_DARK_CSS.contains("hue-rotate"));
        assert!(FORCE_DARK_CSS.starts_with("html{"));
        assert!(FORCE_DARK_CSS.contains("background-color:#fff"));
        assert!(
            FORCE_DARK_CSS.contains("img,video,canvas,embed,object,input[type='image'],svg image")
        );
        assert!(!FORCE_DARK_CSS.contains("iframe"));
        assert!(!FORCE_DARK_CSS.contains("picture"));
        assert!(!FORCE_DARK_CSS.contains("*{"));
    }

    #[test]
    fn script_uses_adopted_stylesheets_and_leaves_storage_alone() {
        let on = force_dark_script(true);
        let off = force_dark_script(false);
        assert!(on.contains("var enabled=true"));
        assert!(off.contains("var enabled=false"));
        assert!(!off.contains("var enabled=true"));
        for script in [&on, &off] {
            assert_eq!(script.matches("filter:invert(1)!important").count(), 2);
            assert!(!script.contains("hue-rotate"));
            assert!(script.contains("adoptedStyleSheets"));
            assert!(script.contains("replaceSync"));
            assert!(script.contains("createElement(\"style\")"));
            assert!(script.contains("vrclibman-force-dark"));
            let lower = script.to_ascii_lowercase();
            assert!(!lower.contains("cookie"));
            assert!(!lower.contains("localstorage"));
            assert!(!lower.contains("sessionstorage"));
            assert!(!lower.contains("indexeddb"));
        }
    }

    #[test]
    fn only_numbered_page_tabs_are_darkened() {
        assert!(is_page_webview("booth-tab-1"));
        assert!(is_page_webview("booth-tab-20"));
        assert!(!is_page_webview("booth-tab-"));
        assert!(!is_page_webview("booth-tab-1a"));
        assert!(!is_page_webview("booth-probe"));
        assert!(!is_page_webview("booth-toolbar"));
        assert!(!is_page_webview("booth-toolbar-1"));
        assert!(!is_page_webview("booth-win-1"));
        assert!(!is_page_webview("main"));
    }

    #[test]
    fn display_pages_install_the_script_and_the_probe_does_not() {
        let surface = include_str!("browser_surface.rs");
        let mount = function_body(surface, "fn mount_tab");
        assert!(mount.contains("initialization_script_for_all_frames"));
        assert!(mount.contains("browser_dark::force_dark_script"));
        assert!(mount.contains("browser_dark::enabled()"));
        let finished = mount
            .find("読み終わり")
            .expect("page load distinguishes the finished event");
        assert!(mount[finished..].contains("browser_dark::force_dark_script"));
        let sync = function_body(surface, "pub fn sync_force_dark");
        assert!(sync.contains("is_page_webview"));
        assert!(sync.contains(".eval("));
        assert!(!sync.contains("booth-probe"));

        let browser = include_str!("browser.rs");
        let probe = function_body(browser, "fn open_profile_probe");
        assert!(!probe.contains("force_dark"));
        assert!(!probe.contains("browser_dark"));
        assert!(!probe.contains("adoptedStyleSheets"));

        let commands = include_str!("commands.rs");
        assert!(function_body(commands, "pub fn update_settings").contains("sync_force_dark"));
        assert!(function_body(commands, "pub fn save_settings").contains("sync_force_dark"));
        let startup = include_str!("lib.rs");
        assert!(startup.contains("browser_dark::set_enabled"));
    }

    fn function_body<'a>(source: &'a str, name: &str) -> &'a str {
        let start = source
            .find(name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let rest = &source[start..];
        let after = rest.find('\n').unwrap() + 1;
        let tail = &rest[after..];
        let end = tail
            .find("\nfn ")
            .or_else(|| tail.find("\npub fn "))
            .or_else(|| tail.find("\npub(crate) fn "))
            .or_else(|| tail.find("\nasync fn "))
            .or_else(|| tail.find("\npub async fn "))
            .unwrap_or(tail.len());
        &rest[..after + end]
    }
}
