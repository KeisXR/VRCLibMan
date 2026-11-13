import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

async function withModules(paths, run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(...(await Promise.all(paths.map((path) => server.ssrLoadModule(path)))));
  } finally {
    await server.close();
  }
}

test("setup screens share one step count", async () => {
  await withModules(["/src/pages/SetupWizard.tsx"], ({ setupStepLabel, SETUP_STEPS }) => {
    assert.equal(SETUP_STEPS.length, 7);
    assert.equal(setupStepLabel("extension"), "初回セットアップ · 1 / 7");
    assert.equal(setupStepLabel("library"), "初回セットアップ · 2 / 7");
    assert.equal(setupStepLabel("presence"), "初回セットアップ · 7 / 7");
  });
  const wizard = readFileSync("src/pages/SetupWizard.tsx", "utf8");
  assert.doesNotMatch(wizard, /使い方ガイド · \{current\.index\} \/ 5/, "no separate count of five");
});

test("the asset update log points to the settings when nothing is watched", async () => {
  await withModules(["/src/pages/AssetUpdateLogPage.tsx"], ({ assetUpdateEmptyText }) => {
    assert.equal(assetUpdateEmptyText(null).needsTargets, true);
    assert.equal(assetUpdateEmptyText({ rows: [{ enabled: false }] }).needsTargets, true);
    const watched = assetUpdateEmptyText({ rows: [{ enabled: true }, { enabled: true }] });
    assert.equal(watched.needsTargets, false);
    assert.match(watched.text, /確認対象 2 件/);
  });
  const page = readFileSync("src/pages/AssetUpdateLogPage.tsx", "utf8");
  assert.doesNotMatch(page, /以前の候補は上の一覧で確認できます/);
  assert.match(page, /<SettingsLink section="settings-asset-updates">/);
});

test("settings use the words people look for, and the old names still find them", () => {
  const nav = readFileSync("src/settingsNav.ts", "utf8");
  assert.match(nav, /label: "アセットの更新"/);
  assert.match(nav, /label: "アプリ情報とデータ"/);
  const settings = readFileSync("src/pages/SettingsPage.tsx", "utf8");
  assert.match(settings, /id="settings-help"/, "the guide and setup live under アプリ情報とデータ");
  assert.match(settings, /アバター詳細・分類の編集・プレビュー/, "the link-target help names where it applies");
  assert.doesNotMatch(settings, /アバター詳細と確認の商品リンク/);
  const updates = readFileSync("src/components/AssetUpdates.tsx", "utf8");
  assert.doesNotMatch(updates, /許可した商品|許可を解除/, "確認対象, not 許可");
  const exportRow = readFileSync("src/components/SyncExportSettings.tsx", "utf8");
  assert.match(exportRow, /取り込む機能はまだありません/);
  const index = readFileSync("src/appSearchIndex.ts", "utf8");
  for (const old of ["更新設定", "バージョン情報", "データ移行", "ヒーロー"]) assert.match(index, new RegExp(`"${old}"`), `old name kept as a keyword: ${old}`);
});

test("the built-in browser comes first and is confirmed in the app, not with an OS dialog", async () => {
  await withModules(["/src/components/BoothImportSettings.tsx"], ({ BUILTIN_BROWSER_CONFIRM }) => {
    assert.deepEqual(BUILTIN_BROWSER_CONFIRM.map((group) => group.title), ["保存するもの", "使う場面", "しないこと", "注意"]);
  });
  const source = readFileSync("src/components/BoothImportSettings.tsx", "utf8");
  assert.doesNotMatch(source, /window\.confirm\(BUILTIN_BROWSER_CONFIRM\)/);
  assert.ok(source.indexOf('"browser",\n              "アプリの内蔵ブラウザ"') < source.indexOf('"extension",\n              "Chrome / Edge の拡張機能"'), "the recommended choice is first");
});

test("the import screen keeps product management on its own and shows the pairing code", () => {
  const page = readFileSync("src/pages/ImportPage.tsx", "utf8");
  assert.match(page, /<h2 id="import-manage">商品の管理<\/h2>/);
  assert.ok(page.indexOf('id="import-exclusions"') > page.indexOf('id="import-existing-files"'), "management comes after the numbered steps");
  assert.match(page, /未分類の商品を見る/);
  assert.match(page, /id="import-pairing-code"/);
});

test("row actions read as buttons on product cards and purchase orders", () => {
  const detail = readFileSync("src/pages/AvatarDetailPage.tsx", "utf8");
  const row = detail.slice(detail.indexOf("function DownloadRow({"), detail.indexOf("\n}\n", detail.indexOf("function DownloadRow({")));
  assert.equal((row.match(/className="button secondary"/g) ?? []).length, 5, "every file-row action is a secondary button");
  const purchases = readFileSync("src/pages/PurchasesPage.tsx", "utf8");
  const menu = purchases.slice(purchases.indexOf('<details className="purchase-order-menu">'), purchases.indexOf("</details>", purchases.indexOf('<details className="purchase-order-menu">')));
  assert.match(menu, /purchase-gift-select/, "the gift choice is folded into the order menu");
  const css = readFileSync("src/styles.css", "utf8");
  assert.match(css, /\.purchase-order-menu > summary \{ list-style: none; padding: 3px 10px; border-radius: var\(--radius-sm\)/);
});
