import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("remote product probe helpers and preview command", async (t) => {
  const server = await createServer({
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
  });
  try {
    const {
      applyRemoteProbe,
      clearRemoteProbeCache,
      itemNeedsRemoteProbe,
      placeLabelForFile,
      readProbeCache,
      remoteProbeStatusFor,
      writeProbeCache,
      withClientTimeout,
    } = await server.ssrLoadModule("/src/remoteProductProbe.ts");
    const { dispatch } = await server.ssrLoadModule("/src/preview/api.ts");

    const baseItem = {
      boothId: "8800101",
      title: "見本",
      shop: "見本",
      url: "https://example.test/item",
      descriptionSnippet: "",
      thumbnailUrl: null,
      thumbnailPath: null,
      category: "outfit",
      genre: "other",
      isGeneric: false,
      confidence: 0.9,
      viaBaseBody: false,
      localFiles: [],
      localFileCount: 0,
      variations: [],
      downloads: [
        {
          variation: "",
          filename: "pack.zip",
          sizeBytes: 1,
          state: "missing",
          localPath: null,
          remotePath: null,
        },
      ],
      isFree: false,
      isAdult: false,
      unpublished: false,
      remoteConfigured: true,
      remoteChecked: false,
      remoteReachable: false,
    };

    assert.equal(itemNeedsRemoteProbe(baseItem), true);
    assert.equal(placeLabelForFile(baseItem.downloads[0], baseItem, "idle"), "確認中");
    assert.equal(placeLabelForFile(baseItem.downloads[0], baseItem, "probing"), "確認中");
    assert.equal(placeLabelForFile(baseItem.downloads[0], baseItem, "failed"), "未確認");

    clearRemoteProbeCache();
    const probe = {
      boothId: "8800101",
      remoteChecked: true,
      remoteReachable: true,
      downloads: [{ ...baseItem.downloads[0], state: "remote", remotePath: "\\\\nas\\pack.zip" }],
    };
    writeProbeCache([probe]);
    assert.deepEqual(readProbeCache("8800101")?.boothId, "8800101");
    const merged = applyRemoteProbe(baseItem, probe);
    assert.equal(merged.remoteChecked, true);
    assert.equal(merged.downloads[0].state, "remote");
    assert.equal(remoteProbeStatusFor(merged, {}), "ok");

    const ok = await dispatch("probe_remote_products", { ids: ["8800101"] });
    assert.equal(ok.length, 1);
    assert.equal(ok[0].remoteChecked, true);

    await assert.rejects(
      () => withClientTimeout(new Promise((resolve) => setTimeout(() => resolve(1), 80)), 20),
      /タイムアウト/,
    );
  } finally {
    await server.close();
  }
});

test("a fresh local file wins over an older remote probe and can be invalidated", async () => {
  const server = await createServer({
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
  });
  try {
    const {
      applyRemoteProbe,
      clearRemoteProbeCache,
      invalidateRemoteProbe,
      onRemoteProbeInvalidated,
      readProbeCache,
      writeProbeCache,
    } = await server.ssrLoadModule("/src/remoteProductProbe.ts");
    const { finishedPath } = await server.ssrLoadModule("/src/pages/AvatarDetailPage.tsx");
    const { noticeAction } = await server.ssrLoadModule("/src/App.tsx");

    const file = (filename, state, extra = {}) => ({
      variation: "",
      filename,
      sizeBytes: 1,
      state,
      localPath: null,
      remotePath: null,
      ...extra,
    });
    const fresh = {
      boothId: "8800102",
      downloads: [
        file("pack.zip", "local", { localPath: "/lib/8800102/pack.zip" }),
        file("other.zip", "missing"),
        file("renamed.unitypackage", "local", { localPath: "/lib/8800102/renamed.unitypackage" }),
        file("gone.zip", "missing"),
      ],
      remoteConfigured: true,
      remoteChecked: false,
      remoteReachable: false,
    };
    const stale = {
      boothId: "8800102",
      remoteChecked: true,
      remoteReachable: true,
      downloads: [
        file("pack.zip", "missing"),
        file("other.zip", "remote", { remotePath: "/nas/8800102/other.zip" }),
        file("2336028", "missing"),
        file("gone.zip", "local", { localPath: "/lib/8800102/gone.zip" }),
      ],
    };
    const merged = applyRemoteProbe(fresh, stale);
    assert.equal(merged.remoteChecked, true);
    assert.deepEqual(
      merged.downloads.map((row) => [row.filename, row.state]),
      [
        ["pack.zip", "local"],
        ["other.zip", "remote"],
        ["renamed.unitypackage", "local"],
        ["gone.zip", "missing"],
      ],
    );
    assert.equal(merged.downloads[0].localPath, "/lib/8800102/pack.zip");

    clearRemoteProbeCache();
    writeProbeCache([stale]);
    const seen = [];
    const stop = onRemoteProbeInvalidated((id) => seen.push(id));
    invalidateRemoteProbe("8800102");
    stop();
    invalidateRemoteProbe("8800102");
    assert.equal(readProbeCache("8800102"), null);
    assert.deepEqual(seen, ["8800102"]);

    const done = {
      boothId: "8800102",
      variation: "",
      filename: "2336028",
      phase: "done",
      message: "ライブラリに移しました",
      path: "C:\\Assets\\8800102\\renamed.unitypackage",
    };
    assert.equal(finishedPath(done, file("2336028", "missing")), done.path);
    assert.equal(finishedPath(done, file("renamed.unitypackage", "local")), done.path);
    assert.equal(finishedPath(done, file("other.zip", "missing")), null);
    assert.equal(finishedPath({ ...done, variation: "通常" }, file("2336028", "missing")), null);
    assert.equal(finishedPath({ ...done, phase: "receiving" }, file("2336028", "missing")), null);
    assert.equal(finishedPath({ ...done, path: null }, file("2336028", "missing")), null);

    const undo = () => {};
    assert.deepEqual(noticeAction(undo), { label: "元に戻す", run: undo });
    const open = { label: "保存先を開く", run: () => {} };
    assert.equal(noticeAction(open), open);
    assert.equal(noticeAction(undefined), undefined);
  } finally {
    await server.close();
  }
});
