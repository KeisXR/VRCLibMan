import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");

test("rail motion: width slides only while the right panel is closed", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { railMotionAttrs } = await server.ssrLoadModule("/src/uiLayout.ts");
    assert.deepEqual(railMotionAttrs(false, false), {});
    assert.deepEqual(railMotionAttrs(false, true), {});
    assert.deepEqual(railMotionAttrs(true, false), { "data-rail-motion": "slide" });
    // Panel open: never the slide value, so the grid gets no transition.
    assert.deepEqual(railMotionAttrs(true, true), { "data-rail-motion": "fade" });
  } finally {
    await server.close();
  }
});

test("rail motion: grid transition only under slide, and only on allowed properties", () => {
  const slide = '.app-shell[data-rail-motion="slide"] { transition: grid-template-columns var(--motion-rail) var(--ease-out); }';
  assert.ok(css.includes(slide), "the slide rule animates the grid width");
  // No other rule (including the fade one used while the right panel is open) moves the grid.
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
