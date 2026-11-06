import assert from "node:assert/strict";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

test("settings stay usable when auxiliary reads stall and failed reads can be retried", async () => {
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/#/settings");
  window.HTMLInputElement.prototype.select = () => {};
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Element", "Event", "CustomEvent", "EventTarget", "MutationObserver"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const cacheDir = await mkdtemp(path.join(os.tmpdir(), "vrclibman-settings-test-"));
  const server = await createServer({
    cacheDir,
    server: { middlewareMode: true, hmr: false, ws: false }, appType: "custom",
    plugins: [{ name: "short-read-deadline", enforce: "pre", transform(code, id) {
      if (id.replaceAll("\\", "/").endsWith("/src/ipcRecovery.ts")) return code.replace("READ_TIMEOUT_MS = 10000", "READ_TIMEOUT_MS = 80");
    } }],
  });
  const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
  const root = createRoot(document.getElementById("root"));
  let activeRoot = root;
  try {
    await server.ssrLoadModule("/src/preview/api.ts");
    await server.ssrLoadModule("/package.json");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { SettingsPage } = await server.ssrLoadModule("/src/pages/SettingsPage.tsx");
    const { AppUpdaterProvider } = await server.ssrLoadModule("/src/useAppUpdater.ts");
    const original = { ...api };
    const initial = await api.settings();
    let staleKnowledge;
    api.knowledge = () => new Promise((resolve) => { staleKnowledge = resolve; });
    api.catalogInfo = async () => { throw new Error("カタログの確認失敗"); };
    api.appleFmStatus = () => new Promise(() => {});
    const shell = { revision: 0, refresh() {}, notify() {}, fail() {}, workInProgress: false, builtinBrowser: false, refreshAccount() {} };
    const tree = () => createElement(AppUpdaterProvider, null, createElement(ShellProvider, { value: shell }, createElement(SettingsPage)));
    const render = () => act(async () => { root.render(tree()); await tick(); });
    const wait = async (check) => { for (let i = 0; i < 150; i++) { if (check()) return; await act(tick); } assert.fail(document.body.textContent); };
    const retry = () => [...document.querySelectorAll("button")].find((b) => b.textContent === "もう一度読み込む");
    await render();
    await wait(() => document.querySelector("#settings-library input"));
    assert.equal(document.querySelector("#settings-library input").value, initial.libraryFolder);
    assert.match(document.querySelector("#settings-knowledge").textContent, /読み込んでから編集/);
    await wait(() => document.body.textContent.includes("対応表の応答がありません"));
    assert.match(document.body.textContent, /Appleモデルの状態の応答がありません/);
    assert.ok(document.querySelector("#settings-library input"), "optional failures never blank the settings");

    const input = document.querySelector("#settings-library input");
    const props = Object.keys(input).find((key) => key.startsWith("__reactProps$"));
    api.updateSettings = () => new Promise(() => {}); // unsaved edit must survive the reload
    await act(async () => { input[props].onChange({ target: { value: "C:/draft" } }); await tick(); });
    api.knowledge = original.knowledge;
    api.catalogInfo = original.catalogInfo;
    api.appleFmStatus = original.appleFmStatus;
    await act(async () => { retry().dispatchEvent(new window.Event("click", { bubbles: true })); await tick(); });
    await wait(() => document.querySelector("#settings-knowledge .knowledge"));
    assert.equal(document.querySelector("#settings-library input").value, "C:/draft", "reload cannot overwrite an unsaved draft");
    await act(async () => { staleKnowledge({ avatars: [], baseBodies: [] }); await tick(); });
    assert.notEqual(document.querySelector("#settings-knowledge .knowledge .meta").textContent, "0 件中 0 件を表示", "the timed-out response is ignored");

    await act(async () => { root.unmount(); await tick(); });
    Object.assign(api, original);
    api.settings = () => new Promise(() => {});
    const second = createRoot(document.getElementById("root"));
    activeRoot = second;
    await act(async () => { second.render(tree()); await tick(); });
    assert.match(document.body.textContent, /設定を読み込んでいます/);
    await wait(() => document.body.textContent.includes("一般設定の応答がありません"));
    assert.equal(document.querySelector("#settings-library"), null);
    api.settings = original.settings;
    await act(async () => { retry().dispatchEvent(new window.Event("click", { bubbles: true })); await tick(); });
    await wait(() => document.querySelector("#settings-library input"));
  } finally {
    await act(async () => { activeRoot.unmount(); await tick(); });
    await server.close();
    await rm(cacheDir, { recursive: true, force: true });
  }
});
