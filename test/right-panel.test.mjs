import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("right panel tabs keep only known kinds and never store free text", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const panel = await server.ssrLoadModule("/src/rightPanel.ts");
    const kinds = (state) => state.tabs.map((tab) => tab.kind);
    const active = (state) => state.tabs.find((tab) => tab.id === state.activeId)?.kind ?? null;

    const fallback = panel.parseRightPanelTabs("not json");
    assert.deepEqual(kinds(fallback), ["preview"]);
    assert.equal(active(fallback), "preview");
    assert.deepEqual(kinds(panel.parseRightPanelTabs(null)), ["preview"]);

    const parsed = panel.parseRightPanelTabs(
      JSON.stringify({ tabs: ["files", "https://booth.pm/ja/items/1", "preview", "preview", "browser"], active: 4 }),
    );
    assert.deepEqual(kinds(parsed), ["files", "preview"]);
    assert.equal(active(parsed), "preview");
    const stored = panel.serializeRightPanelTabs(parsed);
    assert.equal(stored, '{"tabs":["files","preview"],"active":1}');
    assert.doesNotMatch(stored, /booth\.pm|https?:|browser/);

    const empty = panel.parseRightPanelTabs('{"tabs":[],"active":3}');
    assert.deepEqual(kinds(empty), []);
    assert.equal(empty.activeId, null);
    assert.equal(panel.serializeRightPanelTabs(empty), '{"tabs":[],"active":0}');

    assert.equal(panel.RIGHT_PANEL_KIND_INFO.preview.available, true);
    assert.equal(panel.RIGHT_PANEL_KIND_INFO.files.available, true);
    assert.equal(panel.pageTabsAllowed({}), false);
    assert.equal(panel.pageTabsAllowed({ builtinBrowser: true }), true);
    assert.deepEqual(panel.visibleTabs(parsed).map((tab) => tab.kind), ["files", "preview"]);
    assert.equal(panel.activeTab(parsed).kind, "preview");
    assert.equal(panel.canAddTab(parsed, "files"), true);

    const withPages = panel.focusWebTab(parsed, "booth-tab-1", "one.example");
    const placed = panel.placeTab(withPages, "web-booth-tab-1", 1);
    assert.deepEqual(kinds(placed), ["files", "web", "preview"]);
    const pageStored = panel.serializeRightPanelTabs(placed);
    assert.equal(pageStored, '{"tabs":["files","preview"],"active":0}');
    assert.doesNotMatch(pageStored, /booth-tab|example/);
    const merged = panel.mergePanelWebTabs(placed, [
      { tabId: "booth-tab-1", title: "one.example" },
      { tabId: "booth-tab-2", title: "two.example" },
    ]);
    assert.deepEqual(panel.webTabIds(merged), ["booth-tab-1", "booth-tab-2"]);
    assert.equal(panel.visibleTabs(merged, { builtinBrowser: true }).some((tab) => tab.kind === "web"), true);
    assert.equal(panel.visibleTabs(merged).some((tab) => tab.kind === "web"), false);
    assert.deepEqual(panel.webTabIds(panel.mergePanelWebTabs(merged, [])), []);

    const strip = await server.ssrLoadModule("/src/tabStrip.ts");
    assert.equal(strip.dragOutcome({ moved: false, cancelled: false, insideWindow: false, detachable: true }), "click");
    assert.equal(strip.dragOutcome({ moved: true, cancelled: true, insideWindow: false, detachable: true }), "cancel");
    assert.equal(strip.dragOutcome({ moved: true, cancelled: false, insideWindow: true, detachable: true }), "reorder");
    assert.equal(strip.dragOutcome({ moved: true, cancelled: false, insideWindow: false, detachable: false }), "cancel");
    assert.equal(strip.dragOutcome({ moved: true, cancelled: false, insideWindow: false, detachable: true }), "detach");
    assert.equal(strip.dragOutcome({ moved: true, cancelled: false, insideWindow: false, detachable: true, dragEndOnly: true }), "cancel");
    assert.equal(strip.pointerInsideWindow(-20, 10, 800, 600), false);
    assert.equal(strip.pointerInsideWindow(20, 10, 800, 600), true);
    assert.equal(strip.insertionIndex(5, [{ left: 0, width: 20 }, { left: 24, width: 20 }]), 0);
    assert.equal(strip.insertionIndex(30, [{ left: 0, width: 20 }, { left: 24, width: 20 }]), 1);
    assert.equal(strip.insertionIndex(40, [{ left: 0, width: 20 }, { left: 24, width: 20 }]), 2);

    // The preview tab is single; opening it again focuses it.
    let state = panel.openTab(empty, "preview");
    assert.deepEqual(kinds(state), ["preview"]);
    const again = panel.openTab(state, "preview");
    assert.deepEqual(kinds(again), ["preview"]);
    assert.equal(again.activeId, state.activeId);

    state = panel.focusWebTab({ tabs: [...parsed.tabs], activeId: parsed.tabs[1].id }, "booth-tab-9", "nine.example");
    const moved = panel.moveTab(state, state.tabs[1].id, 1);
    assert.deepEqual(kinds(moved), ["files", "web", "preview"]);
    assert.equal(panel.moveTab(moved, moved.tabs[2].id, 1), moved);
    const closed = panel.closeTab(moved, moved.tabs[1].id);
    assert.deepEqual(kinds(closed), ["files", "preview"]);
    assert.equal(active(closed), "preview");
    const closedMiddle = panel.closeTab(state, state.tabs[1].id);
    assert.equal(active(closedMiddle), "web");
    assert.equal(panel.selectTab(state, "missing"), state);

    const key = (over) => ({ key: "B", code: "KeyB", ctrlKey: true, metaKey: false, shiftKey: true, altKey: false, ...over });
    assert.equal(panel.isRightPanelShortcut(key({})), true);
    assert.equal(panel.isRightPanelShortcut(key({ ctrlKey: false, metaKey: true })), true);
    assert.equal(panel.isRightPanelShortcut(key({ shiftKey: false })), false);
    assert.equal(panel.isRightPanelShortcut(key({ altKey: true })), false);
    assert.equal(panel.isRightPanelShortcut(key({ ctrlKey: false })), false);
    assert.equal(panel.isRightPanelShortcut(key({ key: "T", code: "KeyT" })), false);

    // Docked width: half the window at most, and the list keeps its room.
    assert.equal(panel.rightPanelMaxWidth(1440, 238), 720);
    assert.equal(panel.rightPanelMaxWidth(1100, 444), 290);
    assert.equal(panel.rightPanelMaxWidth(1000, 444), 240);
    assert.equal(panel.fitRightPanelWidth(420, 1440, 238), 420);
    assert.equal(panel.fitRightPanelWidth(900, 1440, 238), 720);
    assert.equal(panel.fitRightPanelWidth(10, 1440, 238), 240);
    assert.equal(panel.fitRightPanelWidth(Number.NaN, 1440, 238), 300);
  } finally {
    await server.close();
  }
});

test("the preview lives in the app's right panel instead of a split inside the list", () => {
  const app = readFileSync("src/App.tsx", "utf8");
  const browser = readFileSync("src/components/GenreBrowser.tsx", "utf8");
  const layout = readFileSync("src/useAppLayout.tsx", "utf8");
  assert.match(app, /<RightPanelColumn/);
  assert.match(app, /useRightPanelState\(fail, \{ builtinBrowser \}\)/);
  assert.match(layout, /panelDocked/);
  assert.doesNotMatch(browser, /split with-side|ResizeSeparator|PreviewPane/);
  assert.match(browser, /panel\.togglePreview/);
  assert.match(browser, /aria-controls="right-panel"/);
  const registry = readFileSync("src/components/rightPanelRegistry.tsx", "utf8");
  assert.match(registry, /preview: PreviewTab/);
  assert.match(registry, /files: FilesTab/);
  assert.doesNotMatch(registry, /browser: BrowserPanelTab/);
  assert.match(readFileSync("src/components/RightPanel.tsx", "utf8"), /unified/);
});


test("panel settings load independently of browser synchronization", async (t) => {
  const { parseHTML } = await import("linkedom");
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Element", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement, StrictMode } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
  try {
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const preview = await server.ssrLoadModule("/src/preview/browser.ts");
    const { useRightPanelState } = await server.ssrLoadModule("/src/components/RightPanel.tsx");
    const settings = { ...(await api.settings()), previewPane: true, rightPanelTabs: '{"tabs":["files","preview"],"active":0}' };
    const kinds = (panel) => panel.tabs.tabs.map((tab) => tab.kind);

    for (const scenario of ["browser disabled", "page opened before settings", "user edits before settings"]) {
      await t.test(scenario, async () => {
        preview.resetPreviewBrowser();
        let resolveSettings;
        const pendingSettings = new Promise((resolve) => { resolveSettings = resolve; });
        api.settings = () => pendingSettings;
        api.browserState = async () => preview.previewBrowserState();
        const writes = [];
        api.updateSettings = async (patch) => { writes.push(patch); return { ...settings, ...patch }; };
        const failures = [];
        let panel;
        function Harness() {
          panel = useRightPanelState((message) => failures.push(message), { builtinBrowser: scenario === "page opened before settings" });
          return null;
        }
        const root = createRoot(document.getElementById("root"));
        try {
          await act(async () => { root.render(createElement(StrictMode, null, createElement(Harness))); await tick(); });
          let pageId;
          if (scenario === "page opened before settings") {
            await act(async () => {
              pageId = preview.previewBrowserAction({ kind: "newTab", hostId: "panel", url: "https://example.com/" }).activeId;
              await tick();
            });
            assert.equal(panel.active?.tabId, pageId);
          } else if (scenario === "user edits before settings") {
            await act(async () => { panel.openKind("files"); await tick(); });
          }
          await act(async () => { resolveSettings(settings); await tick(); });

          if (scenario === "user edits before settings") {
            assert.deepEqual(kinds(panel), ["preview", "files"], "late settings must not overwrite user edits");
            assert.equal(panel.active?.kind, "files");
          } else {
            assert.deepEqual(kinds(panel).filter((kind) => kind !== "web"), ["files", "preview"]);
            assert.equal(writes.some((patch) => "rightPanelTabs" in patch), false, "browser synchronization must not save tab settings");
            if (pageId) assert.equal(panel.active?.tabId, pageId, "loading fixed tabs must keep the opened page selected");
            else assert.equal(panel.active?.kind, "files");
          }
          assert.deepEqual(failures, []);
        } finally {
          await act(async () => root.unmount());
        }
      });
    }
  } finally {
    await server.close();
  }
});

test("narrow panels keep tab controls usable before a product is selected", async () => {
  const { parseHTML } = await import("linkedom");
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Element", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { RightPanelColumn, useRightPanelState } = await server.ssrLoadModule("/src/components/RightPanel.tsx");
    const { RIGHT_PANEL_KIND_INFO } = await server.ssrLoadModule("/src/rightPanel.ts");
    const settings = { ...(await api.settings()), previewPane: true, rightPanelTabs: '{"tabs":["preview"],"active":0}' };
    api.settings = async () => settings;
    api.updateSettings = async (patch) => Object.assign(settings, patch);
    api.listDirectory = async () => ({ path: "C:\\Lib", parent: "C:\\", library: "C:\\Lib", remote: null, insideLibrary: true, entries: [], truncated: false });
    const failures = [];
    const shell = { revision: 0, refresh() {}, notify() {}, fail: (message) => failures.push(message), builtinBrowser: true, refreshAccount() {}, workInProgress: false };
    function Harness() {
      const panel = useRightPanelState(shell.fail, { builtinBrowser: true });
      return createElement(ShellProvider, { value: shell }, createElement(RightPanelColumn, { panel, docked: false, width: 0, maxWidth: 240 }));
    }
    const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
    const wait = async (check) => {
      for (let n = 0; n < 100; n++) { if (check()) return; await act(tick); }
      assert.fail(`Panel did not settle: ${document.body.textContent}`);
    };
    const click = (node) => act(async () => { assert.ok(node && !node.disabled); node.dispatchEvent(new window.Event("click", { bubbles: true })); await tick(); });
    root = createRoot(document.getElementById("root"));
    await act(async () => { root.render(createElement(Harness)); await tick(); });
    await wait(() => document.querySelector('[aria-label="タブを追加"]'));
    assert.ok(document.querySelector(".right-panel-overlay.modeless"));
    assert.equal(document.querySelector('[aria-modal="true"]'), null, "the product list stays available");
    await click(document.querySelector('[aria-label="タブを追加"]'));
    const kind = RIGHT_PANEL_KIND_INFO.files.available ? "files" : "browser";
    const label = RIGHT_PANEL_KIND_INFO[kind].label;
    const menuItem = [...document.querySelectorAll('[role="menuitem"]')].find((node) => node.textContent === label);
    await click(menuItem);
    await wait(() => document.querySelector('[role="tab"][aria-selected="true"]')?.textContent === label);
    assert.ok(document.querySelector('[role="dialog"][aria-modal="true"]'));
    await click(document.querySelector('[aria-label="右パネルを閉じる"]'));
    assert.equal(document.querySelector("#right-panel"), null);
    assert.equal(settings.previewPane, false);
    assert.deepEqual(failures, []);
  } finally {
    if (root) await act(async () => root.unmount());
    await server.close();
  }
});
