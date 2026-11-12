import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const css = readFileSync(join(root, "src/styles.css"), "utf8");
const component = readFileSync(join(root, "src/components/AssetUpdates.tsx"), "utf8");
const avatarDetail = readFileSync(join(root, "src/pages/AvatarDetailPage.tsx"), "utf8");

async function withModule(path, run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(await server.ssrLoadModule(path));
  } finally {
    await server.close();
  }
}

const status = (rows, extra = {}) => ({
  automatic: false,
  allProducts: false,
  totalProducts: rows.length,
  running: false,
  rows,
  ...extra,
});

const row = (id, opts = {}) => ({
  revision: 1,
  boothId: id,
  title: `商品 ${id}`,
  enabled: true,
  checkedAt: "2026-11-05T00:00:00.000Z",
  error: null,
  changes: [],
  ...opts,
});

test("asset update list starts collapsed", async () => {
  await withModule("/src/assetUpdatesUi.ts", (m) => {
    assert.equal(m.assetUpdateListStartsOpen(0), false);
    assert.equal(m.assetUpdateListStartsOpen(1), false);
    assert.equal(m.assetUpdateListStartsOpen(50), false);
  });
});

test("summary always shows counts and state", async () => {
  await withModule("/src/assetUpdatesUi.ts", (m) => {
    assert.equal(m.assetUpdateListSummary(null), "読み込み中…");
    assert.equal(m.assetUpdateListSummary(status([], { running: true })), "確認中…");
    assert.equal(m.assetUpdateListSummary(status([])), "確認を許可した商品はありません");
    assert.match(
      m.assetUpdateListSummary(status([row("1"), row("2"), row("3", { changes: ["商品説明の変更"] }), row("4", { error: "失敗", enabled: false })])),
      /確認対象 3 件/,
    );
    assert.match(
      m.assetUpdateListSummary(status([row("1"), row("2"), row("3", { changes: ["商品説明の変更"] }), row("4", { error: "失敗", enabled: false })])),
      /更新 1 件/,
    );
    assert.match(
      m.assetUpdateListSummary(status([row("1"), row("2"), row("3", { changes: ["商品説明の変更"] }), row("4", { error: "失敗", enabled: false })])),
      /確認失敗 1 件/,
    );
    assert.match(m.assetUpdateListSummary(status([row("1"), row("2")])), /更新なし/);
  });
});

test("component defaults to collapsed list with toggle", () => {
  assert.match(component, /assetUpdateListStartsOpen/);
  assert.match(component, /aria-expanded=\{listOpen\}/);
  assert.match(component, /一覧を表示/);
  assert.match(component, /一覧を閉じる/);
  assert.match(component, /assetUpdateListSummary/);
  assert.match(component, /listOpen &&/);
});

test("expanded list scrolls inside max-height; title ellipsizes; motion is opacity/transform only", () => {
  assert.match(css, /\.asset-update-list\s*\{[^}]*max-height:\s*min\(40vh,\s*360px\)/s);
  assert.match(css, /\.asset-update-list\s*\{[^}]*overflow:\s*auto/s);
  assert.match(css, /\.asset-update-title\s*\{[^}]*text-overflow:\s*ellipsis/s);
  assert.match(css, /\.asset-update-panel\s*\{[^}]*animation:\s*switch-rise\s+var\(--motion\)/s);
  assert.doesNotMatch(css, /\.asset-update-(panel|list|summary)[^}]*transition:[^;}]*(width|height|max-height)/);
});

test("product card shows new-file badge and folds superseded missing files", () => {
  assert.match(avatarDetail, /file\.isNew \? <span className="badge new-file">新しいファイル<\/span> : null/);
  assert.match(avatarDetail, /以前のファイル \{superseded\.length\}件/);
  assert.match(component, /内蔵ブラウザが有効なら BOOTH ライブラリから最新のファイル情報も読み直して商品カードへ反映/);
});
