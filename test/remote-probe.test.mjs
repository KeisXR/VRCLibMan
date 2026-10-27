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
