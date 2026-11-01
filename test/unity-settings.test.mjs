import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("preview fake Unity detection selects, rejects unknown paths and clears", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { dispatch } = await server.ssrLoadModule("/src/preview/api.ts");
    const initial = await dispatch("unity_editors", {});
    assert.equal(initial.selectedPath, "");
    assert.equal(initial.selectedMissing, false);
    assert.ok(initial.editors.length >= 2);
    assert.deepEqual(initial.editors.map((editor) => editor.version), ["6000.0.33f1", "2022.3.22f1", "2022.3.6f1"]);
    for (const editor of initial.editors) assert.match(editor.path, /Unity\.exe$/);

    const target = initial.editors[1].path;
    const selected = await dispatch("set_unity_editor", { path: target });
    assert.equal(selected.selectedPath, target);
    assert.equal((await dispatch("unity_editors", {})).selectedPath, target);

    await assert.rejects(dispatch("set_unity_editor", { path: "C:\\Tools\\Other.exe" }), /Unity 本体/);
    assert.equal((await dispatch("unity_editors", {})).selectedPath, target);

    const cleared = await dispatch("set_unity_editor", { path: "" });
    assert.equal(cleared.selectedPath, "");
  } finally {
    await server.close();
  }
});
