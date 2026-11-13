import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

// Dummy data only.
const file = { boothId: "1234567", variation: "", filename: "dummy.zip", sizeBytes: null, state: "missing" };

test("無料配布のファイルを探す: shown only for an item with no file info while the built-in browser is on", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const free = await server.ssrLoadModule("/src/freeFiles.ts");
    assert.equal(free.offerFreeFileCheck({ downloads: [], localFileCount: 0 }, true), true);
    assert.equal(free.offerFreeFileCheck({ downloads: [], localFileCount: 0 }, false), false);
    assert.equal(free.offerFreeFileCheck({ downloads: [file], localFileCount: 0 }, true), false);
    assert.equal(free.offerFreeFileCheck({ downloads: [], localFileCount: 2 }, true), false);
    assert.equal(free.offerFreeFileCheck({ downloads: [], localFileCount: 0, unpublished: true }, true), false);
    // ダウンロードと取り違えない名前にする（押してもファイル行を足すだけ）。
    assert.equal(free.FREE_FILE_CHECK_LABEL, "無料配布のファイルを探す");
    assert.equal(free.FREE_FILE_CHECK_BUSY_LABEL, "探しています…");
    assert.equal(free.FREE_FILE_CHECK_HINT, undefined);
    assert.match(free.freeFilesFoundMessage(2), /2 件.*ログイン/);
    assert.match(free.freeFilesFoundMessage(0), /ありません/);
    // 有料・一覧なしは Rust の理由を通常の通知で出す（失敗の帯にしない）。
    assert.equal(free.freeFilesResultMessage({ files: 0, notice: "無料配布のファイルはありませんでした" }), "無料配布のファイルはありませんでした");
    assert.match(free.freeFilesResultMessage({ files: 2 }), /2 件/);
  } finally {
    await server.close();
  }
});

test("無料配布のファイルを探す: the card calls only the check command and never starts a download by itself", () => {
  const page = readFileSync(new URL("../src/pages/AvatarDetailPage.tsx", import.meta.url), "utf8").replace(/\r\n/g, "\n");
  const start = page.indexOf("async function checkFreeFiles");
  assert.ok(start > 0);
  const body = page.slice(start, page.indexOf("\n  }\n", start));
  assert.match(body, /api\.fetchFreeFiles\(item\.boothId\)/);
  assert.doesNotMatch(body, /downloadBoothFile|openBoothLink|addFreeProduct/);
  const api = readFileSync(new URL("../src/api.ts", import.meta.url), "utf8");
  assert.match(api, /call<FreeFilesReport>\("fetch_free_files", \{ boothId \}\)/);
  const preview = readFileSync(new URL("../src/preview/api.ts", import.meta.url), "utf8");
  assert.match(preview, /case "fetch_free_files"/);
});
