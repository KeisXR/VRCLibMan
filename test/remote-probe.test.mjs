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

test("saving library settings invalidates cached, mounted and in-flight remote probes", async (t) => {
  const { parseHTML } = await import("linkedom");
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Element", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement, useEffect } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { clearRemoteProbeCache, readProbeCache, useRemoteProductProbe } = await server.ssrLoadModule("/src/remoteProductProbe.ts");
    const { useSettingsAutosave } = await server.ssrLoadModule("/src/useSettingsAutosave.ts");
    const initial = { ...(await api.settings()), remoteLibraryFolder: "/remote-A" };
    const base = {
      boothId: "8800103", remoteConfigured: true, remoteChecked: false, remoteReachable: false,
      downloads: [{ variation: "", filename: "sample.zip", sizeBytes: 5, state: "missing", localPath: null, remotePath: null }],
    };
    const items = [base];
    const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
    const wait = async (check) => {
      for (let n = 0; n < 100; n++) { if (check()) return; await act(tick); }
      assert.fail(`Remote probe did not settle: ${document.body.textContent}`);
    };

    async function fixture(run) {
      clearRemoteProbeCache();
      let stored = { ...initial };
      let controller;
      let refuseSave = false;
      const pending = [];
      api.updateSettings = async (patch) => {
        if (refuseSave) throw new Error("保存に失敗しました");
        stored = { ...stored, ...patch };
        return { ...stored };
      };
      api.probeRemoteProducts = () => new Promise((resolve) => pending.push({ root: stored.remoteLibraryFolder, resolve }));
      function Saver() {
        controller = useSettingsAutosave(() => {});
        useEffect(() => controller.hydrate(initial, controller.ticket()), []);
        return null;
      }
      function Probe() {
        const result = useRemoteProductProbe(items);
        return createElement("output", null, JSON.stringify(result.items[0]));
      }
      const root = createRoot(document.getElementById("root"));
      const render = (showProbe) => act(async () => {
        root.render(createElement("div", null, createElement(Saver, { key: "settings" }), showProbe ? createElement(Probe, { key: "probe" }) : null));
        await tick();
      });
      const finish = (request) => act(async () => {
        const remote = request.root === "/remote-A";
        request.resolve([{
          boothId: base.boothId, remoteChecked: true, remoteReachable: true,
          downloads: [{ ...base.downloads[0], state: remote ? "remote" : "missing", remotePath: remote ? "/remote-A/8800103/sample.zip" : null }],
        }]);
        await tick();
      });
      const change = (patch) => act(async () => { controller.change(patch); await tick(); });
      const displayed = () => JSON.parse(document.querySelector("output").textContent);
      try {
        await render(true);
        await wait(() => pending.length === 1);
        await run({ pending, render, finish, change, displayed, failSaves: () => { refuseSave = true; }, controller: () => controller });
      } finally {
        await act(async () => root.unmount());
      }
    }

    await t.test("a remounted page probes the new remote, and failed saves keep valid cache", () => fixture(async (f) => {
      await f.finish(f.pending[0]);
      assert.equal(f.displayed().downloads[0].state, "remote");
      await f.render(false);
      await f.change({ remoteLibraryFolder: "/remote-B" });
      assert.equal(readProbeCache(base.boothId), null);
      await f.render(true);
      await wait(() => f.pending.length === 2);
      assert.equal(f.pending[1].root, "/remote-B");
      await f.finish(f.pending[1]);
      assert.equal(f.displayed().downloads[0].state, "missing");
      await f.render(false);
      await f.render(true);
      assert.equal(f.pending.length, 2, "the new remote's cache can still be reused");
      f.failSaves();
      await f.change({ remoteLibraryFolder: "/remote-C" });
      await wait(() => f.controller().phase === "error");
      assert.equal(readProbeCache(base.boothId).downloads[0].state, "missing");
      assert.equal(f.pending.length, 2);
    }));

    await t.test("saving a library folder clears results in a page that stays mounted", () => fixture(async (f) => {
      await f.finish(f.pending[0]);
      assert.equal(f.displayed().downloads[0].state, "remote");
      await f.change({ libraryFolder: "/new-library", remoteLibraryFolder: "/remote-B" });
      await wait(() => f.pending.length === 2);
      assert.equal(f.displayed().downloads[0].remotePath, null);
      await f.finish(f.pending[1]);
      assert.equal(f.displayed().downloads[0].state, "missing");
    }));

    await t.test("an old request finishing after unmount cannot restore the previous remote", () => fixture(async (f) => {
      const oldRequest = f.pending[0];
      await f.render(false);
      await f.change({ remoteLibraryFolder: "/remote-B" });
      await f.render(true);
      await wait(() => f.pending.length === 2);
      await f.finish(f.pending[1]);
      await f.finish(oldRequest);
      assert.equal(f.displayed().downloads[0].state, "missing");
      assert.equal(readProbeCache(base.boothId).downloads[0].remotePath, null);
    }));
  } finally {
    await server.close();
  }
});
