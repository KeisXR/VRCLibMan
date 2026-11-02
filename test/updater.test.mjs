import assert from "node:assert/strict";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";

test("updater requires explicit download and restart, survives stale status, and permits retries", async () => {
  const { window } = parseHTML(
    '<html><body><div id="root"></div></body></html>',
  );
  window.location = new URL("http://localhost:1420/");
  for (const name of [
    "window",
    "document",
    "location",
    "HTMLElement",
    "Event",
    "CustomEvent",
    "EventTarget",
  ]) {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      writable: true,
      value: window[name],
    });
  }
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: { userAgent: "test" },
  });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
  });
  const root = createRoot(document.getElementById("root"));
  const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
  const byText = (text) =>
    [...document.querySelectorAll("button")].find(
      (node) => node.textContent === text,
    );
  const click = async (text) =>
    act(async () => {
      const button = byText(text);
      assert.ok(button && !button.disabled, `${text} is enabled`);
      button.dispatchEvent(new window.Event("click", { bubbles: true }));
      await tick();
    });
  try {
    const { appUpdater } = await server.ssrLoadModule("/src/api.ts");
    const { AppUpdater } = await server.ssrLoadModule(
      "/src/components/AppUpdater.tsx",
    );
    const { AppUpdaterProvider } = await server.ssrLoadModule("/src/useAppUpdater.ts");
    const idle = {
      revision: 0,
      phase: "idle",
      version: null,
      notes: null,
      downloaded: 0,
      total: null,
      error: null,
      message: null,
    };
    let current = idle;
    let checks = 0,
      downloads = 0,
      installs = 0,
      flushes = 0;
    let rejectFlush = false;
    appUpdater.status = async () => idle; // A late poll must never overwrite a newer response.
    appUpdater.check = async () => {
      checks++;
      return (current = {
        ...idle,
        revision: 1,
        phase: "available",
        version: "99.0.0",
        notes: "<img src=x onerror=alert(1)>",
      });
    };
    appUpdater.download = async () => {
      downloads++;
      return (current = {
        ...current,
        revision: current.revision + 1,
        phase: downloads === 1 ? "available" : "ready",
        error: downloads === 1 ? "署名を確認できません" : null,
      });
    };
    appUpdater.install = async () => {
      installs++;
      return (current = {
        ...current,
        revision: current.revision + 1,
        phase: "ready",
        error: "更新を適用できませんでした",
      });
    };
    const beforeInstall = async () => {
      flushes++;
      if (rejectFlush) throw new Error("設定を保存できません");
    };
    const render = (workInProgress = false) =>
      root.render(
        createElement(AppUpdaterProvider, null, createElement(AppUpdater, { beforeInstall, workInProgress })),
      );
    await act(async () => {
      render();
      await tick();
    });
    assert.equal(checks, 0, "mount only reads local state");
    await click("更新を確認");
    assert.equal(checks, 1);
    assert.equal(downloads, 0);
    assert.ok(document.body.textContent.includes("v99.0.0 に更新できます"));
    assert.equal(
      document.querySelector("img"),
      null,
      "release notes are plain text",
    );
    await click("更新をダウンロード");
    assert.ok(
      document.querySelector('[role="alert"]').textContent.includes("署名"),
    );
    assert.equal(installs, 0);
    await click("更新をダウンロード");
    assert.ok(document.body.textContent.includes("準備ができました"));
    await act(async () => {
      render(true);
      await tick();
    });
    assert.ok(byText("再起動して更新").disabled);
    await act(async () => {
      render();
      await tick();
    });
    window.confirm = () => false;
    await click("再起動して更新");
    assert.equal(installs, 0);
    assert.equal(flushes, 0);
    window.confirm = () => true;
    rejectFlush = true;
    await click("再起動して更新");
    assert.equal(installs, 0, "unsaved settings block installation");
    assert.ok(document.body.textContent.includes("設定を保存できません"));
    rejectFlush = false;
    await click("再起動して更新");
    assert.equal(installs, 1);
    assert.ok(document.body.textContent.includes("更新を適用できませんでした"));
    await click("再起動して更新");
    assert.equal(installs, 2, "installation failure can be retried");
  } finally {
    await act(() => root.unmount());
    await server.close();
  }
});

test("startup checks once across pending changes, failures and StrictMode; manual checks still work", async () => {
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  for (const name of ["window", "document", "HTMLElement", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  mockIPC(() => undefined, { shouldMockEvents: true });
  const { act, createElement, StrictMode } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  const root = createRoot(document.getElementById("root"));
  const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
  try {
    const { appUpdater } = await server.ssrLoadModule("/src/api.ts");
    const { AppUpdaterProvider, useAppUpdater, useStartupUpdateCheck } = await server.ssrLoadModule("/src/useAppUpdater.ts");
    const idle = { revision: 0, phase: "idle", version: null, notes: null, downloaded: 0, total: null, error: null, message: null };
    appUpdater.status = async () => idle;
    let updater;
    function Probe({ enabled }) {
      updater = useAppUpdater();
      useStartupUpdateCheck(enabled);
      return null;
    }
    const render = (key, enabled) => root.render(createElement(StrictMode, null,
      createElement(AppUpdaterProvider, { key }, createElement(Probe, { enabled }))));

    for (const outcome of ["current", "available", "idle", "reject"]) {
      const calls = [];
      let complete;
      appUpdater.check = (manual) => {
        calls.push(manual);
        return new Promise((resolve, reject) => {
          complete = () => outcome === "reject"
            ? reject(new Error("通信失敗"))
            : resolve({ ...idle, revision: calls.length, phase: outcome });
        });
      };
      await act(async () => { render(outcome, true); await tick(); });
      assert.deepEqual(calls, [false], "StrictMode starts one automatic check");
      assert.equal(updater.pending, true);
      await act(async () => { complete(); await tick(); });
      assert.equal(updater.pending, false);
      assert.deepEqual(calls, [false], `${outcome} does not trigger another automatic check`);
      await act(async () => { render(outcome, false); await tick(); });
      await act(async () => { render(outcome, true); await tick(); });
      assert.deepEqual(calls, [false], "re-enabling does not repeat the startup check");

      await act(async () => {
        void updater.check(true);
        void updater.check(true);
        await tick();
      });
      assert.deepEqual(calls, [false, true], "manual retry is allowed, concurrent calls are ignored");
      await act(async () => { complete(); await tick(); });
      assert.equal(updater.pending, false);
      assert.deepEqual(calls, [false, true], "manual completion does not restart the startup check");
    }
    await act(async () => { render("disabled", false); await tick(); });
    let checks = 0;
    appUpdater.check = async () => { checks++; return idle; };
    await act(async () => { render("disabled", false); await tick(); });
    assert.equal(checks, 0, "disabled startup does not check");
    await act(async () => { render("disabled", true); await tick(); });
    assert.equal(checks, 1, "settings loaded after mount can enable the startup check");
  } finally {
    await act(() => root.unmount());
    clearMocks();
    await server.close();
  }
});
