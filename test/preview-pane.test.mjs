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
    assert.equal(pane.PREVIEW_PANE_WIDTH.default, 300);
    assert.equal(pane.PREVIEW_PANE_WIDTH.min, 240);
    assert.equal(pane.previewPaneMaxWidth(1440, null), 720);
    assert.equal(pane.previewPaneMaxWidth(1440, 900), 636);
    assert.equal(pane.previewPaneMaxWidth(1000, 500), 240);
    assert.equal(pane.clampPreviewPaneWidth(100, 1440, 900), 240);
    assert.equal(pane.clampPreviewPaneWidth(500, 1440, 900), 500);
    assert.equal(pane.clampPreviewPaneWidth(900, 1440, 900), 636);
    assert.equal(pane.clampPreviewPaneWidth(Number.NaN, 1440, null), 300);
    assert.equal(pane.clampStoredPreviewPaneWidth(10), 240);
    assert.equal(pane.clampStoredPreviewPaneWidth(9000), 1600);
    assert.equal(pane.clampStoredPreviewPaneWidth(420), 420);
    pane.rememberPreviewWidth(480);
    assert.equal(pane.rememberedPreviewWidth(), 480);
    pane.rememberPreviewWidth(10);
    assert.equal(pane.rememberedPreviewWidth(), 240);
    pane.rememberPreviewOpen(true);
    assert.equal(pane.rememberedPreviewOpen(), true);
    pane.rememberPreviewOpen(false);
    assert.equal(pane.rememberedPreviewOpen(), false);
  } finally {
    await server.close();
  }
});
