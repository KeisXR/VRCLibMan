import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

const row = (id, opts = {}) => ({
  revision: 1,
  boothId: id,
  title: `商品 ${id}`,
  enabled: true,
  checkedAt: "2026-11-05T00:00:00.000Z",
  error: null,
  changes: [],
  ...opts,
});
const status = (rows, extra = {}) => ({
  automatic: false,
  allProducts: false,
  totalProducts: rows.length,
  running: false,
  rows,
  ...extra,
});

test("the settings section is labelled アセットの更新 and the sidebar button sits above the account pill", () => {
  const nav = readFileSync("src/settingsNav.ts", "utf8");
  assert.match(nav, /id: "settings-asset-updates", label: "アセットの更新"/, "nav label");
  const section = readFileSync("src/components/AssetUpdates.tsx", "utf8");
  assert.match(section, /<h2>アセットの更新<\/h2>/, "section heading");
  const app = readFileSync("src/App.tsx", "utf8");
  const buttonAt = app.indexOf("<SidebarAssetUpdateButton");
  const accountAt = app.indexOf("<AccountPill");
  assert.ok(buttonAt > 0 && accountAt > 0 && buttonAt < accountAt, "button renders above the account pill in the rail foot");
});

test("sidebar asset update button runs the configured check and reports a quiet result", async () => {
  const { window } = parseHTML("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  window.location = new URL("http://127.0.0.1:1420/#/");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "HTMLButtonElement", "Event", "CustomEvent", "EventTarget", "MouseEvent"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test", clipboard: { writeText: async () => {} } } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { act, createElement } = await import("react");
    const { createRoot } = await import("react-dom/client");
    const { assetUpdates } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { SidebarAssetUpdateButton, useSidebarAssetUpdate } = await server.ssrLoadModule("/src/components/SidebarAssetUpdateButton.tsx");
    const { AssetUpdateLogPage } = await server.ssrLoadModule("/src/pages/AssetUpdateLogPage.tsx");
    const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
    const notes = [];
    const shell = {
      revision: 0,
      refresh() {},
      notify(message, action, options) { notes.push({ message, options }); },
      fail(message) { throw new Error(message); },
      builtinBrowser: false,
      refreshAccount() {},
      workInProgress: false,
      searchQuery: "",
      setSearchQuery() {},
    };
    function Sidebar({ compact }) {
      const update = useSidebarAssetUpdate(shell.notify, shell.fail);
      return createElement(SidebarAssetUpdateButton, { ...update, compact });
    }
    root = createRoot(document.getElementById("root"));
    const render = async (compact = false) => {
      // Remount each time: the button reads its status on mount, like a fresh sidebar.
      await act(async () => { root.render(null); });
      await act(async () => {
        root.render(createElement(Sidebar, { compact }));
        await tick();
      });
    };

    // Nothing configured: no button.
    assetUpdates.status = async () => status([row("1", { enabled: false })]);
    await render();
    assert.equal(document.querySelector(".sidebar-asset-update"), null, "hidden until a target is enabled");

    // Configured targets: the button appears, and a press runs the same manual check.
    let checks = 0;
    assetUpdates.status = async () => status([row("1"), row("2")]);
    assetUpdates.check = async () => { checks += 1; return status([row("1"), row("2")]); };
    await render();
    const button = document.querySelector(".sidebar-asset-update");
    assert.ok(button, "button appears once targets are enabled");
    assert.match(button.textContent, /アセットの更新を確認/);
    await act(async () => { button.click(); await tick(); });
    assert.equal(checks, 1, "press runs the check");
    assert.ok(notes.some((n) => n.message.includes("更新なし")), "a quiet result is reported");

    // A result with failures stays on screen as a persistent notice.
    notes.length = 0;
    assetUpdates.check = async () => status([row("1", { error: "取得に失敗しました" }), row("2")]);
    await act(async () => { document.querySelector(".sidebar-asset-update").click(); await tick(); });
    const failure = notes.find((n) => n.message.includes("確認失敗"));
    assert.ok(failure, "failures are reported");
    assert.equal(failure.options?.persist, true, "failure notice persists");

    // Enabling the first target in settings surfaces the button without a reload:
    // the mounted button re-reads its status when the screen changes.
    assetUpdates.status = async () => status([row("1", { enabled: false })]);
    await render();
    assert.equal(document.querySelector(".sidebar-asset-update"), null, "hidden again with no targets");
    assetUpdates.status = async () => status([row("1")]);
    await act(async () => { window.dispatchEvent(new window.Event("hashchange")); await tick(); });
    assert.ok(document.querySelector(".sidebar-asset-update"), "button appears after the settings change");

    // Collapsed rail: an icon button with the same action.
    await render(true);
    const icon = document.querySelector(".rail-icon-button");
    assert.ok(icon, "compact icon button");
    assert.equal(icon.getAttribute("aria-label"), "アセットの更新を確認");
    assetUpdates.status = async () => status([row("424242", { changes: ["新しいファイルの追加"] })], { running: true, progress: { done: 7, total: 159 } });
    await render();
    assert.doesNotMatch(document.querySelector(".sidebar-asset-update").textContent, /159/);
    assert.match(document.querySelector(".sidebar-asset-progress").textContent, /7 \/ 159 件/);
    assetUpdates.status = async () => status([row("424242", { changes: ["新しいファイルの追加"] })]);
    await act(async () => { window.dispatchEvent(new window.Event("hashchange")); await tick(); });
    assert.equal(document.querySelector(".sidebar-asset-update").disabled, false);

    assetUpdates.history = async () => [{ boothId: "424242", detectedAt: "2026-11-05T00:00:00.000Z", source: "library", descriptionChanged: false,
      removed: [{ filename: "old.zip", variation: "通常", downloadableId: "123456" }], added: [{ filename: "new.zip", variation: "通常", downloadableId: "234567" }] }];
    assetUpdates.configure = async () => status([row("424242")]);
    await act(async () => { root.render(createElement(ShellProvider, { value: shell }, createElement(AssetUpdateLogPage, { onAcknowledge() {} }))); await tick(); });
    const timestamp = document.querySelector("time");
    assert.equal(timestamp.getAttribute("datetime") ?? timestamp.getAttribute("dateTime"), "2026-11-05T00:00:00.000Z");
    assert.deepEqual([...document.querySelectorAll(".log-changes code")].map(n => n.textContent), ["https://booth.pm/downloadables/123456", "https://booth.pm/downloadables/234567"]);
    await act(async () => { root.render(null); });
  } finally {
    await server.close();
  }
});
