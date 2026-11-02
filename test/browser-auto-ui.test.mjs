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

test("acquisition classification defers, retries failed saves, and classifies multiple products in Chromium", { timeout: 60000, skip: chromePath ? false : "Chrome not installed (set CHROME_PATH)" }, async (t) => {
  const { createServer } = await import("vite");
  const vite = await createServer({ server: { host: "127.0.0.1", port: 0 }, appType: "spa", logLevel: "error" });
  await vite.listen();
  const baseUrl = `http://127.0.0.1:${vite.httpServer.address().port}`;
  const profile = await mkdtemp(path.join(os.tmpdir(), "vrclibman-auto-classification-"));
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
    await page("Page.navigate", { url: `${baseUrl}/test/fixtures/browser-auto-classification.html` });
    for (let attempt = 0; ; attempt++) {
      const state = await evaluate("window.autoFixture && { ready: window.autoFixture.ready, error: window.autoFixture.error }").catch(() => null);
      if (state?.error) throw new Error(state.error);
      if (state?.ready && (await evaluate("document.querySelector('dialog')?.open"))) break;
      if (attempt > 200) throw new Error("fixture did not load");
      await pause(50);
    }
    async function clickText(text) {
      const point = await evaluate(`(() => { const node = [...document.querySelectorAll('button')].find(n => n.textContent === ${JSON.stringify(text)}); const r = node.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()`);
      await page('Input.dispatchMouseEvent', {type:'mousePressed',...point,button:'left',clickCount:1});
      await page('Input.dispatchMouseEvent', {type:'mouseReleased',...point,button:'left',clickCount:1});
      await pause(150);
    }
    assert.equal(await evaluate("document.querySelector('dialog').open"), true);
    await clickText('後で分類する');
    assert.equal(await evaluate("document.querySelector('dialog').open"), false);
    assert.equal(await evaluate("window.autoFixture.pending.length"), 2);
    await clickText('取得した商品の分類（2件）');
    assert.equal(await evaluate("document.querySelector('dialog').open"), true);
    await evaluate('window.autoFixture.failNext=true');
    await clickText('分類を確定');
    assert.equal(await evaluate('window.autoFixture.pending.length'), 2);
    assert.equal(await evaluate('window.autoFixture.lastError'), '保存失敗の見本');
    assert.equal(await evaluate("document.querySelector('dialog [role=alert]').textContent"), '保存失敗の見本');
    assert.equal(await evaluate("document.querySelector('dialog').open"), true);
    await clickText('分類を確定');
    assert.equal(await evaluate('window.autoFixture.pending.length'), 1);
    assert.equal(await evaluate("document.querySelector('dialog p').textContent"), '見本のワールド');
    await evaluate("(() => { const s = document.querySelector('dialog select'); s.value='world'; s.dispatchEvent(new Event('change',{bubbles:true})); })()");
    await pause(150);
    await clickText('分類を確定');
    assert.equal(await evaluate('window.autoFixture.pending.length'), 0);
    assert.equal(await evaluate("document.querySelector('dialog').open"), false);
    assert.deepEqual(await evaluate('window.autoFixture.saved'), [{id:'123456',kind:'asset',genre:'outfit'}, {id:'234567',kind:'world',genre:'world'}]);
  } finally {
    socket?.close();
    chrome.kill();
    await vite.close();
    await pause(200);
    await rm(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
});
