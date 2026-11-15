import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createContext, runInContext } from "node:vm";
import { parseHTML } from "linkedom";

const port = "17321";
const rust = readFileSync("src-tauri/src/local_import.rs", "utf8");
const shared = readFileSync("extension/shared.js", "utf8");
const background = readFileSync("extension/background.js", "utf8");
const manifest = JSON.parse(readFileSync("extension/manifest.json", "utf8"));
const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
const config = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"));
const agents = readFileSync("AGENTS.md", "utf8");

test("main window keeps HTML5 drag and drop for genre moves and JSON drops", () => {
  const main = config.app.windows.find((window) => window.label === "main");
  assert.equal(main.dragDropEnabled, false);
  const css = readFileSync("src/styles.css", "utf8");
  assert.match(css, /\.product-actions-panel \{[^}]*flex-direction:\s*column/);
  assert.match(css, /\.item > \.row-actions \{[^}]*align-self:\s*start/);
});

test("extension and app share the fixed loopback port", () => {
  assert.match(rust, new RegExp(`LOCAL_IMPORT_PORT: u16 = ${port}`));
  assert.match(rust, /127\.0\.0\.1:\{LOCAL_IMPORT_PORT\}/);
  assert.equal(shared.match(/const LOCAL_PORT = (\d+)/)[1], port);
  assert.match(shared, /http:\/\/127\.0\.0\.1:\$\{LOCAL_PORT\}/);
  assert.deepEqual(manifest.host_permissions.filter((host) => host.startsWith("http://127.0.0.1")), [
    `http://127.0.0.1:${port}/*`,
  ]);
});

test("the extension wakes the app, never reads cookies and carries the app's version", () => {
  assert.match(background, /credentials: "omit"/);
  assert.match(shared, /vrclibman:\/\/import/);
  assert.match(shared, /https:\/\/accounts\.booth\.pm\/library/);
  for (const file of ["background.js", "content.js", "popup.js", "offscreen.js", "walker.js", "options.js", "sidepanel.js"]) {
    assert.equal(readFileSync(`extension/${file}`, "utf8").includes("chrome.cookies"), false, file);
  }
  assert.equal(manifest.permissions.includes("cookies"), false);
  assert.equal(manifest.version, packageJson.version, "the extension shares the app's version number");
  assert.match(agents, /拡張の版はアプリの版と同じ番号/);
  assert.deepEqual(config.plugins["deep-link"].desktop.schemes, ["vrclibman"]);
  assert.equal(manifest.name, "VRCLibMan");
  for (const size of ["16", "32", "48", "128"]) assert.equal(manifest.icons[size], `icons/icon-${size}.png`);
});

// The real popup script against a browser DOM and an in-memory extension runtime.
test("the manifest key fixes the extension ID the app accepts, and the app ships the extension", () => {
  const der = Buffer.from(manifest.key, "base64");
  const id = createHash("sha256").update(der).digest("hex").slice(0, 32).replace(/[0-9a-f]/g, (digit) => "abcdefghijklmnop"[parseInt(digit, 16)]);
  assert.equal(rust.match(/OFFICIAL_EXTENSION_ID: &str = "([a-p]{32})"/)[1], id);
  assert.equal(config.bundle.resources["../extension/"], "extension/");
  const packaging = readFileSync("src-tauri/src/extension_package.rs", "utf8");
  assert.match(packaging, /"package\.json"/, "development files stay out of the managed folder");
});

async function popupHarness({ stored = {}, tab = { id: 1, url: "https://booth.pm/ja/items/123456", windowId: 1 }, connection = { hasToken: true, status: 200 }, products = [], statuses = [], job = null, pageMissing = false } = {}) {
  const { window } = parseHTML(readFileSync("extension/popup.html", "utf8"));
  const sent = [];
  const reloaded = [];
  const local = { pairingToken: "ABCD-2345", routeDownloads: true, boothIntervalMs: 1000, pendingDeliveries: [], importHistory: [], ...stored };
  const context = createContext({
    document: window.document,
    Node: window.Node,
    window: { close() {} },
    URL,
    Date,
    setTimeout,
    chrome: {
      runtime: {
        getURL: (path) => `chrome-extension://test/${path}`,
        openOptionsPage() {},
        async sendMessage(message) {
          sent.push(message);
          if (message.type === "connection") return connection;
          if (message.type === "product-status") return { products: statuses };
          if (message.type === "job-start") return { ok: true, id: "job" };
          return { ok: true };
        },
      },
      storage: {
        onChanged: { addListener() {} },
        local: { get: async (defaults) => ({ ...defaults, ...local }), set: async (values) => Object.assign(local, values) },
        session: { get: async () => ({ job }) },
      },
      tabs: {
        query: async () => [tab],
        async sendMessage() {
          if (pageMissing) throw new Error("Could not establish connection. Receiving end does not exist.");
          return { products };
        },
        reload: (id) => reloaded.push(id),
        create() {},
      },
    },
  });
  for (const file of ["shared.js", "companion.js", "ui.js", "popup.js"]) {
    runInContext(readFileSync(`extension/${file}`, "utf8"), context, { filename: file });
  }
  for (let i = 0; i < 20; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
  const main = window.document.querySelector("#main");
  return {
    document: window.document,
    sent,
    local,
    reloaded,
    text: () => main.textContent,
    buttons: () => [...main.querySelectorAll("button")].map((button) => button.textContent),
    chip: () => window.document.querySelector("#link-chip").textContent,
  };
}

test("without a code the popup only offers pairing; a rejected code says how to fix it", async () => {
  const unpaired = await popupHarness({ stored: { pairingToken: "" }, connection: { hasToken: false, status: null } });
  assert.equal(unpaired.chip(), "未接続");
  assert.match(unpaired.text(), /アプリとつなぐ/);
  assert.deepEqual(unpaired.buttons(), ["接続する"]);
  assert.equal(unpaired.document.querySelector("#footer").hidden, true);
  const rejected = await popupHarness({ connection: { hasToken: true, status: 401 } });
  assert.match(rejected.text(), /コードが一致しません/);
  assert.equal(rejected.document.querySelector("input").value, "ABCD-2345");
});

test("on an item page the popup shows the product's state, one send action and the page's files", async () => {
  const product = { boothId: "123456", title: "見本の衣装", downloads: [
    { downloadableId: "555555", filename: "sample_a.zip", variation: "A用" },
    { downloadableId: "666666", filename: "sample_b.zip", variation: "B用" },
    { downloadableId: "777777", filename: "readme.pdf", variation: "" },
  ] };
  const h = await popupHarness({
    products: [product],
    statuses: [{ boothId: "123456", registered: true, owned: true, localRecorded: true, filesKnown: true, savedFiles: ["666666"], avatars: [{ id: "a", name: "見本アバター", viaBaseBody: false }] }],
  });
  assert.equal(h.chip(), "接続中");
  assert.match(h.text(), /見本の衣装/);
  assert.match(h.text(), /所持.*保存記録あり.*見本アバター/);
  assert.deepEqual(h.buttons(), ["この商品を送る", "取得"], "the saved file and non-asset files have no button");
  assert.match(h.text(), /sample_b\.zip/);
  assert.doesNotMatch(h.text(), /readme\.pdf/);
  h.document.querySelector("#main button").dispatchEvent(new h.document.defaultView.Event("click"));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.deepEqual(structuredClone(h.sent.find((message) => message.type === "job-start")), { type: "job-start", kind: "page", tabId: 1 });
});

test("on a tab opened before the extension was installed or updated, the popup says why and offers a reload", async () => {
  const h = await popupHarness({ pageMissing: true });
  assert.match(h.text(), /拡張機能を入れる前か更新する前に開いた/);
  assert.deepEqual(h.buttons(), ["ページを再読み込み"]);
  h.document.querySelector("#main button").dispatchEvent(new h.document.defaultView.Event("click"));
  assert.deepEqual(h.reloaded, [1]);
  const empty = await popupHarness();
  assert.match(empty.text(), /このページから商品を見つけられませんでした/);
});

test("while a walk runs, the popup shows its progress with cancel and side panel actions", async () => {
  const h = await popupHarness({
    tab: { id: 1, url: "https://accounts.booth.pm/library", windowId: 1 },
    job: { id: "job", kind: "library", label: "購入ライブラリを読み込み中", state: "running", line: "購入ライブラリ 4 / 12 ページ", current: 4, total: 12, count: 182 },
  });
  assert.match(h.text(), /購入ライブラリを読み込み中/);
  assert.match(h.text(), /182 件/);
  assert.match(h.text(), /閉じても続きます/);
  assert.deepEqual(h.buttons(), ["サイドパネルで見る", "中止"]);
  assert.equal(h.document.querySelector("[role=progressbar]").getAttribute("aria-valuenow"), "33");
});

test("on the library the popup offers new purchases first, then the full read and the purchase history", async () => {
  const h = await popupHarness({
    tab: { id: 1, url: "https://accounts.booth.pm/library/gifts", windowId: 1 },
    stored: { importHistory: [{ at: new Date().toISOString(), kind: "library-new", inserted: 3 }], pendingDeliveries: [{ id: "x" }] },
  });
  assert.match(h.text(), /ギフト/);
  assert.match(h.text(), /最後の取り込み：今日 .*（追加 3 件）/);
  assert.deepEqual(h.buttons(), ["新しい購入を送る", "すべて読み込む", "購入履歴を読み込む"]);
  assert.equal(h.document.querySelector("#pending").hidden, false);
  assert.equal(h.document.querySelector("#pending-count").textContent, "1");
  const other = await popupHarness({ tab: { id: 2, url: "https://example.com/", windowId: 1 } });
  assert.deepEqual(other.buttons(), ["購入ライブラリを開く"]);
});
