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
            <div class="browser-tools"><div class="browser-bookmarks"><button id="bookmark" onclick="window.bookmarkClicks=(window.bookmarkClicks||0)+1">Bookmark</button></div></div>
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
        return {innerWidth, innerHeight, top:r.top, left:r.left, width:r.width, height:r.height, bookmarkLeft:b.left, bookmarkRight:b.right, bookmarkTop:b.top, bookmarkBottom:b.bottom};})()
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
            self.checkBookmark(metrics: metrics, candidates: [
                self.window.contentLayoutRect.maxY,
                parent.frame.height
            ], index: 0)
        }
    }
    func click(_ point: NSPoint) {
        let location = page.superview!.convert(point, to: nil)
        for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
            let event = NSEvent.mouseEvent(with: type, location: location, modifierFlags: [],
                timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                context: nil, eventNumber: 0, clickCount: 1, pressure: 1)!
            window.sendEvent(event)
        }
    }

    func checkBookmark(metrics: [String: Any], candidates: [CGFloat], index: Int) {
        guard index < candidates.count else {
            fputs("Could not locate HTML bookmark with native pointer input\n", stderr)
            exit(1)
        }
        let number = { (key: String) -> CGFloat in (metrics[key] as! NSNumber).doubleValue }
        let x = (number("bookmarkLeft") + number("bookmarkRight")) / 2
        let center = (number("bookmarkTop") + number("bookmarkBottom")) / 2
        let point = NSPoint(x: x, y: candidates[index] - center)
        page.isHidden = true
        click(point)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
            self.main.evaluateJavaScript("window.bookmarkClicks||0") { value, _ in
                guard (value as? NSNumber)?.intValue == 1 else {
                    self.checkBookmark(metrics: metrics, candidates: candidates, index: index + 1)
                    return
                }
                print("Bookmark is clickable without native page at \(point), reference top=\(candidates[index])")
                self.page.isHidden = false
                self.click(point)
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) {
                    self.main.evaluateJavaScript("window.bookmarkClicks||0") { value, _ in
                        let clicks = (value as? NSNumber)?.intValue ?? 0
                        print("Bookmark clicks after showing native page: \(clicks)")
                        // Diagnostic stage: record the original native placement before fixing it.
                        exit(0)
                    }
                }
            }
        }
    }

}

let application = NSApplication.shared
application.setActivationPolicy(.regular)
let probe = PanelProbe()
probe.start()
application.run()
