import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
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
  assert.equal(syncExportFilename("kei-desk-2", date), "vrclibman-export-kei-desk-2-20261105.json");
  assert.equal(syncExportFilename("kei", new Date(2027, 0, 2, 0, 1)), "vrclibman-export-kei-20270102.json");
});

test("characters Windows cannot store become a hyphen", async () => {
  const { syncExportFilename } = await load();
  assert.equal(syncExportFilename("A/B:C", date), "vrclibman-export-A-B-C-20261105.json");
  assert.equal(syncExportFilename('a\\b*c?d"e<f>g|h\ti', date), "vrclibman-export-a-b-c-d-e-f-g-h-i-20261105.json");
  assert.equal(syncExportFilename("居間の PC", date), "vrclibman-export-居間の PC-20261105.json");
});

test("an empty device name falls back to device", async () => {
  const { syncExportFilename } = await load();
  assert.equal(syncExportFilename("", date), "vrclibman-export-device-20261105.json");
  assert.equal(syncExportFilename("   ", date), "vrclibman-export-device-20261105.json");
});

test("both export commands refuse every window except main", () => {
  const commands = readFileSync(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8");
  for (const name of ["sync_export_json", "save_sync_export"]) {
    const start = commands.indexOf(`fn ${name}(`);
    assert.notEqual(start, -1, name);
    const body = commands.slice(start, commands.indexOf("#[tauri::command]", start));
    assert.match(body, /if view\.label\(\) != "main" \{\s+return Err\("この画面から同期データは書き出せません"\.into\(\)\);/);
  }
});

test("the settings screen offers export only and states what the file holds", () => {
  const panel = readFileSync(new URL("../src/components/SyncExportSettings.tsx", import.meta.url), "utf8");
  const page = readFileSync(new URL("../src/pages/SettingsPage.tsx", import.meta.url), "utf8");
  assert.match(panel, />エクスポート</);
  assert.doesNotMatch(panel, /インポート/);
  assert.match(panel, /商品名とファイル名を含みます/);
  assert.match(page, /エクスポートしたファイルには商品名とファイル名が含まれます/);
});
