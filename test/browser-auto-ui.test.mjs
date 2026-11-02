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

test("acquisition classification defers, retries failed saves, and classifies multiple products in Chromium", { timeout: 90000, skip: chromePath ? false : "Chrome not installed (set CHROME_PATH)" }, async (t) => {
  const { createServer } = await import("vite");
  const vite = await createServer({ server: { host: "127.0.0.1", port: 0, hmr: false }, appType: "spa", logLevel: "error" });
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
    await call(browserWs, "Target.activateTarget", { targetId });
    const { sessionId } = await call(browserWs, "Target.attachToTarget", { targetId, flatten: true });
    const page = (method, params) => call(browserWs, method, params, sessionId);
    const evaluate = async (expression) => (await page("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result.value;
    await page("Page.navigate", { url: `${baseUrl}/test/fixtures/browser-auto-classification.html` });
    for (let attempt = 0; ; attempt++) {
      const state = await evaluate("window.autoFixture && { ready: window.autoFixture.ready, error: window.autoFixture.error }").catch(() => null);
      if (state?.error) throw new Error(state.error);
      if (state?.ready && (await evaluate("document.querySelector('dialog')?.open"))) break;
      if (attempt > 500) {
        const debug = await evaluate("JSON.stringify({ ready: window.autoFixture && window.autoFixture.ready, href: location.href, text: document.body ? document.body.innerText.slice(0, 300) : '' })").catch((error) => String(error));
        throw new Error(`fixture did not load: ${debug}`);
      }
      await pause(50);
    }
    async function clickText(text) {
      const point = await evaluate(`(() => { const node = [...document.querySelectorAll('button')].find(n => n.textContent === ${JSON.stringify(text)}); const r = node.getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()`);
      await page('Input.dispatchMouseEvent', {type:'mousePressed',...point,button:'left',clickCount:1});
      await page('Input.dispatchMouseEvent', {type:'mouseReleased',...point,button:'left',clickCount:1});
      await pause(150);
    }
    async function waitFor(expression, label) {
      for (let attempt = 0; attempt < 100; attempt++) {
        if (await evaluate(expression)) return;
        await pause(50);
      }
      throw new Error(`${label}: ${await evaluate("document.body.innerText")}`);
    }
    async function pressEscape() {
      await page("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27 });
      await page("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27 });
    }
    async function reloadPending(next) {
      const before = await evaluate("window.autoFixture.fetches");
      await evaluate(`(() => { window.autoFixture.pending = ${JSON.stringify(next)}; window.autoFixture.bump(); })()`);
      await waitFor(`window.autoFixture.fetches > ${before}`, "pending list did not reload");
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

    const hair = { boothId: "345678", title: "見本の髪" };
    const texture = { boothId: "456789", title: "見本のテクスチャ" };
    const fresh = { boothId: "567890", title: "見本の新しい商品" };
    await reloadPending([hair, texture]);
    await waitFor("document.querySelector('dialog')?.open === true && document.querySelector('dialog p')?.textContent === '見本の髪'", "reloaded queue did not open");
    assert.equal(await evaluate("[...document.querySelectorAll('dialog button')].some(node => ['×', '閉じる'].includes(node.textContent.trim()))"), false);
    const backdrop = await evaluate("(() => { const r = document.querySelector('dialog').getBoundingClientRect(); const x = r.left > 30 ? 8 : Math.min(window.innerWidth - 8, r.right + 16); const y = r.top > 30 ? 8 : Math.min(window.innerHeight - 8, r.bottom + 16); return { x, y, outside: x < r.left || x > r.right || y < r.top || y > r.bottom }; })()");
    assert.equal(backdrop.outside, true, "the click point must be outside the dialog");
    await page("Input.dispatchMouseEvent", { type: "mousePressed", x: Math.round(backdrop.x), y: Math.round(backdrop.y), button: "left", clickCount: 1 });
    await page("Input.dispatchMouseEvent", { type: "mouseReleased", x: Math.round(backdrop.x), y: Math.round(backdrop.y), button: "left", clickCount: 1 });
    await pause(150);
    assert.equal(await evaluate("document.querySelector('dialog').open"), true, "backdrop click must leave the dialog open");
    assert.equal(await evaluate("document.querySelector('.browser-auto-resume') === null"), true);

    await pressEscape();
    await waitFor("document.querySelector('dialog').open === false && !!document.querySelector('.browser-auto-resume')", "Escape did not postpone");
    assert.equal(await evaluate("window.autoFixture.pending.length"), 2);
    assert.equal(await evaluate("window.autoFixture.saved.length"), 2);
    const fetches = await evaluate("window.autoFixture.fetches");
    await evaluate("window.autoFixture.bump()");
    await waitFor(`window.autoFixture.fetches > ${fetches}`, "same list did not reload");
    await pause(150);
    assert.equal(await evaluate("document.querySelector('dialog').open"), false, "the same pending list must stay postponed");
    assert.equal(await evaluate("document.querySelector('.browser-auto-resume').textContent"), "取得した商品の分類（2件）");

    await reloadPending([hair, texture, fresh]);
    await waitFor("document.querySelector('dialog')?.open === true && document.body.innerText.includes('残り3件')", "a new product after Escape did not reopen");
    assert.equal(await evaluate("document.querySelector('.browser-auto-resume') === null"), true);
    assert.equal(await evaluate("window.autoFixture.saved.length"), 2, "reopening must not classify on its own");
    await clickText("後で分類する");
    assert.equal(await evaluate("document.querySelector('dialog').open"), false);
    assert.equal(await evaluate("document.querySelector('.browser-auto-resume').textContent"), "取得した商品の分類（3件）");
    await reloadPending([hair, texture, fresh, { boothId: "678901", title: "見本の次の商品" }]);
    await waitFor("document.querySelector('dialog')?.open === true && document.body.innerText.includes('残り4件')", "a new product after 後で分類する did not reopen");

    await evaluate("window.autoFixture.holdSave = true");
    try {
      await clickText("分類を確定");
      await waitFor("document.body.innerText.includes('保存中')", "save did not start");
      await pressEscape();
      await pause(150);
      const openDuringSave = await evaluate("document.querySelector('dialog').open === true");
      const resumeDuringSave = await evaluate("!!document.querySelector('.browser-auto-resume')");
      assert.equal(openDuringSave || resumeDuringSave, true, "Escape during save must keep the dialog or show resume");
      if (openDuringSave) {
        await evaluate("document.querySelector('dialog').close()");
        await waitFor("document.querySelector('dialog').open === false && !!document.querySelector('.browser-auto-resume')", "closing during save hid the queue");
      }
      assert.equal(await evaluate("window.autoFixture.saved.length"), 2, "a held save must not finish before it is released");
    } finally {
      await evaluate("window.autoFixture.releaseSave()");
    }
  } finally {
    socket?.close();
    chrome.kill();
    await vite.close();
    await pause(200);
    await rm(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
});
