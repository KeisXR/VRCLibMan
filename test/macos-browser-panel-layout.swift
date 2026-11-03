import AppKit
import WebKit

// Exercise the native view hierarchy used by Tauri's macOS child webviews.
// TitleBarStyle::Visible enables .fullSizeContentView in tauri-runtime-wry.
final class PanelProbe: NSObject, WKNavigationDelegate {
    var window: NSWindow!
    var main: WKWebView!
    var page: WKWebView!

    func start() {
        window = NSWindow(contentRect: NSRect(x: 40, y: 40, width: 1180, height: 800),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                          backing: .buffered, defer: false)
        window.title = "VRCLibMan native panel test"
        window.titlebarAppearsTransparent = false
        let parent = NSView(frame: window.contentView!.frame)
        window.contentView = parent
        main = WKWebView(frame: parent.bounds)
        main.navigationDelegate = self
        parent.addSubview(main)
        page = WKWebView(frame: NSRect(x: 0, y: 0, width: 1, height: 1))
        parent.addSubview(page)
        page.isHidden = true
        let css = try! String(contentsOfFile: "src/styles.css", encoding: .utf8)
        main.loadHTMLString("""
        <!doctype html><style>\(css)</style>
        <div class="browser-panel" style="height:100dvh;width:400px;margin-left:auto">
          <div class="browser-chrome embedded panel">
            <div class="browser-navigation">Address</div>
            <div class="browser-tools"><div class="browser-bookmarks"><button id="bookmark">Bookmark</button></div></div>
          </div>
          <div id="panel" class="browser-panel-viewport"></div>
        </div>
        """, baseURL: nil)
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
            fputs("Native macOS panel test timed out\n", stderr)
            exit(1)
        }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
            self.measure()
        }
    }

    func measure() {
        main.evaluateJavaScript("""
        (() => {const r = document.getElementById('panel').getBoundingClientRect();
        const b = document.getElementById('bookmark').getBoundingClientRect();
        return {innerWidth, innerHeight, top:r.top, left:r.left, width:r.width, height:r.height, bookmarkTop:b.top, bookmarkBottom:b.bottom};})()
        """) { value, error in
            guard let metrics = value as? [String: Any] else {
                fputs("Missing DOM geometry: \(String(describing: error))\n", stderr)
                exit(1)
            }
            let parent = self.page.superview!
            let number = { (key: String) -> CGFloat in (metrics[key] as! NSNumber).doubleValue }
            let height = floor(number("height"))
            // The currently shipped wry::set_bounds conversion.
            self.page.frame = NSRect(x: ceil(number("left")),
                                     y: parent.frame.height - ceil(number("top")) - height,
                                     width: floor(number("width")), height: height)
            print("DOM: \(metrics)")
            print("native parent.frame=\(parent.frame) main.frame=\(self.main.frame) main.bounds=\(self.main.bounds)")
            print("native layout=\(self.window.contentLayoutRect) main.safeArea=\(self.main.safeAreaInsets) parent.safeArea=\(parent.safeAreaInsets)")
            print("native page.frame=\(self.page.frame)")
            exit(0)
        }
    }
}

let application = NSApplication.shared
application.setActivationPolicy(.regular)
let probe = PanelProbe()
probe.start()
application.run()
