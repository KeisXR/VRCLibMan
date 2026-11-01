import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

// Chromium (WebView2 on Windows) sends the click after a captured pointer to the capturing
// element, not the pressed tab. This drives real pointer input to catch that.
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const chromePath = process.env.CHROME_PATH ?? ["/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find((file) => {
  try { return process.getBuiltinModule("node:fs").existsSync(file); } catch { return false; }
});

test("right panel tabs switch on a real pointer click in Chromium", { timeout: 60000, skip: chromePath ? false : "Chrome not installed (set CHROME_PATH)" }, async (t) => {
  const { createServer } = await import("vite");
  const vite = await createServer({ server: { host: "127.0.0.1", port: 0 }, appType: "spa", logLevel: "error" });
  await vite.listen();
  const baseUrl = `http://127.0.0.1:${vite.httpServer.address().port}`;
  const profile = await mkdtemp(path.join(os.tmpdir(), "vrclibman-tab-click-"));
  const chrome = spawn(chromePath, ["--headless=new", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--window-size=1600,1000", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  let socket;
  try {
    let log = "";
    const wsUrl = await new Promise((resolve, reject) => {
      chrome.stderr.on("data", (chunk) => {
        log += chunk;
        const match = /DevTools listening on (ws:\/\/\S+)/.exec(log);
        if (match) resolve(match[1]);
      });
      chrome.on("exit", () => reject(new Error(`Chrome exited: ${log}`)));
    });
    const browserWs = new WebSocket(wsUrl);
    await new Promise((resolve) => browserWs.addEventListener("open", resolve, { once: true }));
    let id = 0;
    const pending = new Map();
    const call = (ws, method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const message = { id: ++id, method, params };
      if (sessionId) message.sessionId = sessionId;
      pending.set(message.id, { resolve, reject });
      ws.send(JSON.stringify(message));
    });
    browserWs.addEventListener("message", (event) => {
      const data = JSON.parse(event.data);
      const waiter = pending.get(data.id);
      if (!waiter) return;
      pending.delete(data.id);
      if (data.error) waiter.reject(new Error(JSON.stringify(data.error)));
      else waiter.resolve(data.result);
    });
    socket = browserWs;
    const { targetId } = await call(browserWs, "Target.createTarget", { url: "about:blank" });
    const { sessionId } = await call(browserWs, "Target.attachToTarget", { targetId, flatten: true });
    const page = (method, params) => call(browserWs, method, params, sessionId);
    const evaluate = async (expression) => (await page("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result.value;
    await page("Page.navigate", { url: `${baseUrl}/test/fixtures/browser-panel-layout.html` });
    for (let attempt = 0; ; attempt++) {
      const state = await evaluate("window.layoutFixture && { ready: window.layoutFixture.ready, error: window.layoutFixture.error }").catch(() => null);
      if (state?.error) throw new Error(state.error);
      if (state?.ready && (await evaluate("document.querySelectorAll('.right-panel-tabs [role=tab]').length")) >= 2) break;
      if (attempt > 200) throw new Error("fixture did not load");
      await pause(50);
    }
    const selected = () => evaluate("document.querySelector('.right-panel-tabs [aria-selected=true]')?.id ?? null");
    async function press(selector, dx = 0) {
      const box = await evaluate(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; })()`);
      const base = { button: "left", clickCount: 1, pointerType: "mouse" };
      await page("Input.dispatchMouseEvent", { type: "mouseMoved", x: box.x, y: box.y });
      await page("Input.dispatchMouseEvent", { type: "mousePressed", x: box.x, y: box.y, buttons: 1, ...base });
      for (let step = 1; dx && step <= 8; step++) {
        await page("Input.dispatchMouseEvent", { type: "mouseMoved", x: box.x + (dx * step) / 8, y: box.y, buttons: 1, button: "left" });
      }
      await page("Input.dispatchMouseEvent", { type: "mouseReleased", x: box.x + dx, y: box.y, ...base });
      await pause(150);
    }
    const pageId = await evaluate("window.layoutFixture.pageId");
    const pageTab = `#right-panel-tab-web-${pageId}`;
    assert.equal(await selected(), pageTab.slice(1), "the new page tab starts selected");

    const previewTab = '.right-panel-tabs [role=tab][id^="right-panel-tab-preview"]';
    const previewId = await evaluate(`document.querySelector(${JSON.stringify(previewTab)}).id`);

    await t.test("clicking a fixed tab selects it", async () => {
      await press(previewTab);
      assert.equal(await selected(), previewId);
    });
    await t.test("clicking a page tab selects it", async () => {
      await press(pageTab);
      assert.equal(await selected(), pageTab.slice(1));
    });
    await t.test("a small wobble under the drag threshold still clicks", async () => {
      await press(previewTab, 3);
      assert.equal(await selected(), previewId);
    });
    await t.test("a drag past the threshold reorders without breaking the next click", async () => {
      const span = await evaluate(`document.querySelector(${JSON.stringify(pageTab)}).getBoundingClientRect().left - document.querySelector(${JSON.stringify(previewTab)}).getBoundingClientRect().left`);
      await press(pageTab, -span);
      const order = await evaluate("[...document.querySelectorAll('.right-panel-tabs [role=tab]')].map((tab) => tab.id)");
      assert.equal(order[0], pageTab.slice(1), `page tab should move to the front: ${JSON.stringify(order)}`);
      await press(previewTab);
      assert.equal(await selected(), previewId);
      await press(pageTab);
      assert.equal(await selected(), pageTab.slice(1));
    });
    await t.test("clicking between two page tabs switches pages", async () => {
      const second = await evaluate(`(async () => { const { api } = await import("/src/api.ts"); return (await api.browserAction({ kind: "newTab", url: "https://example.test/second" })).activeId; })()`);
      const secondTab = `#right-panel-tab-web-${second}`;
      for (let attempt = 0; !(await evaluate(`!!document.querySelector(${JSON.stringify(secondTab)})`)); attempt++) {
        if (attempt > 100) throw new Error("second page tab did not appear");
        await pause(20);
      }
      await press(pageTab);
      assert.equal(await selected(), pageTab.slice(1));
      await press(secondTab);
      assert.equal(await selected(), secondTab.slice(1));
      await press(previewTab);
      assert.equal(await selected(), previewId);
      await press(secondTab);
      assert.equal(await selected(), secondTab.slice(1));
    });
    await t.test("Escape still reaches the app after a click", async () => {
      const prevented = await evaluate("(() => { const e = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }); document.body.dispatchEvent(e); return e.defaultPrevented; })()");
      assert.equal(prevented, false);
    });
  } finally {
    socket?.close();
    chrome.kill();
    await vite.close();
    await pause(200);
    await rm(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
});
