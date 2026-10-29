import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("preview selection moves inside the list and waits to load a large image", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const pane = await server.ssrLoadModule("/src/previewPane.ts");
    const ids = ["8800101", "8800102", "8800103"];
    assert.equal(pane.nextPreviewId(ids, null, "ArrowDown"), "8800101");
    assert.equal(pane.nextPreviewId(ids, null, "ArrowRight"), "8800101");
    assert.equal(pane.nextPreviewId(ids, null, "ArrowUp"), "8800103");
    assert.equal(pane.nextPreviewId(ids, null, "ArrowLeft"), "8800103");
    assert.equal(pane.nextPreviewId(ids, "8800101", "ArrowDown"), "8800102");
    assert.equal(pane.nextPreviewId(ids, "8800101", "ArrowRight"), "8800102");
    assert.equal(pane.nextPreviewId(ids, "8800102", "ArrowUp"), "8800101");
    assert.equal(pane.nextPreviewId(ids, "8800102", "ArrowLeft"), "8800101");
    assert.equal(pane.nextPreviewId(ids, "8800103", "ArrowDown"), "8800103");
    assert.equal(pane.nextPreviewId(ids, "8800101", "ArrowUp"), "8800101");
    assert.equal(pane.nextPreviewId(ids, "8800102", "Enter"), "8800102");
    assert.equal(pane.nextPreviewId([], null, "ArrowDown"), null);
    assert.equal(pane.isPreviewMoveKey("ArrowLeft"), true);
    assert.equal(pane.isPreviewMoveKey(" "), false);
    assert.equal(pane.shouldRequestLargePreview(true, false), false);
    assert.equal(pane.shouldRequestLargePreview(true, true), true);
    assert.equal(pane.shouldRequestLargePreview(false, false), true);
    assert.equal(pane.previewKeysBlocked("INPUT", false, false), true);
    assert.equal(pane.previewKeysBlocked("TEXTAREA", false, false), true);
    assert.equal(pane.previewKeysBlocked("SELECT", false, false), true);
    assert.equal(pane.previewKeysBlocked("ARTICLE", true, false), true);
    assert.equal(pane.previewKeysBlocked("BUTTON", false, true), true);
    assert.equal(pane.previewKeysBlocked("ARTICLE", false, false), false);
    assert.equal(pane.previewKeysBlocked("BUTTON", false, false), false);
    assert.ok(pane.PREVIEW_SETTLE_MS >= 400);
    pane.rememberPreviewOpen(true);
    assert.equal(pane.rememberedPreviewOpen(), true);
    pane.rememberPreviewOpen(false);
    assert.equal(pane.rememberedPreviewOpen(), false);
  } finally {
    await server.close();
  }
});
