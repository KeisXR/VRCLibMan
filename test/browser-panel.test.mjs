import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

const rect = (left, top, width, height) => ({ left, top, width, height });

test("browser panel geometry: rounded rectangles, covers, drags and the narrow overlay", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const panel = await server.ssrLoadModule("/src/browserPanel.ts");
    assert.deepEqual(panel.panelRect(rect(820.4, 95.6, 419.8, 600.2)), rect(821, 96, 419, 599));
    // The native page must never extend into the controls, even by a fraction
    // of a logical point (macOS truncates native child bounds to whole points).
    for (const top of [95, 95.01, 95.25, 95.49, 95.5, 95.99]) {
      const viewport = rect(820.25, top, 419.5, 600.5);
      const area = panel.panelRect(viewport);
      assert.ok(area.top >= viewport.top, "page starts below the controls");
      assert.ok(area.left >= viewport.left);
      assert.ok(area.left + area.width <= viewport.left + viewport.width);
      assert.ok(area.top + area.height <= viewport.top + viewport.height);
    }
    assert.equal(panel.panelRect(rect(0.1, 0.1, 2, 2)), null, "hide when fewer than two whole pixels fit");
    assert.equal(panel.panelRect(rect(0, 0, 1, 400)), null);
    assert.equal(panel.panelRect(rect(Number.NaN, 0, 400, 400)), null);

    assert.equal(panel.rectsOverlap(rect(0, 0, 10, 10), rect(5, 5, 10, 10)), true);
    assert.equal(panel.rectsOverlap(rect(0, 0, 10, 10), rect(10, 0, 10, 10)), false, "touching edges");
    assert.equal(panel.rectsOverlap(rect(0, 0, 10, 10), rect(2, 2, 0, 5)), false, "empty cover");

    const area = rect(800, 100, 400, 600);
    const calm = { overlay: false, dragging: false, covers: [] };
    assert.equal(panel.browserPanelVisible(area, calm), true);
    assert.equal(panel.browserPanelVisible(null, calm), false);
    assert.equal(panel.browserPanelVisible(area, { ...calm, overlay: true }), false);
    assert.equal(panel.browserPanelVisible(area, { ...calm, dragging: true }), false);
    // A menu over the panel hides the native view; one elsewhere in the window does not.
    assert.equal(panel.browserPanelVisible(area, { ...calm, covers: [rect(1000, 120, 168, 120)] }), false);
    assert.equal(panel.browserPanelVisible(area, { ...calm, covers: [rect(20, 600, 200, 120)] }), true);

    assert.equal(panel.sameReport(null, { area, visible: true }), false);
    assert.equal(panel.sameReport({ area: { ...area }, visible: true }, { area, visible: true }), true);
    assert.equal(panel.sameReport({ area, visible: true }, { area, visible: false }), false);
    assert.equal(panel.sameReport({ area: null, visible: false }, { area: null, visible: false }), true);

    const viewport = { id: "viewport" };
    const layer = (box, holdsViewport = false) => ({
      contains: (node) => holdsViewport && node === viewport,
      getBoundingClientRect: () => box,
    });
    const root = {
      querySelectorAll: (selector) => {
        assert.equal(selector, panel.BROWSER_PANEL_COVER_SELECTOR);
        return [layer(rect(0, 0, 1400, 900), true), layer(rect(900, 40, 160, 90)), layer(rect(0, 0, 0, 0))];
      },
    };
    // The overlay that holds the panel itself and invisible layers are not covers.
    assert.deepEqual(panel.coveringRects(root, viewport), [rect(900, 40, 160, 90)]);
    for (const selector of ['[role="menu"]', '[role="dialog"]', '[aria-modal="true"]', ".account-popover", ".preview-overlay"]) {
      assert.ok(panel.BROWSER_PANEL_COVER_SELECTORS.includes(selector), selector);
    }
  } finally {
    await server.close();
  }
});

test("the right panel's browser tab docks, hides under menus and drags, and moves to its own window", async (t) => {
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/");
  window.HTMLInputElement.prototype.select = () => {};
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Element", "Event", "CustomEvent", "EventTarget", "MutationObserver"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  // The viewport reports a fixed place in the main window; everything else has no size.
  const boxes = new WeakMap();
  window.HTMLElement.prototype.getBoundingClientRect = function () {
    if (this.hasAttribute?.("data-browser-panel-viewport")) return rect(820, 180, 420, 560);
    return boxes.get(this) ?? rect(0, 0, 0, 0);
  };
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
  let root;
  try {
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const preview = await server.ssrLoadModule("/src/preview/browser.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { BrowserPanelTab } = await server.ssrLoadModule("/src/components/BrowserPanel.tsx");
    const wait = async (check, label = "") => {
      for (let i = 0; i < 300; i++) {
        if (await check()) return;
        await act(tick);
      }
      assert.fail(`Browser panel did not settle ${label}: ${document.body.textContent}`);
    };
    const button = (text) => [...document.querySelectorAll("button")].find((node) => node.textContent === text);
    const click = async (node) => {
      assert.ok(node && !node.disabled, "enabled control exists");
      await act(async () => {
        node.dispatchEvent(new window.Event("click", { bubbles: true }));
        await tick();
      });
    };
    const failures = [];
    const shell = {
      revision: 0,
      refresh: () => {},
      notify: () => {},
      fail: (message) => failures.push(message),
      builtinBrowser: true,
      refreshAccount: () => {},
      workInProgress: false,
    };
    const render = (overlay) =>
      act(async () => {
        root.render(createElement(ShellProvider, { value: shell }, createElement(BrowserPanelTab, { panel: {}, overlay })));
        await tick();
      });
    preview.resetPreviewBrowser();
    let requests = 0;
    const stopRequests = preview.onPreviewBrowserPanelRequested(() => {
      requests += 1;
    });
    root = createRoot(document.getElementById("root"));
    await render(false);

    await t.test("nothing opens until asked, then the pages are placed over the measured area", async () => {
      await wait(() => button("ブラウザを開く"), "start");
      assert.equal((await api.browserState()).open, false);
      await click(button("ブラウザを開く"));
      await wait(() => document.querySelector(".browser-panel-viewport"), "docked");
      const state = await api.browserState();
      assert.equal(state.host, "panel");
      assert.ok(requests >= 1, "the panel tab is asked to show");
      assert.ok(document.querySelector(".browser-chrome.panel"));
      assert.equal(document.querySelector(".browser-window-controls"), null);
      assert.ok(document.querySelector('[aria-label="別ウィンドウで開く"]'));
      await wait(() => preview.previewBrowserPanelArea().visible, "visible");
      assert.deepEqual(preview.previewBrowserPanelArea().area, rect(820, 180, 420, 560));
    });

    await t.test("a menu over the panel hides the pages until it closes", async () => {
      const menu = document.createElement("div");
      menu.setAttribute("role", "menu");
      boxes.set(menu, rect(1000, 200, 168, 120));
      await act(async () => {
        document.body.appendChild(menu);
        await tick();
      });
      await wait(() => !preview.previewBrowserPanelArea().visible, "hidden by menu");
      await act(async () => {
        menu.remove();
        await tick();
      });
      await wait(() => preview.previewBrowserPanelArea().visible, "menu closed");
      const far = document.createElement("div");
      far.setAttribute("role", "dialog");
      boxes.set(far, rect(10, 10, 200, 100));
      await act(async () => {
        document.body.appendChild(far);
        await tick();
      });
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 60));
      });
      assert.equal(preview.previewBrowserPanelArea().visible, true, "a dialog elsewhere keeps the pages");
      far.remove();
    });

    await t.test("dragging a pane edge hides the pages until the pointer is released", async () => {
      const separator = document.createElement("div");
      separator.className = "resize-separator";
      document.body.appendChild(separator);
      await act(async () => {
        separator.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
        await tick();
      });
      await wait(() => !preview.previewBrowserPanelArea().visible, "hidden while dragging");
      await act(async () => {
        separator.dispatchEvent(new window.Event("pointerup", { bubbles: true }));
        await tick();
      });
      await wait(() => preview.previewBrowserPanelArea().visible, "drag ended");
      separator.remove();
    });

    await t.test("a narrow window hides the pages and offers the separate window", async () => {
      await render(true);
      await wait(() => button("別ウィンドウで開く"), "overlay message");
      assert.equal(document.querySelector(".browser-panel-viewport"), null);
      assert.deepEqual(preview.previewBrowserPanelArea(), { area: null, visible: false });
      await click(button("別ウィンドウで開く"));
      await wait(async () => (await api.browserState()).host === "window", "undocked");
      await render(false);
      await wait(() => button("パネルに戻す"), "window placeholder");
      assert.ok(button("ウィンドウを前面に"));
      // A new session now starts in the window, as the user chose.
      await act(async () => {
        await api.closeBoothBrowser();
        await api.openBoothBrowser();
        await tick();
      });
      assert.equal((await api.browserState()).host, "window");
      await click(button("パネルに戻す"));
      await wait(() => document.querySelector(".browser-panel-viewport"), "docked again");
      await wait(() => preview.previewBrowserPanelArea().visible, "visible again");
    });

    await t.test("leaving the tab hides the pages, and no page address is kept by the panel", async () => {
      await act(async () => {
        root.render(createElement("div"));
        await tick();
      });
      assert.deepEqual(preview.previewBrowserPanelArea(), { area: null, visible: false });
      assert.equal((await api.browserState()).host, "panel", "hiding the tab keeps the session");
      assert.deepEqual(failures, []);
    });
    stopRequests();
  } finally {
    await act(async () => root?.unmount());
    await server.close();
  }
});

test("the docked browser keeps BOOTH addresses out of settings and follows the browser setting", () => {
  const panel = readFileSync("src/components/RightPanel.tsx", "utf8");
  const browserPanel = readFileSync("src/components/BrowserPanel.tsx", "utf8");
  const kinds = readFileSync("src/rightPanel.ts", "utf8");
  const model = readFileSync("src-tauri/src/model.rs", "utf8");
  // Only kinds and an index are saved; the browser tab never writes a page address.
  assert.doesNotMatch(browserPanel, /updateSettings|localStorage/);
  assert.match(kinds, /pageTabsAllowed/);
  assert.match(model, /RIGHT_PANEL_SINGLETONS: &\[&str\] = &\["preview", "files"\]/);
  assert.match(panel, /listenBrowserPanelRequested/);
  assert.match(panel, /if \(!builtinBrowser\)/);
  // Docked pages are children of the main window. The main window's capability stays
  // local-only (no remote URLs), and browser commands check the caller's webview label.
  const capability = JSON.parse(readFileSync("src-tauri/capabilities/default.json", "utf8"));
  assert.equal(capability.remote, undefined);
  const toolbar = JSON.parse(readFileSync("src-tauri/capabilities/browser-toolbar.json", "utf8"));
  assert.ok(toolbar.webviews.includes("booth-toolbar-*"));
  const commands = readFileSync("src-tauri/src/commands.rs", "utf8");
  assert.match(commands, /BrowserAction::PanelArea \{ area, visible \} => \{\s*main_window_only\(&label\)\?;/);
  // Closing the panel hides docked pages. It does not end their session.
  assert.doesNotMatch(panel, /closeBoothBrowser/);
  assert.match(panel, /unified/);
  assert.match(browserPanel, /この環境では、ブラウザは別ウィンドウで開きます/);
  assert.match(browserPanel, /kind: "panelArea", area: null, visible: false/);
  assert.doesNotMatch(browserPanel, /closeTab|closeBoothBrowser/);
  // A press under 6px stays a click. Pointer capture starts only after the drag threshold.
  assert.match(readFileSync("src/tabStrip.ts", "utf8"), /TAB_DRAG_THRESHOLD_PX = 6/);
  assert.match(
    panel,
    /< TAB_DRAG_THRESHOLD_PX\) return;\s*if \(!current\.moved\) capture\(event\.pointerId\)/,
  );
});

test("stored browser tabs are fixed kinds only, and old browser entries are dropped", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const panel = await server.ssrLoadModule("/src/rightPanel.ts");
    const parsed = panel.parseRightPanelTabs('{"tabs":["browser","preview","browser"],"active":2}');
    assert.deepEqual(parsed.tabs.map((tab) => tab.kind), ["preview"]);
    assert.equal(parsed.tabs.find((tab) => tab.id === parsed.activeId).kind, "preview");
    assert.equal(panel.serializeRightPanelTabs(parsed), '{"tabs":["preview"],"active":0}');
    const again = panel.openTab(parsed, "preview");
    assert.equal(again.tabs.length, 1);
  } finally {
    await server.close();
  }
});

test("one page can leave for its own window while the panel keeps the rest", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const preview = await server.ssrLoadModule("/src/preview/browser.ts");
    preview.resetPreviewBrowser();
    preview.previewBrowserAction({ kind: "newTab", url: "https://one.example/a" });
    preview.previewBrowserAction({ kind: "newTab", url: "https://two.example/b" });
    const opened = preview.previewBrowserState();
    assert.equal(opened.host, "panel");
    assert.equal(opened.hosts.length, 1);
    const [first, second] = opened.hosts[0].tabIds;
    const detached = preview.previewBrowserAction({ kind: "detachTab", tabId: first, x: -1920, y: 40 });
    const panelHost = detached.hosts.find((host) => host.kind === "panel");
    const windowHost = detached.hosts.find((host) => host.kind === "window");
    assert.deepEqual(panelHost.tabIds, [second]);
    assert.deepEqual(windowHost.tabIds, [first]);
    assert.equal(windowHost.activeId, first);
    assert.equal(detached.tabs.find((tab) => tab.id === second).url, "https://two.example/b");
    const named = preview.previewBrowserAction({ kind: "navigate", url: "https://two.example/next", tabId: second, hostId: "panel" });
    assert.equal(named.tabs.find((tab) => tab.id === second).url, "https://two.example/next");
    assert.equal(named.tabs.find((tab) => tab.id === first).url, "https://one.example/a");
    const docked = preview.previewBrowserAction({ kind: "dockTab", tabId: first });
    assert.equal(docked.hosts.some((host) => host.kind === "window"), false);
    assert.equal(docked.host, "panel");
    const order = docked.hosts[0].tabIds;
    const reversed = preview.previewBrowserAction({ kind: "reorderTabs", hostId: "panel", tabIds: [...order].reverse() });
    assert.deepEqual(reversed.hosts[0].tabIds, [...order].reverse());
    const popped = preview.previewBrowserAction({ kind: "detachTab", tabId: order[0], x: 12, y: 12 });
    const alone = popped.hosts.find((host) => host.kind === "window");
    const closed = preview.previewBrowserAction({ kind: "closeTab", tabId: alone.tabIds[0] });
    assert.equal(closed.hosts.some((host) => host.id === alone.id), false);
    assert.equal(closed.host, "panel");
    assert.throws(() => preview.previewBrowserAction({ kind: "detachTab", tabId: closed.hosts[0].tabIds[0], x: Number.NaN, y: 1 }));
    assert.throws(() => preview.previewBrowserAction({ kind: "reorderTabs", hostId: "panel", tabIds: ["missing"] }));
  } finally {
    await server.close();
  }
});
