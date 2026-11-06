import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");

test("rail motion: width slides on every toggle, hold only when the panel rectangle changes", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { railMotionAttrs, railMotionHoldsPanel, computeAppLayout } = await server.ssrLoadModule("/src/uiLayout.ts");
    assert.deepEqual(railMotionAttrs("none"), {});
    assert.deepEqual(railMotionAttrs("slide"), { "data-rail-motion": "slide" });
    assert.deepEqual(railMotionAttrs("hold"), { "data-rail-motion": "slide", "data-rail-hold": "" });
    const base = { railWidth: 200, genreWidth: 200, browseGenre: true, panelOpen: true, panelMaximized: false, panelStoredWidth: 360 };
    const toggle = (input) => railMotionHoldsPanel(computeAppLayout({ ...input, railCollapsed: false }), computeAppLayout({ ...input, railCollapsed: true }));
    // Panel closed: never hold.
    assert.equal(toggle({ ...base, windowWidth: 1150, panelOpen: false }), false);
    // Wide window with the panel open: the list absorbs the change, so the panel stays put.
    assert.equal(toggle({ ...base, windowWidth: 1600 }), false);
    assert.equal(toggle({ ...base, windowWidth: 1280, browseGenre: false }), false);
    // Maximized panel follows the rail, so the page is hidden.
    assert.equal(toggle({ ...base, windowWidth: 1600, panelMaximized: true }), true);
    // Around 1020–1140px the genre column flips with the toggle, but the panel keeps its width
    // and left edge (the list absorbs it), so the page keeps following: no hold.
    let flips = 0;
    for (let w = 1000; w <= 1400; w += 2) {
      const input = { ...base, windowWidth: w };
      const a = computeAppLayout({ ...input, railCollapsed: false });
      const b = computeAppLayout({ ...input, railCollapsed: true });
      assert.equal(toggle(input), a.panelWidth !== b.panelWidth, `w=${w}`);
      if (a.genreVisible !== b.genreVisible) flips++;
    }
    assert.ok(flips > 0, "a width where the genre column flips exists");
    // A stored width at the limit gets clamped differently with the rail open or closed: hold.
    let clamped = false;
    for (let w = 1000; w <= 1400 && !clamped; w += 2) clamped = toggle({ ...base, windowWidth: w, browseGenre: false, panelStoredWidth: 5000 });
    assert.ok(clamped, "panel width clamped at its limit changes with the toggle");
  } finally {
    await server.close();
  }
});

test("rail motion: hold hides the docked page like a drag", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  const prev = globalThis.document;
  try {
    const { browserPanelVisible } = await server.ssrLoadModule("/src/browserPanel.ts");
    const area = { left: 900, top: 40, width: 360, height: 600 };
    let hold = false;
    globalThis.document = { body: { classList: { contains: () => false } }, querySelector: (sel) => (sel === "[data-rail-hold]" && hold ? {} : null) };
    assert.equal(browserPanelVisible(area, { overlay: false, dragging: false, covers: [] }), true);
    hold = true;
    assert.equal(browserPanelVisible(area, { overlay: false, dragging: false, covers: [] }), false);
  } finally {
    globalThis.document = prev;
    await server.close();
  }
  const panel = readFileSync(new URL("../src/components/BrowserPanel.tsx", import.meta.url), "utf8");
  assert.match(panel, /attributeFilter: \[[^\]]*"data-rail-hold"/, "the hold attribute re-reports the panel");
});

test("rail motion: grid transition only under slide, and only on allowed properties", () => {
  const slide = '.app-shell[data-rail-motion="slide"] { transition: grid-template-columns var(--motion-rail) var(--ease-out); }';
  assert.ok(css.includes(slide), "the slide rule animates the grid width");
  // No other rule moves the grid.
  assert.doesNotMatch(css.replace(slide, ""), /grid-template-columns[^;{}]*(var\(--motion|ms)/);
  assert.doesNotMatch(css.replace(slide, ""), /transition:[^;]*grid-template-columns/);
  for (const name of ["rail-label-in", "rail-icon-in"]) {
    const body = new RegExp(`@keyframes ${name} \\{([^\\n]*)\\}`).exec(css)?.[1];
    assert.ok(body, name);
    const props = [...body.matchAll(/([a-z-]+):/g)].map((m) => m[1]);
    for (const prop of props) assert.ok(["opacity", "transform"].includes(prop), `${name} animates ${prop}`);
  }
  for (const token of ["--motion: 160ms", "--motion-fast: 120ms", "--motion-rail: 140ms", "--motion-delay: 30ms", "--ease-out: cubic-bezier(0.32, 0.72, 0, 1)"]) {
    assert.ok(css.includes(token), token);
  }
});

test("rail motion: reduced motion stops every animation and transition", () => {
  const block = /@media \(prefers-reduced-motion: reduce\) \{\s*\*, \*::before, \*::after \{([^}]*)\}/.exec(css)?.[1];
  assert.ok(block);
  assert.match(block, /animation: none !important/);
  assert.match(block, /transition: none !important/);
});

test("rail motion: the old fade mode is gone", () => {
  assert.doesNotMatch(css, /data-rail-motion="fade"|rail-label-out|-in-soft|--motion-fade-out/);
});

test("rail motion: slide mode values are unchanged (C2)", () => {
  assert.ok(css.includes(".app-shell[data-rail-motion] .rail:not(.collapsed) :is(.nav-label, .count, .brand-wordmark, .rail-status) {\n  animation: rail-label-in var(--motion-fast) var(--ease-out) var(--motion-delay) both;"));
  assert.ok(css.includes(".app-shell[data-rail-motion] .rail.collapsed :is(.nav-icon, .brand-mark, .rail-toggle) {\n  animation: rail-icon-in var(--motion-fast) var(--ease-out) both;"));
  assert.ok(css.includes("@keyframes rail-label-in { from { opacity: 0; transform: translateX(-4px); } to { opacity: 1; transform: none; } }"));
  assert.ok(css.includes("@keyframes rail-icon-in { from { opacity: 0; } to { opacity: 1; } }"));
  assert.doesNotMatch(css, /\[data-rail-motion="slide"\][^{]*\.rail/);
});
