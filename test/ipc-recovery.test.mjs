import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { createServer } from "vite";

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const waitFor = async (check) => { for (let i = 0; i < 200; i++) { if (check()) return; await new Promise((resolve) => setTimeout(resolve, 5)); } assert.fail("report did not settle"); };
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };

test("bounded IPC reads and ordered panel reports recover without replaying mutations", async (t) => {
  const server = await createServer({ configFile: false, server: { middlewareMode: true, hmr: false, ws: false }, appType: "custom" });
  try {
    const { installIpcRecovery, readWithTimeout } = await server.ssrLoadModule("/src/ipcRecovery.ts");
    const { createPanelReporter } = await server.ssrLoadModule("/src/panelReporter.ts");
    await t.test("Tauri 2.12.5 switches to authenticated postMessage and does not replay a pending save", async () => {
      const callbacks = new Map();
      const fetches = [];
      const messages = [];
      const target = {
        __TAURI_INTERNALS__: {
          convertFileSrc: (command) => `http://ipc.localhost/${encodeURIComponent(command)}`,
          runCallback: (id, value) => callbacks.get(id)?.(value),
        },
        fetch: (url) => { fetches.push(url); return new Promise(() => {}); },
        ipc: { postMessage: (raw) => {
          const message = JSON.parse(raw);
          assert.equal(message.__TAURI_INVOKE_KEY__, "fixture-key");
          messages.push(message.cmd);
          target.__TAURI_INTERNALS__.runCallback(message.callback, { ok: true });
        } },
      };
      const restore = installIpcRecovery(target, 15);
      // Unmodified Tauri 2.12.5 script (MIT/Apache-2.0), with only its build-time
      // templates filled in. This exercises the real fallback rather than a replica.
      const script = readFileSync("test/fixtures/tauri-ipc-protocol.js", "utf8")
        .replace("__TEMPLATE_invoke_key__", '"fixture-key"')
        .replace("__TEMPLATE_os_name__", '"windows"')
        .replace("__TEMPLATE_fetch_channel_data_command__", '"plugin:__TAURI_CHANNEL__|fetch"')
        .replace("__RAW_process_ipc_message_fn__", '(payload => ({contentType:"application/json",data:JSON.stringify(payload)}))');
      vm.runInNewContext(script, { window: target, fetch: (...args) => target.fetch(...args), Headers, console });
      let serial = 0;
      const invoke = (cmd, payload = {}) => new Promise((resolve, reject) => {
        const callback = ++serial, error = ++serial;
        callbacks.set(callback, resolve); callbacks.set(error, reject);
        target.__TAURI_INTERNALS__.postMessage({ cmd, callback, error, payload });
      });
      try {
        void invoke("update_settings", { patch: { libraryFolder: "private" } });
        assert.deepEqual(await invoke("get_settings"), { ok: true });
        assert.deepEqual(messages, ["get_settings"], "the pending mutation was never repeated");
        assert.deepEqual(await invoke("update_settings", { patch: { uiRailCollapsed: false } }), { ok: true });
        assert.deepEqual(messages, ["get_settings", "update_settings"]);
        assert.equal(fetches.length, 2, "subsequent calls use postMessage directly");
      } finally { restore(); }
    });
    await t.test("a stalled safe read rejects and aborts the custom-protocol fetch", async () => {
      let signal;
      const target = { __TAURI_INTERNALS__: {}, fetch: (_input, init) => { signal = init.signal; return new Promise(() => {}); } };
      const restore = installIpcRecovery(target, 15);
      try {
        await assert.rejects(target.fetch("http://ipc.localhost/get_settings", { method: "POST", body: "{}" }), /接続方法を切り替え/);
        assert.equal(signal.aborted, true);
      } finally { restore(); }
    });
    await t.test("external requests, saves, navigation, downloads and caller cancellation pass through", async () => {
      const pending = new Promise(() => {});
      const calls = [];
      const original = (input, init) => { calls.push({ input, init }); return pending; };
      const target = { __TAURI_INTERNALS__: {}, fetch: original };
      const restore = installIpcRecovery(target, 1);
      try {
        const cases = [
          ["https://booth.pm/ja/items/1234", { method: "POST" }],
          ["http://ipc.localhost.evil/get_settings", { method: "POST" }],
          ["http://ipc.localhost/update_settings", { method: "POST", body: '{"patch":{"libraryFolder":"private"}}' }],
          ["http://ipc.localhost/download_booth_file", { method: "POST" }],
          ["http://ipc.localhost/browser_action", { method: "POST", body: '{"action":{"kind":"newTab"}}' }],
          ["http://ipc.localhost/browser_action", { method: "POST", body: "bad json" }],
          ["http://ipc.localhost/get_settings", { method: "POST", signal: new AbortController().signal }],
        ];
        for (const [input, init] of cases) assert.equal(target.fetch(input, init), pending);
        assert.deepEqual(calls, cases.map(([input, init]) => ({ input, init })));
        await tick();
      } finally { restore(); }
      assert.equal(target.fetch, original);
      const preview = { fetch: original };
      installIpcRecovery(preview, 1);
      assert.equal(preview.fetch, original, "browser preview has no interception");
    });
    await t.test("an idempotent panel report can trigger fallback, and success clears the deadline", async () => {
      const target = { __TAURI_INTERNALS__: {}, fetch: () => new Promise(() => {}) };
      const restore = installIpcRecovery(target, 10);
      await assert.rejects(target.fetch("http://ipc.localhost/browser_action", { method: "POST", body: '{"action":{"kind":"panelArea","area":null,"visible":false}}' }));
      restore();
      const response = new Response("{}");
      target.fetch = async () => response;
      const done = installIpcRecovery(target, 10);
      assert.equal(await target.fetch("http://ipc.localhost/browser_state", { method: "POST" }), response);
      done();
    });
    await t.test("a timed-out UI read ignores its late result and a later read can succeed", async () => {
      const late = deferred();
      await assert.rejects(readWithTimeout(() => late.promise, "一般設定", 10), /一般設定の応答がありません/);
      late.resolve("old");
      assert.equal(await readWithTimeout(async () => "new", "一般設定", 10), "new");
    });
    await t.test("failed show and final hide retry without another measurement or component", async () => {
      const attempts = [];
      const report = createPanelReporter(async (next) => {
        attempts.push(next);
        if (attempts.length === 1 || attempts.length === 3) throw new Error("offline");
      }, 5);
      const show = { area: { left: 800, top: 100, width: 400, height: 600 }, visible: true };
      const hide = { area: null, visible: false };
      report(show);
      await waitFor(() => attempts.length === 2);
      assert.deepEqual(attempts, [show, show]);
      report(show);
      await tick();
      assert.equal(attempts.length, 2, "only acknowledged reports are deduplicated");
      report(hide);
      await waitFor(() => attempts.length === 4);
      assert.deepEqual(attempts, [show, show, hide, hide]);
    });
    await t.test("a pending old show cannot overwrite a newer hide or a remounted panel", async () => {
      const requests = [];
      const report = createPanelReporter((next) => { const pending = deferred(); requests.push({ next, ...pending }); return pending.promise; }, 5);
      const show = { area: { left: 800, top: 100, width: 400, height: 600 }, visible: true };
      const hide = { area: null, visible: false };
      report(show);
      report(hide);
      assert.equal(requests.length, 1, "at most one native report is in flight");
      requests[0].resolve();
      await tick();
      assert.deepEqual(requests[1].next, hide);
      report(show);
      requests[1].resolve();
      await tick();
      assert.deepEqual(requests[2].next, show);
      requests[2].resolve();
      await tick();
    });
    await t.test("an uncertain failed show invalidates the old hide acknowledgement", async () => {
      const requests = [];
      const report = createPanelReporter((next) => { const pending = deferred(); requests.push({ next, ...pending }); return pending.promise; }, 5);
      const hide = { area: null, visible: false };
      const show = { area: { left: 800, top: 100, width: 400, height: 600 }, visible: true };
      report(hide); requests[0].resolve(); await tick();
      report(show); report(hide); requests[1].reject(new Error("response lost"));
      await waitFor(() => requests.length === 3);
      assert.deepEqual(requests[2].next, hide);
      requests[2].resolve(); await tick();
    });
  } finally { await server.close(); }
});
