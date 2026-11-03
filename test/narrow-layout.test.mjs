import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";

// Narrow windows in real Chromium: the left sidebar stays a column (icons only at 860px or less),
// and the genre column turns into the sticky chip bar when the list would get less than 480px.
// Set SCREENSHOT_DIR to also save pr1-<width>.png shots of the asset page.
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const chromePath = process.env.CHROME_PATH ?? ["/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find((file) => existsSync(file));

test("narrow window layout in Chromium", { timeout: 120000, skip: chromePath ? false : "Chrome not installed (set CHROME_PATH)" }, async (t) => {
  const { createServer } = await import("vite");
  const profile = await mkdtemp(path.join(os.tmpdir(), "vrclibman-narrow-"));
  // The whole app loads here, so keep its own dependency cache: npm test runs other Vite
  // servers at the same time, and a shared cache rebuilt mid-load leaves a blank page.
  const vite = await createServer({ server: { host: "127.0.0.1", port: 0 }, cacheDir: path.join(profile, "vite"), appType: "spa", logLevel: "error" });
  await vite.listen();
  const baseUrl = `http://127.0.0.1:${vite.httpServer.address().port}`;
  const chrome = spawn(chromePath, ["--headless=new", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--window-size=1400,900", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
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
    socket = new WebSocket(wsUrl);
    await new Promise((resolve) => socket.addEventListener("open", resolve, { once: true }));
    let id = 0;
    const pending = new Map();
    const call = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const message = { id: ++id, method, params };
      if (sessionId) message.sessionId = sessionId;
      pending.set(message.id, { resolve, reject });
      socket.send(JSON.stringify(message));
    });
    socket.addEventListener("message", (event) => {
      const data = JSON.parse(event.data);
      const waiter = pending.get(data.id);
      if (!waiter) return;
      pending.delete(data.id);
      if (data.error) waiter.reject(new Error(JSON.stringify(data.error)));
      else waiter.resolve(data.result);
    });
    const { targetId } = await call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
    const page = (method, params) => call(method, params, sessionId);
    const evaluate = async (expression) => {
      const result = await page("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
      return result.result.value;
    };
    const waitFor = async (expression, label, timeout = 5000) => {
      for (const end = Date.now() + timeout; Date.now() < end; ) {
        if (await evaluate(expression).catch(() => false)) return;
        await pause(25);
      }
      const seen = await evaluate("location.href + ' ' + document.body?.innerText.slice(0, 300)").catch((error) => String(error));
      throw new Error(`timed out: ${label} (${seen})`);
    };
    const resize = async (width, height = 820) => {
      await page("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });
      await waitFor(`window.innerWidth === ${width}`, `width ${width}`);
      await pause(120);
    };
    const shot = async (name) => {
      if (!process.env.SCREENSHOT_DIR) return;
      const { data } = await page("Page.captureScreenshot", { format: "png" });
      await writeFile(path.join(process.env.SCREENSHOT_DIR, `${name}.png`), Buffer.from(data, "base64"));
    };
    // ?browser=1 makes the preview skip the setup wizard and show the browser item too.
    await page("Page.navigate", { url: `${baseUrl}/?browser=1#/assets` });
    await waitFor("!!document.querySelector('.app-shell .rail nav a') && !document.querySelector('.setup-screen')", "app", 60000);
    await waitFor("document.querySelectorAll('.genre-browser .item, .genre-browser .empty').length > 0", "asset list", 30000);

    const state = () => evaluate(`(() => {
      const shell = document.querySelector('.app-shell');
      const rail = document.querySelector('.rail');
      const nav = [...document.querySelectorAll('.rail nav .nav-link')];
      const railRect = rail.getBoundingClientRect();
      const bar = document.querySelector('.genre-chip-bar');
      return {
        columns: getComputedStyle(shell).gridTemplateColumns.split(' ').length,
        railTop: railRect.top,
        railHeight: Math.round(railRect.height),
        railWidth: Math.round(railRect.width),
        collapsed: rail.dataset.collapsed,
        auto: rail.dataset.autoCollapsed ?? null,
        toggle: !!document.querySelector('.rail-toggle'),
        navInView: nav.every((link) => { const r = link.getBoundingClientRect(); return r.bottom <= window.innerHeight && r.top >= 0 && r.right <= railRect.right; }),
        navScrolls: (() => { const n = document.querySelector('.rail nav'); return n.scrollHeight > n.clientHeight + 1; })(),
        genreColumn: !!document.querySelector('.genre-rail'),
        chipBar: !!bar,
        chips: bar ? bar.querySelectorAll('.chip-bar-item').length : 0,
        activeChip: bar?.querySelector('.chip-bar-item.active .chip-bar-label')?.textContent ?? null,
        chipCount: bar?.querySelector('.chip-bar-item .chip-bar-count')?.textContent ?? null,
        oldPicker: !!document.querySelector('.genre-mobile-picker, .genre-mobile-nav'),
        listWidth: Math.round(document.querySelector('.main').getBoundingClientRect().width),
      };
    })()`);

    const savedCollapsed = () => evaluate(`(async () => { const { api } = await import("/src/api.ts"); return (await api.settings()).uiRailCollapsed; })()`);

    await t.test("1100px: open sidebar and the genre column beside the list", async () => {
      await resize(1100);
      const s = await state();
      assert.equal(s.collapsed, "false");
      assert.equal(s.auto, null);
      assert.ok(s.toggle);
      assert.ok(s.genreColumn);
      assert.ok(!s.chipBar && !s.oldPicker);
      assert.equal(s.columns, 5, "rail | sep | genre | sep | list");
      await shot("pr1-1100");
    });

    for (const width of [1000, 900, 880]) {
      await t.test(`${width}px: the list keeps 480px with the genre column`, async () => {
        await resize(width);
        const s = await state();
        assert.equal(s.collapsed, "false");
        assert.ok(s.genreColumn, JSON.stringify(s));
        assert.ok(!s.chipBar);
        assert.ok(s.listWidth >= 480, JSON.stringify(s));
        if (width === 900) await shot("pr1-900");
      });
    }

    await t.test("860px: icons only for now, still a column with every item on screen", async () => {
      await resize(860, 520);
      const s = await state();
      assert.equal(s.collapsed, "true");
      assert.equal(s.auto, "true");
      assert.ok(!s.toggle, "the toggle would change the saved state, so it is hidden");
      assert.equal(s.railTop, 0);
      assert.equal(s.railHeight, 520, "the rail is a full-height column, not a strip cut at 42vh");
      assert.equal(s.railWidth, 60);
      assert.ok(s.navInView, JSON.stringify(s));
      assert.ok(!s.navScrolls);
      assert.ok(s.genreColumn, "a 60px rail leaves room for the genre column");
      assert.equal(await savedCollapsed(), false, "the automatic icon rail is not saved");
    });

    await t.test("700px: the genre chip bar replaces the column and stays on top", async () => {
      await resize(700);
      const s = await state();
      assert.equal(s.collapsed, "true");
      assert.ok(!s.genreColumn);
      assert.ok(s.chipBar && !s.oldPicker, JSON.stringify(s));
      assert.equal(s.columns, 3, "rail | sep | list");
      assert.ok(s.chips >= 5);
      assert.equal(s.activeChip, "すべて");
      assert.match(s.chipCount ?? "", /^\d+$/);
      await shot("pr1-700");
      const overflow = await evaluate("getComputedStyle(document.querySelector('.chip-bar-track')).overflowX");
      assert.equal(overflow, "auto");
      // Scroll the page: the chip bar sticks below the banner area.
      await evaluate("document.querySelector('.main').scrollTop = 600");
      await pause(100);
      const top = await evaluate("Math.round(document.querySelector('.genre-chip-bar').getBoundingClientRect().top)");
      const topbar = await evaluate("Math.ceil(document.querySelector('.topbar').getBoundingClientRect().bottom)");
      assert.ok(Math.abs(top - topbar) <= 1, `chip bar top ${top}, banner bottom ${topbar}`);
      await shot("pr1-700-scrolled");
      await evaluate("document.querySelector('.main').scrollTop = 0");
      // Picking a genre in the bar routes like the column and marks it current.
      await evaluate("document.querySelectorAll('.genre-chip-bar .chip-bar-item')[1].click()");
      await waitFor("document.querySelector('.genre-chip-bar .chip-bar-item.active')?.getAttribute('aria-current') === 'page' && location.hash !== '#/assets'", "chip route");
    });

    await t.test("widening again restores the saved open sidebar", async () => {
      await resize(1100);
      const s = await state();
      assert.equal(s.collapsed, "false");
      assert.ok(s.toggle && s.genreColumn && !s.chipBar);
    });
  } finally {
    socket?.close();
    chrome.kill();
    await vite.close();
    await rm(profile, { recursive: true, force: true }).catch(() => undefined);
  }
});
