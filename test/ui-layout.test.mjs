import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

test("ui layout width helpers clamp and fit list minimum", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { clampRailWidth, clampGenreWidth, fitBrowseWidths, UI_LIST_MIN_WIDTH } = await server.ssrLoadModule(
      "/src/uiLayout.ts",
    );
    assert.equal(clampRailWidth(100), 180);
    assert.equal(clampRailWidth(999), 360);
    assert.equal(clampGenreWidth(120), 160);
    const fitted = fitBrowseWidths(900, 232, 200, true);
    assert.ok(fitted.rail + fitted.genre + UI_LIST_MIN_WIDTH + 12 <= 900);
  } finally {
    await server.close();
  }
});
