import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("settings autosave handles rapid edits, stale reads, and failed saves", async (t) => {
  const server = await createServer({
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
  });
  try {
    const { createSettingsAutosave } = await server.ssrLoadModule(
      "/src/settingsAutosave.ts",
    );
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const initial = { ...(await api.settings()) };
    const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

    await t.test(
      "older responses cannot replace a newer choice and concurrent changes are serialized",
      async () => {
        let stored = { ...initial };
        const requests = [];
        const pending = [];
        const controller = createSettingsAutosave((patch) => {
          requests.push(patch);
          return new Promise((resolve) =>
            pending.push(() => {
              stored = { ...stored, ...patch };
              resolve(stored);
            }),
          );
        });
        controller.hydrate(stored, controller.ticket());
        const staleTicket = controller.ticket();
        controller.change({ ollamaModel: "qwen3.5:4b" });
        controller.change({
          ollamaModel: "qwen3.5:9b",
          confidenceThreshold: 0.8,
        });
        controller.hydrate(initial, staleTicket);
        assert.equal(requests.length, 1);
        assert.equal(controller.snapshot().settings.ollamaModel, "qwen3.5:9b");
        pending.shift()();
        await tick();
        assert.equal(requests.length, 2);
        assert.equal(controller.snapshot().settings.ollamaModel, "qwen3.5:9b");
        assert.equal(controller.snapshot().phase, "saving");
        pending.shift()();
        await controller.flush();
        assert.equal(stored.ollamaModel, "qwen3.5:9b");
        assert.equal(stored.confidenceThreshold, 0.8);
        assert.equal(controller.snapshot().phase, "saved");
        controller.hydrate(initial, staleTicket);
        assert.equal(controller.snapshot().settings.confidenceThreshold, 0.8);
      },
    );

    await t.test(
      "leaving an input or page flushes the last text value without a save button",
      async () => {
        const patches = [];
        const controller = createSettingsAutosave(async (patch) => {
          patches.push(patch);
          return { ...initial, ...patch };
        });
        controller.hydrate(initial, controller.ticket());
        controller.change({ remoteLibraryFolder: "/first" }, 300);
        controller.change({ remoteLibraryFolder: "/complete/path" }, 300);
        assert.deepEqual(patches, []);
        await controller.flush();
        assert.deepEqual(patches, [{ remoteLibraryFolder: "/complete/path" }]);
        assert.equal(controller.snapshot().phase, "saved");
      },
    );

    await t.test(
      "validation errors retain the draft and a corrected edit retries all unsaved changes",
      async () => {
        let stored = { ...initial };
        const controller = createSettingsAutosave(async (patch) => {
          if (patch.ollamaModel === "") throw new Error("モデル名が空です");
          stored = { ...stored, ...patch };
          return stored;
        });
        controller.hydrate(initial, controller.ticket());
        controller.change({ ollamaModel: "", confidenceThreshold: 0.9 });
        await assert.rejects(controller.flush(), /モデル名が空/);
        assert.equal(controller.snapshot().phase, "error");
        assert.equal(controller.snapshot().settings.ollamaModel, "");
        assert.equal(stored.confidenceThreshold, initial.confidenceThreshold);
        controller.hydrate(initial, controller.ticket());
        assert.equal(controller.snapshot().settings.confidenceThreshold, 0.9);
        controller.change({ ollamaModel: "qwen3.5:4b" });
        await controller.flush();
        assert.equal(stored.ollamaModel, "qwen3.5:4b");
        assert.equal(stored.confidenceThreshold, 0.9);
        assert.equal(controller.snapshot().error, null);
      },
    );
  } finally {
    await server.close();
  }
});
