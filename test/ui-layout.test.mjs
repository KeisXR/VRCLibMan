import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("ui layout width helpers clamp and fit list minimum", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { clampRailWidth, clampGenreWidth, fitBrowseWidths, UI_LIST_MIN_WIDTH, UI_RAIL_COLLAPSED_WIDTH } = await server.ssrLoadModule(
      "/src/uiLayout.ts",
    );
    assert.equal(clampRailWidth(100), 180);
    assert.equal(clampRailWidth(999), 360);
    assert.equal(clampGenreWidth(120), 160);
    const fitted = fitBrowseWidths(900, 232, 200, true);
    assert.ok(fitted.rail + fitted.genre + UI_LIST_MIN_WIDTH + 12 <= 900);
    // A collapsed rail keeps its icon width; only the genre column gives way.
    assert.deepEqual(fitBrowseWidths(1280, 300, 200, false, true), { rail: UI_RAIL_COLLAPSED_WIDTH, genre: 0 });
    assert.deepEqual(fitBrowseWidths(1280, 300, 240, true, true), { rail: UI_RAIL_COLLAPSED_WIDTH, genre: 240 });
    const tight = fitBrowseWidths(800, 300, 320, true, true);
    assert.equal(tight.rail, UI_RAIL_COLLAPSED_WIDTH);
    assert.ok(tight.rail + tight.genre + UI_LIST_MIN_WIDTH + 12 <= 800);
    assert.ok(UI_RAIL_COLLAPSED_WIDTH >= 56 && UI_RAIL_COLLAPSED_WIDTH <= 64);
    const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
    const tokens = readFileSync(new URL("../docs/design/tokens.css", import.meta.url), "utf8");
    // The wordmark is outlined glyphs, not text that needs a bundled font.
    const wordmark = readFileSync(new URL("../src/assets/brand/vrclibman-wordmark.svg", import.meta.url), "utf8");
    assert.match(wordmark, /<path d="M/);
    assert.match(wordmark, /fill="currentColor"/);
    assert.doesNotMatch(wordmark, /<text|@font-face|font-family|\.woff/);
    assert.doesNotMatch(css, /@font-face|font-family:[^;]*Urbanist/);
    for (const source of [css, tokens]) {
      assert.match(source, new RegExp(`--sidebar-collapsed-width: ${UI_RAIL_COLLAPSED_WIDTH}px;`));
    }
  } finally {
    await server.close();
  }
});
