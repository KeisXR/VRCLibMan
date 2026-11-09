import AppKit
import WebKit

// The DOM uses WKWebView's unobscured viewport, whereas wry::set_bounds uses
// the whole parent NSView. Exercise real pointer input, not just DOM rectangles.
func require(_ condition: Bool, _ message: String) {
    if !condition {
        fputs("\(message)\n", stderr)
        exit(1)
    }
}

final class PanelProbe: NSObject, WKNavigationDelegate {
    var window: NSWindow!
    var main: WKWebView!
    var page: WKWebView!
    var detached: NSWindow?
    var step = 0
    let sizes: [NSSize] = [NSSize(width: 960, height: 600), NSSize(width: 920, height: 520), NSSize(width: 980, height: 620)]

    func start() {
        require(CommandLine.arguments.count == 2, "Pass the compiled macos-panel-frame helper")
        // TitleBarStyle::Visible in tauri-runtime-wry also sets FullSizeContentView.
        window = NSWindow(contentRect: NSRect(origin: NSPoint(x: 20, y: 20), size: sizes[0]),
                          styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                          backing: .buffered, defer: false)
        window.title = "VRCLibMan native panel test"
        window.titlebarAppearsTransparent = false
        let parent = NSView(frame: window.contentView!.frame)
        window.contentView = parent
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        main = WKWebView(frame: parent.bounds, configuration: configuration)
        main.autoresizingMask = [.width, .height]
        main.navigationDelegate = self
        parent.addSubview(main)
        page = WKWebView(frame: NSRect(x: 0, y: 0, width: 1, height: 1), configuration: configuration)
        parent.addSubview(page)
        page.isHidden = true
        page.loadHTMLString("""
        <!doctype html><style>body{margin:0;background:#e99}button{position:absolute;left:10px;top:10px;width:100px;height:50px}</style>
        <button onclick="window.pageClicks=(window.pageClicks||0)+1">Page</button>
        """, baseURL: nil)
        let css = try! String(contentsOfFile: "src/styles.css", encoding: .utf8)
        main.loadHTMLString("""
        <!doctype html><meta name="viewport" content="width=device-width, initial-scale=1.0"><style>\(css)</style>
        <div class="browser-panel" style="height:100dvh;width:400px;margin-left:auto">
          <div class="browser-chrome embedded panel">
            <div class="browser-navigation">Address</div>
            <div class="browser-tools"><div class="browser-bookmarks"><button id="bookmark" onclick="window.bookmarkClicks=(window.bookmarkClicks||0)+1">Bookmark</button></div></div>
          </div>
          <div id="panel" class="browser-panel-viewport"></div>
        </div>
        """, baseURL: nil)
        window.makeKeyAndOrderFront(nil)
        main.frame = parent.bounds
        NSApp.activate(ignoringOtherApps: true)
        DispatchQueue.main.asyncAfter(deadline: .now() + 45) {
            require(false, "Native macOS panel test timed out")
        }
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        later { self.measure() }
    }

    func later(_ body: @escaping () -> Void) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: body)
    }

    func evaluate(_ view: WKWebView, _ script: String, _ completion: @escaping (Any?) -> Void) {
        view.evaluateJavaScript(script) { value, error in
            require(error == nil, "JavaScript failed: \(String(describing: error))")
            completion(value)
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

    func productionFrame(area: NSRect, layout: NSRect, flipped: Bool) -> NSRect {
        // This executable imports the same Rust function used by set_panel_bounds.
        let process = Process()
        process.executableURL = URL(fileURLWithPath: CommandLine.arguments[1])
        process.arguments = [area.origin.x, area.origin.y, area.width, area.height,
                             layout.origin.x, layout.origin.y, layout.width, layout.height,
                             flipped ? 1 : 0].map { String(Double($0)) }
        let output = Pipe()
        process.standardOutput = output
        try! process.run()
        process.waitUntilExit()
        require(process.terminationStatus == 0, "Production frame helper failed")
        let values = String(data: output.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8)!
            .trimmingCharacters(in: .whitespacesAndNewlines).split(separator: ",").compactMap { Double($0) }
        require(values.count == 4, "Invalid production frame")
        return NSRect(x: values[0], y: values[1], width: values[2], height: values[3])
    }

    func measure() {
        page.isHidden = true
        evaluate(main, """
        (() => { window.bookmarkClicks=0; const r=document.getElementById('panel').getBoundingClientRect();
        const b=document.getElementById('bookmark').getBoundingClientRect();
        const left=Math.ceil(r.left),top=Math.ceil(r.top);
        return {innerWidth,innerHeight,left,top,width:Math.floor(r.right)-left,height:Math.floor(r.bottom)-top,
        bookmarkX:(b.left+b.right)/2,bookmarkY:(b.top+b.bottom)/2};})()
        """) { value in
            guard let metrics = value as? [String: Any] else { require(false, "Missing DOM geometry"); return }
            let number = { (key: String) -> CGFloat in (metrics[key] as! NSNumber).doubleValue }
            let parent = self.page.superview!
            let layout = parent.convert(self.window.contentLayoutRect, from: nil)
            let point = NSPoint(x: layout.minX + number("bookmarkX"), y: layout.maxY - number("bookmarkY"))
            let area = NSRect(x: number("left"), y: number("top"), width: number("width"), height: number("height"))
            let oldFrame = NSRect(x: area.minX, y: parent.frame.height - area.minY - area.height,
                                  width: area.width, height: area.height)
            let fixedFrame = self.productionFrame(area: area, layout: layout, flipped: parent.isFlipped)
            print("native case \(self.step): DOM=\(metrics), parent=\(parent.frame), layout=\(layout)")
            print("native case \(self.step): old=\(oldFrame), corrected=\(fixedFrame), scale=\(self.window.backingScaleFactor)")
            require(abs(fixedFrame.minY - layout.minY) <= 1, "Page does not fill the viewport to its bottom")
            self.click(point)
            self.later {
                self.evaluate(self.main, "window.bookmarkClicks||0") { value in
                    require((value as? NSNumber)?.intValue == 1, "Bookmark was not clickable without the page")
                    self.page.frame = oldFrame
                    self.page.isHidden = false
                    self.later {
                        self.click(point)
                        self.later {
                            self.evaluate(self.main, "window.bookmarkClicks||0") { value in
                                require((value as? NSNumber)?.intValue == 1, "Original placement did not reproduce the hidden bookmark")
                                self.page.frame = fixedFrame
                                self.page.clipsToBounds = true
                                self.later {
                                    self.click(point)
                                    self.later {
                                        self.evaluate(self.main, "window.bookmarkClicks||0") { value in
                                            require((value as? NSNumber)?.intValue == 2, "Corrected page still covers the bookmark")
                                            print("native case \(self.step): original hides bookmark, corrected frame receives the real click")
                                            self.checkPage()
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    func checkPage() {
        evaluate(page, "window.pageClicks=0") { _ in
            self.click(NSPoint(x: self.page.frame.minX + 35, y: self.page.frame.maxY - 35))
            self.later {
                self.evaluate(self.page, "window.pageClicks||0") { value in
                    require((value as? NSNumber)?.intValue == 1, "Corrected page does not receive pointer input")
                    self.next()
                }
            }
        }
    }

    func next() {
        step += 1
        if step < sizes.count {
            window.setContentSize(sizes[step])
            main.frame = window.contentView!.bounds
            later { self.measure() }
        } else if step == sizes.count {
            // Same native page leaves for an overlay-titlebar host and comes back.
            detached = NSWindow(contentRect: NSRect(x: 30, y: 30, width: 800, height: 500),
                                styleMask: [.titled, .resizable, .fullSizeContentView], backing: .buffered, defer: false)
            detached!.titlebarAppearsTransparent = true
            detached!.contentView!.addSubview(page)
            window.contentView!.addSubview(page)
            later { self.measure() }
        } else {
            print("native macOS panel: original overlap reproduced; bookmarks and page input pass at three sizes and after reparenting")
            exit(0)
        }
    }
}

let application = NSApplication.shared
application.setActivationPolicy(.regular)
let probe = PanelProbe()
probe.start()
application.run()
