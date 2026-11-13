// One shared, numbered copy of the built-in browser's state per window (src/browserStore.ts).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

const state = (revision, title = "page") => ({
  revision,
  open: true,
  tabs: [{ id: "booth-tab-1", url: "https://booth.pm/", title, loading: false, canGoBack: false, canGoForward: false, zoomFactor: 1, hostId: "panel" }],
  activeId: "booth-tab-1",
  hosts: [{ id: "panel", kind: "panel", tabIds: ["booth-tab-1"], activeId: "booth-tab-1" }],
  bookmarks: [],
  busy: false,
  host: "panel",
  panelSupported: true,
});

test("older states never replace newer ones, overlapping reads merge, and one timer polls", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  const realSetInterval = globalThis.setInterval;
  const realClearInterval = globalThis.clearInterval;
  const tick = () => new Promise((resolve) => setTimeout(resolve, 5));
  try {
    const store = await server.ssrLoadModule("/src/browserStore.ts");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const pending = [];
    api.browserState = () => new Promise((resolve, reject) => pending.push({ resolve, reject }));
    const intervals = [];
    globalThis.setInterval = (fn, ms) => {
      const id = { fn, ms };
      intervals.push(id);
      return id;
    };
    let cleared = 0;
    globalThis.clearInterval = () => {
      cleared += 1;
    };

    assert.equal(store.newerBrowserState(null, { revision: 3 }), true);
    assert.equal(store.newerBrowserState(5, { revision: 3 }), false);
    assert.equal(store.newerBrowserState(5, { revision: 5 }), false, "the same state is not applied twice");
    assert.equal(store.newerBrowserState(5, {}), true, "the browser preview has no numbers");

    let notified = 0;
    const stopA = store.subscribeBrowserState(() => { notified += 1; }, { poll: true });
    const stopB = store.subscribeBrowserState(() => {}, { poll: true });
    const stopC = store.subscribeBrowserState(() => {});
    assert.equal(pending.length, 1, "three subscribers start one read");
    assert.equal(intervals.length, 1, "pollers share one timer");
    assert.equal(intervals[0].ms, store.BROWSER_POLL_MS);

    // A command result arrives while the read is still out.
    store.applyBrowserState(state(10, "after the command"));
    assert.equal(store.getBrowserSnapshot().state.revision, 10);
    pending.shift().resolve(state(4, "read before the command"));
    await tick();
    assert.equal(store.getBrowserSnapshot().state.tabs[0].title, "after the command", "the slower, older read is dropped");
    assert.equal(pending.length, 1, "the merged second read runs after the first");
    pending.shift().resolve(state(12, "newest"));
    await tick();
    assert.equal(store.getBrowserSnapshot().state.tabs[0].title, "newest");
    assert.ok(notified >= 2);

    // A failed read keeps the last state and reports the error.
    store.refreshBrowserState();
    pending.shift().reject(new Error("応答がありません"));
    await tick();
    assert.equal(store.getBrowserSnapshot().state.revision, 12);
    assert.match(store.getBrowserSnapshot().error, /応答がありません/);

    stopA();
    assert.equal(cleared, 0, "another poller is still on screen");
    stopB();
    assert.equal(cleared, 1, "the timer stops with the last poller");
    stopC();
    assert.equal(store.getBrowserSnapshot().state, null, "an unwatched state is dropped, not shown stale later");
  } finally {
    globalThis.setInterval = realSetInterval;
    globalThis.clearInterval = realClearInterval;
    await server.close();
  }
});

test("browser controls share the store instead of reading and polling on their own", () => {
  const chrome = readFileSync("src/components/BrowserChrome.tsx", "utf8");
  const panel = readFileSync("src/components/BrowserPanel.tsx", "utf8");
  const rightPanel = readFileSync("src/components/RightPanel.tsx", "utf8");
  for (const [name, source] of [["BrowserChrome", chrome], ["BrowserPanel", panel], ["RightPanel", rightPanel]]) {
    assert.doesNotMatch(source, /listenBrowserChanged|api\.browserState\(/, `${name} uses src/browserStore.ts`);
  }
  assert.doesNotMatch(chrome, /setInterval/, "the controls do not poll on their own");
  assert.match(chrome, /useBrowserSnapshot\(\{ poll: true \}\)/);
  assert.match(chrome, /applyBrowserState\(await api\.browserAction\(targeted\)\)/);
  // The panel's page image is pulled one at a time and at most every SNAPSHOT_MIN_MS on changes.
  assert.match(panel, /export const SNAPSHOT_MIN_MS = 500;/);
  assert.match(panel, /subscribeBrowserState\(soon\)/);
  // The app sends the numbered state with the event.
  const api = readFileSync("src/api.ts", "utf8");
  assert.match(api, /listen<BrowserState \| null>\("browser-state-changed", \(event\) => onChanged\(event\.payload \?\? null\)\)/);
  const commands = readFileSync("src-tauri/src/commands.rs", "utf8");
  assert.match(commands, /surface::panel_area\(handle, panel_rect\(area\)\?, visible\)\?;[\s\S]{0,200}return surface::current_state\(handle\);/);
  // A separate window's toolbar does not load the whole app.
  const main = readFileSync("src/main.tsx", "utf8");
  assert.match(main, /import\("\.\/components\/BrowserChrome"\)/);
  assert.match(main, /import\("\.\/App"\)/);
  assert.doesNotMatch(main, /^import App/m);
});

test("settings calls from a window run in the order they were made", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { inSettingsOrder } = await server.ssrLoadModule("/src/api.ts");
    const order = [];
    let releaseFirst;
    const first = inSettingsOrder(() => new Promise((resolve) => { releaseFirst = () => { order.push("first"); resolve(1); }; }));
    const failed = inSettingsOrder(async () => { order.push("second"); throw new Error("保存できません"); });
    const third = inSettingsOrder(async () => { order.push("third"); return 3; });
    await new Promise((resolve) => setTimeout(resolve, 5));
    assert.deepEqual(order, [], "later calls wait for the first");
    releaseFirst();
    assert.equal(await first, 1);
    await assert.rejects(failed, /保存できません/);
    assert.equal(await third, 3, "a failure does not stop the calls after it");
    assert.deepEqual(order, ["first", "second", "third"]);
    const source = readFileSync("src/api.ts", "utf8");
    assert.match(source, /updateSettings: \(patch: Partial<Settings>\) => inSettingsOrder\(/);
    assert.match(source, /settings: \(\) => inSettingsOrder\(/);
    const commands = readFileSync("src-tauri/src/commands.rs", "utf8");
    assert.match(commands, /#\[tauri::command\(async\)\]\npub fn update_settings\(/, "saving runs off the UI thread that places the pages");
  } finally {
    await server.close();
  }
});
