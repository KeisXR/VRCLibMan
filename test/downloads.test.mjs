import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("downloads run side by side and keep their own state", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const d = await server.ssrLoadModule("/src/downloads.ts");
    const body = { boothId: "424242", variation: "", filename: "body.unitypackage" };
    const hair = { boothId: "424242", variation: "", filename: "hair.unitypackage" };
    let board = d.startDownload(d.EMPTY_DOWNLOADS, body);
    board = d.startDownload(board, hair);
    assert.equal(d.activeDownloads(board), 2);
    assert.deepEqual(board.entries.map((e) => e.filename), ["hair.unitypackage", "body.unitypackage"]);

    board = d.applyFileStatus(board, { ...body, phase: "receiving", message: "ダウンロード中", received: 5 * 1024 * 1024, total: 20 * 1024 * 1024 });
    board = d.applyFileStatus(board, { ...hair, phase: "queued", message: "順番待ち" });
    const bodyEntry = board.entries.find((e) => e.filename === "body.unitypackage");
    assert.equal(d.downloadPercent(bodyEntry), 25);
    assert.equal(d.downloadDetail(bodyEntry), "5.0 MB / 20.0 MB（25%）");
    assert.equal(d.downloadDetail(board.entries.find((e) => e.filename === "hair.unitypackage")), "順番待ち");
    // Status updates keep the order the files were pressed in.
    assert.deepEqual(board.entries.map((e) => e.filename), ["hair.unitypackage", "body.unitypackage"]);
    assert.deepEqual(d.overallDownload(board), { current: 0, total: 0 });

    board = d.applyFileStatus(board, { ...body, phase: "done", message: "ライブラリに移しました", path: "/lib/424242/body.unitypackage" });
    // A late progress event and the command result do not undo the finished state.
    board = d.applyFileStatus(board, { ...body, phase: "receiving", message: "ダウンロード中", received: 1, total: 2 });
    board = d.finishDownload(board, body, { ok: true, path: null });
    const done = board.entries.find((e) => e.filename === "body.unitypackage");
    assert.equal(done.phase, "done");
    assert.equal(done.path, "/lib/424242/body.unitypackage");
    assert.equal(d.activeDownloads(board), 1);
    assert.equal(d.downloadsLabel(board), "ダウンロード中 1件");

    board = d.finishDownload(board, hair, { ok: false, message: "ダウンロードできませんでした" });
    assert.equal(d.downloadsLabel(board), "ダウンロード 1件失敗");
    // Pressing again restarts the failed file at the top.
    board = d.startDownload(board, hair);
    assert.equal(board.entries[0].phase, "starting");
    board = d.clearFinishedDownloads(board);
    assert.deepEqual(board.entries.map((e) => e.filename), ["hair.unitypackage"]);
  } finally {
    await server.close();
  }
});

test("finished downloads are trimmed and the detail page matches renamed rows", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const d = await server.ssrLoadModule("/src/downloads.ts");
    let board = d.EMPTY_DOWNLOADS;
    for (let i = 0; i < d.KEPT_FINISHED + 5; i += 1) {
      const target = { boothId: "5000", variation: "", filename: `f${i}.zip` };
      board = d.finishDownload(d.startDownload(board, target), target, { ok: true, path: null });
    }
    board = d.startDownload(board, { boothId: "5000", variation: "", filename: "running.zip" });
    assert.equal(board.entries.length, d.KEPT_FINISHED + 1);
    assert.equal(board.entries[0].filename, "running.zip");

    const page = await server.ssrLoadModule("/src/pages/AvatarDetailPage.tsx");
    const placed = { boothId: "4242", variation: "", filename: "2336028" };
    let files = d.startDownload(d.EMPTY_DOWNLOADS, placed);
    files = d.applyFileStatus(files, { ...placed, phase: "done", message: "ライブラリに移しました", path: "/lib/4242/renamed.unitypackage" });
    const row = (filename, state) => ({ variation: "", filename, state });
    assert.equal(page.finishedPathIn(files.entries, "4242", row("renamed.unitypackage", "local")), "/lib/4242/renamed.unitypackage");
    assert.equal(page.finishedPathIn(files.entries, "4242", row("other.zip", "missing")), null);
    assert.equal(page.finishedPathIn(files.entries, "9999", row("2336028", "missing")), null);
  } finally {
    await server.close();
  }
});
