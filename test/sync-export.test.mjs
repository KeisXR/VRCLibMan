import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

const date = new Date(2026, 10, 5, 23, 59);

async function load() {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    return await server.ssrLoadModule("/src/syncExport.ts");
  } finally {
    await server.close();
  }
}

test("the default file name holds the device name and the local date", async () => {
  const { syncExportFilename } = await load();
  assert.equal(syncExportFilename("kei-desk-2", date), "vrclibman-migration-kei-desk-2-20261105.zip");
  assert.equal(syncExportFilename("kei", new Date(2027, 0, 2, 0, 1)), "vrclibman-migration-kei-20270102.zip");
});

test("characters Windows cannot store become a hyphen", async () => {
  const { syncExportFilename } = await load();
  assert.equal(syncExportFilename("A/B:C", date), "vrclibman-migration-A-B-C-20261105.zip");
  assert.equal(syncExportFilename('a\\b*c?d"e<f>g|h\ti', date), "vrclibman-migration-a-b-c-d-e-f-g-h-i-20261105.zip");
  assert.equal(syncExportFilename("居間の PC", date), "vrclibman-migration-居間の PC-20261105.zip");
});

test("an empty device name falls back to device", async () => {
  const { syncExportFilename } = await load();
  assert.equal(syncExportFilename("", date), "vrclibman-migration-device-20261105.zip");
  assert.equal(syncExportFilename("   ", date), "vrclibman-migration-device-20261105.zip");
});

test("both export commands refuse every window except main", () => {
  const commands = readFileSync(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8");
  for (const name of ["sync_export_json", "save_sync_export"]) {
    const start = commands.indexOf(`fn ${name}(`);
    assert.notEqual(start, -1, name);
    const body = commands.slice(start, commands.indexOf("#[tauri::command]", start));
    assert.match(body, /if view\.label\(\) != "main" \{\s+return Err\("この画面からデータ移行のファイルは書き出せません"\.into\(\)\);/);
  }
});

test("the settings screen offers export only and states what the file holds", () => {
  const panel = readFileSync(new URL("../src/components/SyncExportSettings.tsx", import.meta.url), "utf8");
  const page = readFileSync(new URL("../src/pages/SettingsPage.tsx", import.meta.url), "utf8");
  assert.match(panel, />エクスポート</);
  assert.doesNotMatch(panel, /インポート/);
  assert.match(panel, /label="データ移行"/);
  assert.match(panel, /ZIP に書き出します/);
  assert.match(panel, /商品名、説明文、画像、ファイル名を含みます/);
  assert.match(page, /エクスポートしたファイルには商品名、ショップ名、説明文、サムネイル、ヒーロー画像、ファイル名が含まれます/);
});

test("the old name is gone, and the separate remote library sync keeps its own", () => {
  for (const file of ["src/api.ts", "src/types.ts", "src/components/SyncExportSettings.tsx", "src-tauri/src/commands.rs"]) {
    assert.doesNotMatch(readFileSync(new URL(`../${file}`, import.meta.url), "utf8"), /同期データ/, file);
  }
  const page = readFileSync(new URL("../src/pages/SettingsPage.tsx", import.meta.url), "utf8");
  assert.match(page, /label="リモートへ自動同期"/);
});

test("the save dialog asks for a zip", () => {
  const api = readFileSync(new URL("../src/api.ts", import.meta.url), "utf8");
  assert.match(api, /title: "データ移行のファイルをエクスポート"/);
  assert.match(api, /filters: \[\{ name: "ZIP", extensions: \["zip"\] \}\]/);
});

test("the preview manifest is version 4 and has no images to point at", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { dispatch } = await server.ssrLoadModule("/src/preview/api.ts");
    const manifest = JSON.parse(await dispatch("sync_export_json", {}));
    assert.equal(manifest.format, "vrclibman-sync");
    assert.equal(manifest.version, 4);
    assert.ok(manifest.products.length > 0);
    for (const product of manifest.products) {
      assert.equal(typeof product.shop, "string");
      assert.equal(typeof product.descriptionSnippet, "string");
      assert.equal(product.thumbnail, null);
    }
    assert.equal(manifest.counts.thumbnails, 0);
    assert.equal(manifest.counts.heroImages, 0);
    const report = await dispatch("save_sync_export", { path: "ignored.zip" });
    assert.deepEqual(report.counts, manifest.counts);
  } finally {
    await server.close();
  }
});

test("clicking export in the preview saves the manifest as JSON and reports every count", async () => {
  const { window } = parseHTML('<!doctype html><html><body><div id="root"></div></body></html>');
  for (const name of ["window", "document", "HTMLElement", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    await server.ssrLoadModule("/src/preview/api.ts");
    const { SyncExportSettings } = await server.ssrLoadModule("/src/components/SyncExportSettings.tsx");
    const downloads = [];
    const createAnchor = document.createElement.bind(document);
    document.createElement = (name, ...rest) => {
      const node = createAnchor(name, ...rest);
      if (name === "a") node.click = () => downloads.push(node.download);
      return node;
    };
    const notices = [];
    const failures = [];
    root = createRoot(document.getElementById("root"));
    await act(async () => {
      root.render(createElement(SyncExportSettings, {
        settings: { deviceName: "kei-desk-2" },
        onChange: () => {},
        onBlur: () => {},
        beforeExport: async () => null,
        onNotify: (message) => notices.push(message),
        onFail: (message) => failures.push(message),
      }));
    });
    assert.match(document.body.textContent, /データ移行/);
    assert.doesNotMatch(document.body.textContent, /同期データ/);
    const button = [...document.querySelectorAll("button")].find((node) => node.textContent === "エクスポート");
    await act(async () => {
      button.dispatchEvent(new window.Event("click", { bubbles: true }));
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    assert.deepEqual(failures, []);
    assert.equal(downloads.length, 1);
    assert.match(downloads[0], /^vrclibman-migration-kei-desk-2-\d{8}-manifest\.json$/);
    assert.equal(notices.length, 1);
    assert.match(notices[0], /サムネイル 0 件、ヒーロー画像 0 件/);
  } finally {
    if (root) await act(async () => root.unmount());
    await server.close();
  }
});

test("the manifest declares format version 4 and names only package paths", () => {
  const code = readFileSync(new URL("../src-tauri/src/sync_export.rs", import.meta.url), "utf8");
  const production = code.slice(0, code.indexOf("#[cfg(test)]"));
  assert.match(production, /const FORMAT: &str = "vrclibman-sync";/);
  assert.match(production, /const VERSION: u32 = 4;/);
  assert.match(production, /const MANIFEST_ENTRY: &str = "manifest\.json";/);
  assert.match(production, /const THUMBNAIL_ENTRIES: &str = "files\/thumbnails";/);
  assert.match(production, /const HERO_ENTRIES: &str = "files\/heroes";/);
  assert.doesNotMatch(production, /thumbnail_url/);
  const manifest = production.slice(production.indexOf("pub(crate) struct SyncDevice"), production.indexOf("pub(crate) struct PackageImages"));
  assert.doesNotMatch(manifest, /PathBuf/);
});
