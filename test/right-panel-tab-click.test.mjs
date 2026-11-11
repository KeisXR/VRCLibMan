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
    await t.test("browser menu remains visible and clickable outside the toolbar", async () => {
      for (let attempt = 0; !(await evaluate("!!document.querySelector('.browser-more-button')")); attempt++) {
        if (attempt > 100) throw new Error("browser controls did not load");
        await pause(50);
      }
      await press('.browser-more-button');
      const menu = await evaluate(`(() => {
        const menu = document.querySelector('.browser-more-popover');
        const buttons = [...menu.querySelectorAll('button')];
        return { portal: menu.parentElement === document.body, reachable: buttons.filter(b => !b.disabled).every(b => {
          const r = b.getBoundingClientRect();
          return b.contains(document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2));
        }) };
      })()`);
      assert.deepEqual(menu, { portal: true, reachable: true });
      await press('.browser-more-popover button[aria-label="ページを縮小"]');
      assert.equal(await evaluate("document.querySelector('.browser-more-zoom-value').textContent"), '90%');
      await page('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Escape', code: 'Escape' });
      await page('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Escape', code: 'Escape' });
      assert.equal(await evaluate("!!document.querySelector('.browser-more-popover')"), false);
    });
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
      // Drop just inside the left edge of the first tab, well before its midpoint. Measuring
      // from tab edges instead made the drop point depend on label widths, so it landed past
      // the midpoint (no reorder) with the fonts on CI runners.
      const span = await evaluate(`(() => {
        const from = document.querySelector(${JSON.stringify(pageTab)}).getBoundingClientRect();
        const to = document.querySelector(${JSON.stringify(previewTab)}).getBoundingClientRect();
        return from.left + from.width / 2 - (to.left + Math.min(4, to.width / 4));
      })()`);
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
    await t.test("a page tab with a very long title can still be closed", async () => {
      // The preview names a page tab after its host, so a long host gives a long title.
      const host = Array.from({ length: 12 }, (_, i) => `very-long-booth-page-title-${i}`).join(".") + ".test";
      const longId = await evaluate(`(async () => { const { api } = await import("/src/api.ts"); return (await api.browserAction({ kind: "newTab", url: "https://${host}/" })).activeId; })()`);
      const longTab = `#right-panel-tab-web-${longId}`;
      for (let attempt = 0; !(await evaluate(`!!document.querySelector(${JSON.stringify(longTab)})`)); attempt++) {
        if (attempt > 100) throw new Error("long page tab did not appear");
        await pause(20);
      }
      await press(longTab);
      assert.equal(await selected(), longTab.slice(1));
      const close = `[data-tab-id="web-${longId}"] .right-panel-tab-close`;
      const hit = await evaluate(`(() => {
        const button = document.querySelector(${JSON.stringify(close)});
        const strip = document.querySelector('.right-panel-tabs').getBoundingClientRect();
        const r = button.getBoundingClientRect();
        const x = r.left + r.width / 2, y = r.top + r.height / 2;
        const others = [...document.querySelectorAll('.right-panel-tab')].filter((tab) => !tab.contains(button));
        return {
          inside: r.left >= strip.left - 0.5 && r.right <= strip.right + 0.5,
          reachable: button.contains(document.elementFromPoint(x, y)),
          othersVisible: others.every((tab) => tab.getBoundingClientRect().width > 40),
        };
      })()`);
      assert.deepEqual(hit, { inside: true, reachable: true, othersVisible: true }, "the close button stays inside the strip without crushing other tabs");
      await press(close);
      assert.equal(await evaluate(`!!document.querySelector(${JSON.stringify(longTab)})`), false, "the long tab is closed");
      await press(pageTab);
      assert.equal(await selected(), pageTab.slice(1));
    });
    await t.test("three to five tabs remain independently clickable at narrow widths", async () => {
      await evaluate(`(async () => {
        const { api } = await import("/src/api.ts");
        await api.updateSettings({ previewPaneWidth: 300 });
      })()`);
      for (const [width, height] of [[1180, 800], [920, 640]]) {
        await page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
        await pause(250);
        for (const count of [3, 5]) {
          await evaluate(`(async () => {
            const { api } = await import("/src/api.ts");
            while (document.querySelectorAll('.right-panel-tabs [role=tab]').length > ${count}) {
              const last = [...document.querySelectorAll('.right-panel-tabs [role=tab]')].findLast(tab => tab.id.includes('-web-'));
              await api.browserAction({ kind: "closeTab", tabId: last.id.split('-web-')[1] });
              await new Promise(resolve => setTimeout(resolve, 100));
            }
            for (let n = document.querySelectorAll('.right-panel-tabs [role=tab]').length; n < ${count}; n++) {
              await api.browserAction({ kind: "newTab", url: "https://accounts.booth.pm/library" });
            }
          })()`);
          await pause(250);
          const ids = await evaluate("[...document.querySelectorAll('.right-panel-tabs [role=tab]')].map(tab => tab.id)");
          assert.equal(ids.length, count);
          for (const id of ids) {
            const hit = await evaluate(`(() => {
              const tab = document.getElementById(${JSON.stringify(id)});
              tab.scrollIntoView({ block: "nearest", inline: "nearest" });
              const rect = tab.getBoundingClientRect();
              return tab.parentElement.getBoundingClientRect().width >= 36
                && tab.contains(document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2));
            })()`);
            assert.ok(hit, `${width}×${height}, ${count} tabs: ${id} has its own hit target`);
            await press(`#${id}`);
            assert.equal(await selected(), id);
          }
        }
      }
    });
    await t.test("standard design keeps page geometry aligned in both color schemes", async () => {
      for (const scheme of ["light", "dark"]) {
        await page("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
        for (const width of [1600, 1180, 1100]) {
          await page("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: false });
          await pause(600);
          const metrics = await evaluate(`(() => {
            const viewport = document.querySelector('.browser-panel-viewport').getBoundingClientRect();
            const body = document.querySelector('.right-panel-body-web').getBoundingClientRect();
            const chrome = getComputedStyle(document.querySelector('.browser-chrome.panel'));
            return { viewport: { left:viewport.left, top:viewport.top, width:viewport.width, height:viewport.height, bottom:viewport.bottom }, body: { width:body.width, bottom:body.bottom }, reported:window.layoutFixture.area(), shadow:chrome.boxShadow, border:chrome.borderLeftWidth };
          })()`);
          assert.equal(metrics.reported.visible, true);
          for (const key of ["left", "top", "width", "height"]) assert.ok(Math.abs(metrics.viewport[key] - metrics.reported.area[key]) <= 1, `${scheme}/${width}/${key}: native and DOM rectangles match`);
          assert.ok(metrics.viewport.height > 200);
          assert.ok(Math.abs(metrics.viewport.bottom - metrics.body.bottom) <= 2);
          assert.ok(Math.abs(metrics.viewport.width - metrics.body.width) <= 2);
          assert.equal(metrics.shadow, "none");
          assert.equal(metrics.border, "0px");
        }
      }
    });
  } finally {
    socket?.close();
    chrome.kill();
    await vite.close();
    await pause(200);
    await rm(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
});
