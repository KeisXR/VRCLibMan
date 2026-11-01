import { spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

export const chromePath = process.env.CHROME_PATH ?? ["/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser"].find((file) => process.getBuiltinModule("node:fs").existsSync(file));
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export async function withChromium(run) {
  const { createServer } = await import("vite");
  const vite = await createServer({ server: { host: "127.0.0.1", port: 0 }, appType: "spa", logLevel: "error" });
  await vite.listen();
  const baseUrl = `http://127.0.0.1:${vite.httpServer.address().port}`;
  const profile = await mkdtemp(path.join(os.tmpdir(), "vrclibman-design-"));
  const chrome = spawn(chromePath, ["--headless=new", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  let socket;
  try {
    let log = "";
    const wsUrl = await new Promise((resolve, reject) => {
      chrome.on("error", reject);
      chrome.on("exit", () => reject(new Error(`Chrome exited: ${log}`)));
      chrome.stderr.on("data", (chunk) => {
        log += chunk;
        const match = /DevTools listening on (ws:\/\/\S+)/.exec(log);
        if (match) resolve(match[1]);
      });
    });
    socket = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      socket.addEventListener("open", resolve, { once: true });
      socket.addEventListener("error", reject, { once: true });
    });
    let id = 0;
    const pending = new Map();
    const call = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const message = { id: ++id, method, params, ...(sessionId ? { sessionId } : {}) };
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
    const waitFor = async (expression) => {
      for (let attempt = 0; attempt < 200; attempt++) {
        if (await evaluate(`Boolean(${expression})`)) return;
        await pause(25);
      }
      throw new Error(`DOM did not settle: ${expression}`);
    };
    await run({ baseUrl, page, evaluate, waitFor, pause });
  } finally {
    socket?.close();
    chrome.kill();
    await vite.close();
    await pause(200);
    await rm(profile, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
}
