import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import test from "node:test";
import { createContext, runInContext } from "node:vm";
import { parseHTML } from "linkedom";

const port = "17321";
const rust = readFileSync("src-tauri/src/local_import.rs", "utf8");
const popup = readFileSync("extension/popup.js", "utf8");
const manifest = JSON.parse(readFileSync("extension/manifest.json", "utf8"));
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const agents = readFileSync("AGENTS.md", "utf8");
const extract = createRequire(import.meta.url)("../extension/extract.js");

async function popupHarness() {
  const { window } = parseHTML(readFileSync("extension/popup.html", "utf8"));
  const stored = {};
  const handlers = new Map();
  for (const element of window.document.querySelectorAll("[id]")) {
    element.addEventListener = (event, handler) => handlers.set(`${element.id}:${event}`, handler);
  }
  let request = async () => { throw new Error("not connected"); };
  const context = createContext({
    document: window.document,
    DOMParser: window.DOMParser,
    window: { confirm: () => true },
    URL, Blob, setTimeout,
    fetch: (...args) => request(...args),
    chrome: {
      runtime: { getManifest: () => manifest },
      storage: { local: {
        get: async (defaults) => typeof defaults === "string"
          ? { [defaults]: stored[defaults] }
          : { ...defaults, ...stored },
        set: async (values) => Object.assign(stored, values),
      } },
      tabs: { query: async () => [{ id: 1, url: "https://accounts.booth.pm/library" }] },
    },
    VrcLibManExtract: extract,
    VrcLibManRequestPolicy: {
      DEFAULT_INTERVAL_MS: 1000,
      normalizeIntervalMs: (value) => [250, 500, 1000].includes(Number(value)) ? Number(value) : 1000,
      createPacer: () => async () => {},
      isThrottled: (value) => value === 429 || value === 503,
    },
  });
  runInContext(`${popup}\nglobalThis.popupUnderTest = { refreshLink, sendLibrary };`, context);
  await context.popupUnderTest.refreshLink();
  return {
    api: context.popupUnderTest,
    document: window.document,
    stored,
    handlers,
    context,
    setFetch: (handler) => { request = handler; },
  };
}

test("extension and app share the fixed loopback port", () => {
  assert.match(rust, new RegExp(`LOCAL_IMPORT_PORT: u16 = ${port}`));
  assert.match(rust, /127\.0\.0\.1:\{LOCAL_IMPORT_PORT\}/);
  assert.match(popup, new RegExp(`http://127\\.0\\.0\\.1:\\$\\{LOCAL_PORT\\}`));
  assert.equal(popup.match(/const LOCAL_PORT = (\d+)/)[1], port);
  assert.deepEqual(manifest.host_permissions.filter((host) => host.startsWith("http://127.0.0.1")), [
    `http://127.0.0.1:${port}/*`,
  ]);
});

test("popup sends the export to the app and can wake it", () => {
  assert.match(popup, /credentials:\s*"omit"/);
  assert.match(popup, /vrclibman:\/\/import/);
  assert.match(popup, /https:\/\/accounts\.booth\.pm\/library/);
  assert.equal(popup.includes("chrome.cookies"), false);
  assert.equal(manifest.permissions.includes("cookies"), false);
  assert.match(manifest.version, /^\d+\.\d+\.\d+$/);
  assert.match(agents, /拡張の版は `extension\/manifest\.json` を正本/);
  assert.deepEqual(config.plugins["deep-link"].desktop.schemes, ["vrclibman"]);
});

test("popup distinguishes authenticated connections, rejected codes, and unavailable apps", async () => {
  const harness = await popupHarness();
  harness.stored.pairingToken = "TESTCODE";
  for (const [status, expected] of [
    [200, /アプリに接続しています/],
    [401, /コードが一致しません/],
    [403, /接続を許可されていません/],
    [429, /少し待って/],
    [null, /アプリに接続できません/],
  ]) {
    harness.setFetch(async (url) => {
      if (url.endsWith("/route-downloads")) return { ok: true, json: async () => ({ enabled: false }) };
      if (status === null) throw new Error("connection refused");
      return { status, ok: status === 200 };
    });
    await harness.api.refreshLink();
    assert.match(harness.document.querySelector("#link-status").textContent, expected);
    assert.equal(harness.document.querySelector("#connection-settings").open, status !== 200);
  }
});

test("routing connection failures keep the local choice without claiming the app is stopped", async () => {
  const harness = await popupHarness();
  harness.stored.pairingToken = "TESTCODE";
  harness.document.querySelector("#route-downloads").checked = true;
  await harness.handlers.get("route-downloads:change")();
  assert.equal(harness.stored.routeDownloads, true);
  assert.match(harness.document.querySelector("#status").textContent, /接続できないため.*設定を保存/);
  assert.doesNotMatch(harness.document.querySelector("#status").textContent, /起動していない/);
});

test("new-purchase imports report only pages actually read and keep file metadata in details", async () => {
  const harness = await popupHarness();
  harness.stored.pairingToken = "TESTCODE";
  const product = (boothId) => ({ boothId, title: "架空の服", url: `https://booth.pm/items/${boothId}`, shop: "架空の工房", descriptionSnippet: "説明".repeat(150) });
  harness.context.VrcLibManExtract = { ...extract, extractPage: () => [product("9003"), product("9001")] };
  const libraryRequests = [];
  let imported;
  harness.setFetch(async (url, options) => {
    if (url.endsWith("/known-ids")) return { ok: true, json: async () => ({ ids: ["9001"] }) };
    if (url.startsWith("https://accounts.booth.pm/library")) {
      libraryRequests.push(url);
      return { ok: true, status: 200, text: async () => '<a href="?page=10">10</a>' };
    }
    if (url.endsWith("/import")) {
      imported = JSON.parse(options.body);
      return { ok: true, json: async () => ({ inserted: 1, updated: 0, unchanged: 0, variations: 2, downloads: 3, warnings: ["一部の情報を確認してください"] }) };
    }
    if (url.endsWith("/health")) return { status: 200, ok: true };
    if (url.endsWith("/route-downloads")) return { ok: true, json: async () => ({ enabled: false }) };
    throw new Error(`unexpected request: ${url}`);
  });
  await harness.api.sendLibrary(true);
  assert.equal(libraryRequests.length, 1);
  assert.deepEqual(imported.products.map((item) => item.boothId), ["9003"]);
  const status = harness.document.querySelector("#status").textContent;
  assert.match(status, /1 ページを確認/);
  assert.doesNotMatch(status, /10 ページ|ダウンロード|バリエーション/);
  assert.match(status, /追加 1 件、更新 0 件/);
  assert.match(status, /一部の情報を確認してください/);
  assert.match(harness.document.querySelector("#result-breakdown").textContent, /ファイル情報 3 件/);
  assert.equal(harness.document.querySelector("#result-details").hidden, false);
});
