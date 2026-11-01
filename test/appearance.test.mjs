import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("appearance startup, live updates and preview settings", async (t) => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { normalizeAppearance, startAppearance } = await server.ssrLoadModule("/src/appearance.ts");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    await t.test("defaults, normalized choices and unrelated patches", async () => {
      assert.equal((await api.settings()).appearance, "classic");
      assert.equal(normalizeAppearance(" liquid "), "liquid");
      assert.equal(normalizeAppearance(null), "classic");
      const before = await api.settings();
      assert.equal((await api.updateSettings({ appearance: "liquid" })).appearance, "liquid");
      const saved = await api.updateSettings({ confidenceThreshold: 0.75 });
      assert.equal(saved.appearance, "liquid");
      assert.equal(saved.ollamaModel, before.ollamaModel);
      assert.equal((await api.updateSettings({ appearance: "future" })).appearance, "classic");
    });
    await t.test("a live change wins over a slow startup read in either window", async () => {
      const painted = [];
      let resolveRead;
      let changed;
      let stopped = false;
      const ready = startAppearance(
        () => new Promise((resolve) => { resolveRead = resolve; }),
        async (listener) => { changed = listener; return () => { stopped = true; }; },
        (value) => painted.push(value),
      );
      await new Promise((resolve) => setTimeout(resolve, 0));
      changed("liquid");
      resolveRead({ appearance: "classic" });
      const stop = await ready;
      assert.deepEqual(painted, ["liquid"]);
      changed("classic");
      assert.deepEqual(painted, ["liquid", "classic"]);
      stop();
      assert.equal(stopped, true);
    });
    await t.test("startup failure still renders a usable classic window", async () => {
      const painted = [];
      await startAppearance(async () => { throw new Error("offline"); }, async () => { throw new Error("events unavailable"); }, (value) => painted.push(value));
      assert.deepEqual(painted, ["classic"]);
    });
  } finally {
    await server.close();
  }
});
