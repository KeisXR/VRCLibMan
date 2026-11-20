import assert from "node:assert/strict";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

// Turning the built-in browser off keeps its login on the device. The delete button must stay
// reachable then, and stay hidden when nothing is stored.
test("the browsing data can be deleted while the built-in browser is off", async () => {
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/#/settings");
  window.HTMLInputElement.prototype.select = () => {};
  window.confirm = () => true;
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Element", "Event", "CustomEvent", "EventTarget", "MutationObserver"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const cacheDir = await mkdtemp(path.join(os.tmpdir(), "vrclibman-browser-data-test-"));
  const server = await createServer({ cacheDir, server: { middlewareMode: true, hmr: false, ws: false }, appType: "custom" });
  const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
  let root;
  try {
    await server.ssrLoadModule("/src/preview/api.ts");
    await server.ssrLoadModule("/package.json");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { SettingsPage } = await server.ssrLoadModule("/src/pages/SettingsPage.tsx");
    const { AppUpdaterProvider } = await server.ssrLoadModule("/src/useAppUpdater.ts");
    const notices = [];
    const shell = { revision: 0, refresh() {}, notify(message) { notices.push(message); }, fail(message) { assert.fail(message); }, workInProgress: false, builtinBrowser: false, refreshAccount() {} };
    const tree = () => createElement(AppUpdaterProvider, null, createElement(ShellProvider, { value: shell }, createElement(SettingsPage)));
    const wait = async (check) => { for (let i = 0; i < 150; i++) { if (check()) return; await act(tick); } assert.fail(document.body.textContent.slice(0, 400)); };
    const row = () => document.querySelector("#settings-clear-browser-data");
    const mount = async () => {
      root = createRoot(document.getElementById("root"));
      await act(async () => { root.render(tree()); await tick(); });
      await wait(() => document.querySelector("#settings-data"));
    };
    assert.equal((await api.settings()).builtinBrowser, false, "the preview starts with the built-in browser off");

    // Nothing stored: no button.
    let stored = false;
    let cleared = 0;
    api.boothSessionStored = async () => stored;
    api.clearBoothSession = async () => { cleared += 1; stored = false; };
    await mount();
    await act(tick);
    assert.equal(row(), null);
    await act(async () => { root.unmount(); });

    // A profile left by earlier use: the button and the reason it is there.
    stored = true;
    await mount();
    await wait(() => row());
    assert.match(row().textContent, /内蔵ブラウザの閲覧データを削除/);
    assert.match(row().textContent, /この端末に残っています/);
    const button = row().querySelector("button");
    const props = Object.keys(button).find((key) => key.startsWith("__reactProps$"));
    await act(async () => { button[props].onClick(); await tick(); });
    await wait(() => !row());
    assert.equal(cleared, 1);
    assert.deepEqual(notices, ["閲覧データを削除しました"]);
  } finally {
    if (root) await act(async () => { root.unmount(); });
    await server.close();
    await rm(cacheDir, { recursive: true, force: true });
  }
});
