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
    assert.deepEqual(kinds(parsed), ["files", "preview", "browser"]);
    assert.equal(active(parsed), "browser");
    const stored = panel.serializeRightPanelTabs(parsed);
    assert.equal(stored, '{"tabs":["files","preview","browser"],"active":2}');
    assert.doesNotMatch(stored, /booth\.pm|https?:/);

    const empty = panel.parseRightPanelTabs('{"tabs":[],"active":3}');
    assert.deepEqual(kinds(empty), []);
    assert.equal(empty.activeId, null);
    assert.equal(panel.serializeRightPanelTabs(empty), '{"tabs":[],"active":0}');

    // Kinds that later changes implement stay stored but hidden in this build.
    assert.equal(panel.RIGHT_PANEL_KIND_INFO.preview.available, true);
    assert.deepEqual(panel.visibleTabs(parsed).map((tab) => tab.kind), ["preview"]);
    assert.equal(panel.activeTab(parsed).kind, "preview");
    assert.equal(panel.canAddTab(parsed, "files"), panel.RIGHT_PANEL_KIND_INFO.files.available);

    // The preview tab is single; opening it again focuses it.
    let state = panel.openTab(empty, "preview");
    assert.deepEqual(kinds(state), ["preview"]);
    const again = panel.openTab(state, "preview");
    assert.deepEqual(kinds(again), ["preview"]);
    assert.equal(again.activeId, state.activeId);

    state = { tabs: [...parsed.tabs], activeId: parsed.tabs[1].id };
    const moved = panel.moveTab(state, state.tabs[1].id, 1);
    assert.deepEqual(kinds(moved), ["files", "browser", "preview"]);
    assert.equal(panel.moveTab(moved, moved.tabs[2].id, 1), moved);
    const closed = panel.closeTab(moved, moved.tabs[2].id);
    assert.deepEqual(kinds(closed), ["files", "browser"]);
    assert.equal(active(closed), "browser");
    const closedMiddle = panel.closeTab(state, state.tabs[1].id);
    assert.equal(active(closedMiddle), "browser");
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
  assert.match(app, /useRightPanelState\(fail\)/);
  assert.match(layout, /panelDocked/);
  assert.doesNotMatch(browser, /split with-side|ResizeSeparator|PreviewPane/);
  assert.match(browser, /panel\.togglePreview/);
  assert.match(browser, /aria-controls="right-panel"/);
  const registry = readFileSync("src/components/rightPanelRegistry.tsx", "utf8");
  assert.match(registry, /preview: PreviewTab/);
});
