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

test("app layout: icon rail at 860px or less, genre column only while the list keeps 480px", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { computeAppLayout, UI_LIST_MIN_WIDTH, UI_RAIL_AUTO_COLLAPSE_PX, UI_PANEL_OVERLAY_PX, UI_RAIL_COLLAPSED_WIDTH } =
      await server.ssrLoadModule("/src/uiLayout.ts");
    assert.equal(UI_LIST_MIN_WIDTH, 480);
    assert.equal(UI_RAIL_AUTO_COLLAPSE_PX, 860);
    assert.equal(UI_PANEL_OVERLAY_PX, 1000);
    const base = { railWidth: 232, railCollapsed: false, genreWidth: 200, browseGenre: true, panelOpen: false, panelMaximized: false, panelStoredWidth: 420 };
    const at = (windowWidth, extra = {}) => computeAppLayout({ ...base, windowWidth, ...extra });
    const pick = ({ railAuto, railCollapsed, rail, genre, genreVisible, genreBar, listWidth }) => ({ railAuto, railCollapsed, rail, genre, genreVisible, genreBar, listWidth });

    assert.deepEqual(pick(at(1100)), { railAuto: false, railCollapsed: false, rail: 232, genre: 200, genreVisible: true, genreBar: false, listWidth: 656 });
    assert.deepEqual(pick(at(1000)), { railAuto: false, railCollapsed: false, rail: 232, genre: 200, genreVisible: true, genreBar: false, listWidth: 556 });
    // The open rail and the genre column shrink together to keep the list at 480px.
    assert.deepEqual(pick(at(900)), { railAuto: false, railCollapsed: false, rail: 219, genre: 189, genreVisible: true, genreBar: false, listWidth: 480 });
    assert.deepEqual(pick(at(880)), { railAuto: false, railCollapsed: false, rail: 208, genre: 180, genreVisible: true, genreBar: false, listWidth: 480 });
    // 860px or less: icons only, without touching the saved state.
    assert.deepEqual(pick(at(860)), { railAuto: true, railCollapsed: true, rail: UI_RAIL_COLLAPSED_WIDTH, genre: 200, genreVisible: true, genreBar: false, listWidth: 588 });
    assert.deepEqual(pick(at(700)), { railAuto: true, railCollapsed: true, rail: UI_RAIL_COLLAPSED_WIDTH, genre: 0, genreVisible: false, genreBar: true, listWidth: 634 });
    // A saved closed rail stays closed at any width.
    assert.equal(at(1100, { railCollapsed: true }).railCollapsed, true);
    assert.equal(at(1100, { railCollapsed: true }).railAuto, false);
    // The genre column shrinks to its 160px minimum before giving way: 60 + 6 + 160 + 6 + 480 = 712.
    assert.deepEqual(pick(at(712, { genreWidth: 320 })), { railAuto: true, railCollapsed: true, rail: 60, genre: 160, genreVisible: true, genreBar: false, listWidth: 480 });
    assert.equal(at(711).genreBar, true);
    // Pages without genres show neither.
    const plain = at(1100, { browseGenre: false });
    assert.equal(plain.genreVisible || plain.genreBar, false);
    assert.equal(plain.genre, 0);

    // A docked right panel keeps its width; the genre column gives way when the list would drop below 480px.
    const docked = at(1100, { panelOpen: true, panelStoredWidth: 300 });
    assert.equal(docked.panelDocked, true);
    assert.equal(docked.panelWidth, 300);
    assert.equal(docked.genreVisible, false);
    assert.equal(docked.genreBar, true);
    assert.equal(docked.rail + 6 + docked.listWidth + 6 + docked.panelWidth, 1100);
    const wide = at(1600, { panelOpen: true, panelStoredWidth: 300 });
    assert.equal(wide.genreVisible, true);
    assert.ok(wide.listWidth >= UI_LIST_MIN_WIDTH);
    // Under 1000px the panel covers the page, so it does not take a column.
    const overlay = at(900, { panelOpen: true, panelStoredWidth: 300 });
    assert.equal(overlay.narrow, true);
    assert.equal(overlay.panelDocked, false);
    assert.equal(overlay.genreVisible, true);

    const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
    const narrowBlock = css.slice(css.indexOf("@media (max-width: 860px)"));
    const narrowRules = narrowBlock.slice(0, narrowBlock.indexOf("\n}\n") + 3);
    // The rail is no longer stacked above the page and cut at 42vh.
    assert.doesNotMatch(narrowRules, /\.app\b|42vh|\.rail/);
    assert.equal(css.match(/@media \(max-width: 860px\)/g)?.length, 1);
    assert.doesNotMatch(css, /genre-mobile/);
    // The grid width animates only under data-rail-motion="slide" (right panel closed); see rail-motion.test.mjs.
    assert.doesNotMatch(css.replace(/\.app-shell\[data-rail-motion="slide"\] \{[^}]*\}/, ""), /transition:[^;]*grid-template-columns/);
    assert.match(css, /\.chip-bar \{[^}]*position: sticky;[^}]*top: var\(--topbar-height/);
    assert.match(css, /\.chip-bar-track \{[^}]*overflow-x: auto;/);
    // 選択中のチップは「線のアクセント」ブロックの共通の直線の下線（::after）。片側の影は使わない。
    assert.doesNotMatch(css, /\.chip-bar-item\.active \{[^}]*inset 0 -2px 0 var\(--line-accent\)/);
    const accent = css.slice(css.indexOf("==== 線のアクセント（朱）"), css.indexOf("==== 線のアクセントここまで"));
    assert.match(accent, /\.chip-bar-item\.active \{ position: relative; \}/);
    assert.match(accent, /\.chip-bar-item\.active::after,[^{]*\{[^}]*left: var\(--accent-inset\);[^}]*border-radius: var\(--accent-cap\);[^}]*background: var\(--line-accent\)/);
    for (const file of ["../src/components/GenreBrowser.tsx", "../src/components/GenreChipBar.tsx"]) {
      assert.doesNotMatch(readFileSync(new URL(file, import.meta.url), "utf8"), /genre-mobile/);
    }
  } finally {
    await server.close();
  }
});
