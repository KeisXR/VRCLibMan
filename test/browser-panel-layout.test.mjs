import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import test from "node:test";

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function unusedPort() {
  const server = net.createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  await new Promise((resolve) => server.close(resolve));
  return port;
}

test("page tabs fill the right panel in a real browser", { timeout: 60000 }, async (t) => {
  let vite;
  let driver;
  let session;
  let driverError;
  let driverLog = "";
  const port = await unusedPort();
  const endpoint = `http://127.0.0.1:${port}`;
  const profile = await mkdtemp(path.join(os.tmpdir(), "vrclibman-browser-layout-"));
  async function request(method, path, body) {
    const response = await fetch(`${endpoint}${path}`, {
      method,
      headers: body ? { "content-type": "application/json" } : {},
      body: body ? JSON.stringify(body) : undefined,
      signal: AbortSignal.timeout(10000),
    });
    const result = await response.json();
    if (!response.ok || result.value?.error) throw new Error(`${method} ${path}: ${JSON.stringify(result.value)}`);
    return result.value;
  }
  const wd = (method, path, body) => request(method, `/session/${session}${path}`, body);
  const evaluate = (script) => wd("POST", "/execute/sync", { script, args: [] });
  async function waitFor(check, label) {
    for (let attempt = 0; attempt < 100; attempt++) {
      const result = await check();
      if (result) return result;
      await pause(50);
    }
    throw new Error(`Timed out: ${label}`);
  }
  async function click(selector) {
    const element = await wd("POST", "/element", { using: "css selector", value: selector });
    await wd("POST", `/element/${element["element-6066-11e4-a52e-4f735466cecf"]}/click`, {});
  }
  async function checkViewport() {
    const metrics = await waitFor(() => evaluate(`
      const viewport = document.querySelector('.browser-panel-viewport');
      const body = document.querySelector('.right-panel-body-web');
      const chrome = document.querySelector('.browser-chrome.panel');
      const area = window.layoutFixture.area();
      if (!viewport || !body || !chrome || !area.visible) return null;
      const box = element => { const r = element.getBoundingClientRect(); return { left:r.left, top:r.top, width:r.width, height:r.height, bottom:r.bottom }; };
      const rect = box(viewport);
      if (['left','top','width','height'].some(key => Math.abs(rect[key] - area.area[key]) > 1)) return null;
      return { viewport:rect, body:box(body), chrome:box(chrome), gap:parseFloat(getComputedStyle(chrome).marginBottom), area:area.area };
    `), "measured browser viewport");
    assert.ok(metrics.viewport.height > 200, `Page viewport collapsed: ${JSON.stringify(metrics)}`);
    assert.ok(Math.abs(metrics.viewport.bottom - metrics.body.bottom) <= 2, `Page must fill the panel vertically: ${JSON.stringify(metrics)}`);
    assert.ok(Math.abs(metrics.viewport.width - metrics.body.width) <= 2, `Page must fill the panel horizontally: ${JSON.stringify(metrics)}`);
    assert.ok(Math.abs(metrics.viewport.top - metrics.chrome.bottom - metrics.gap) <= 2, "Page must follow its controls without extra gaps");
    return metrics;
  }

  try {
    let baseUrl = process.env.BROWSER_LAYOUT_URL;
    if (!baseUrl) {
      const { createServer } = await import("vite");
      vite = await createServer({ server: { host: "127.0.0.1", port: 0 }, appType: "spa" });
      await vite.listen();
      baseUrl = `http://127.0.0.1:${vite.httpServer.address().port}`;
    }
    const noProxy = [process.env.no_proxy ?? process.env.NO_PROXY, new URL(baseUrl).hostname].filter(Boolean).join(",");
    driver = spawn(process.env.WEBKIT_WEBDRIVER ?? "WebKitWebDriver", [`--port=${port}`], {
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, no_proxy: noProxy, NO_PROXY: noProxy, XDG_DATA_HOME: path.join(profile, "data"), XDG_CACHE_HOME: path.join(profile, "cache"), XDG_CONFIG_HOME: path.join(profile, "config") },
    });
    driver.on("error", (error) => { driverError = error; });
    driver.stdout.on("data", (chunk) => { driverLog += chunk; });
    driver.stderr.on("data", (chunk) => { driverLog += chunk; });
    await waitFor(async () => {
      if (driverError) throw new Error(`Install webkit2gtk-driver and run with xvfb-run: ${driverError.message}`);
      try { return await request("GET", "/status"); } catch { return false; }
    }, "WebKitWebDriver");
    session = (await request("POST", "/session", { capabilities: { alwaysMatch: {} } })).sessionId;
    await wd("POST", "/window/rect", { width: 1908, height: 1044 });
    await wd("POST", "/url", { url: `${baseUrl}/test/fixtures/browser-panel-layout.html` });
    await waitFor(async () => {
      const state = await evaluate("return window.layoutFixture && { ready:window.layoutFixture.ready, error:window.layoutFixture.error }");
      if (state?.error) throw new Error(state.error);
      return state?.ready;
    }, "page tab in the app shell");

    await t.test("the unified page tab fills a tall docked panel", async () => {
      const metrics = await checkViewport();
      t.diagnostic(`Docked viewport ${metrics.viewport.width}×${metrics.viewport.height}`);
    });
    await t.test("maximized pages use the full panel width", async () => {
      await click('button[aria-label="全面表示"]');
      await waitFor(() => evaluate("return !!document.querySelector('.right-panel.maximized')"), "maximize");
      await checkViewport();
      await click('button[aria-label="全面表示をやめる"]');
    });
    await t.test("resizing keeps the reported area equal to the displayed viewport", async () => {
      for (const [width, height] of [[1440, 900], [1100, 700]]) {
        await wd("POST", "/window/rect", { width, height });
        await checkViewport();
      }
    });
    await t.test("switching away hides the page and switching back restores its height", async () => {
      await wd("POST", "/window/rect", { width: 1908, height: 1044 });
      await checkViewport();
      await click('.right-panel-tabs [role="tab"]');
      await waitFor(() => evaluate("return !window.layoutFixture.area().visible"), "hide browser page");
      const pageId = await evaluate("return window.layoutFixture.pageId");
      await click(`#right-panel-tab-web-${pageId}`);
      await checkViewport();
    });
  } catch (error) {
    if (session) {
      try { t.diagnostic(JSON.stringify(await evaluate("return { url:location.href, title:document.title, text:document.body.innerText.slice(0,1000) }"))); } catch {}
    }
    t.diagnostic(driverLog);
    throw error;
  } finally {
    if (session) { try { await wd("DELETE", ""); } catch {} }
    driver?.kill();
    await vite?.close();
    await rm(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
    if (driverError) t.diagnostic(driverLog);
  }
});
