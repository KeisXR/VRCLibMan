import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

// The floating right panel of a narrow window must stay whole and on screen, because a docked
// browser page (a native view drawn above the HTML) is placed at the same rectangle.
test("floating panel placement stays on screen and resizes from the dragged edge", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const float = await server.ssrLoadModule("/src/floatingPanel.ts");
    const { FLOAT_PANEL_MARGIN: margin, FLOAT_PANEL_SIZE: size } = float;
    const inside = (r, w, h) =>
      r.left >= margin && r.top >= margin && r.left + r.width <= w - margin && r.top + r.height <= h - margin;

    // First placement: the top-right corner, as the old overlay.
    const first = float.defaultFloatRect(900, 820);
    assert.deepEqual(first, { left: 900 - 16 - 420, top: 16, width: 420, height: 720 });
    assert.deepEqual(float.placeFloatRect(null, 900, 820), first);

    // Dragging far past any edge stops at the margin; the size does not change.
    const moved = float.moveFloatRect(first, -2000, 5000, 900, 820);
    assert.equal(moved.left, margin);
    assert.equal(moved.top, 820 - margin - first.height);
    assert.equal(moved.width, first.width);
    assert.ok(inside(moved, 900, 820));

    // Dragging the left edge keeps the right edge still, also when the minimum width stops it.
    const right = first.left + first.width;
    const wider = float.resizeFloatRect(first, "w", -100, 0, 900, 820);
    assert.equal(wider.left + wider.width, right);
    assert.equal(wider.width, first.width + 100);
    const narrowest = float.resizeFloatRect(first, "sw", 1000, -1000, 900, 820);
    assert.equal(narrowest.width, size.minWidth);
    assert.equal(narrowest.left + narrowest.width, right);
    assert.equal(narrowest.height, size.minHeight);
    // The right and lower edges stop at the window.
    const tallest = float.resizeFloatRect(first, "se", 1000, 1000, 900, 820);
    assert.ok(inside(tallest, 900, 820));
    assert.equal(tallest.left, first.left, "a right edge drag never moves the left edge");

    // A placement from a larger window comes back on screen; windows below the minimum shrink it.
    const shrunk = float.placeFloatRect({ left: 700, top: 500, width: 600, height: 700 }, 640, 480);
    assert.ok(inside(shrunk, 640, 480), JSON.stringify(shrunk));
    const tiny = float.clampFloatRect({ left: 0, top: 0, width: 420, height: 720 }, 200, 150);
    assert.deepEqual(tiny, { left: margin, top: margin, width: 200 - 2 * margin, height: 150 - 2 * margin });

    // Rectangles are whole CSS pixels, and broken input falls back to a usable placement.
    const fractional = float.clampFloatRect({ left: 100.4, top: 50.6, width: 300.5, height: 400.2 }, 900, 820);
    assert.ok(Object.values(fractional).every(Number.isInteger), JSON.stringify(fractional));
    const broken = float.clampFloatRect({ left: Number.NaN, top: Infinity, width: Number.NaN, height: -5 }, 900, 820);
    assert.ok(Object.values(broken).every(Number.isFinite) && inside(broken, 900, 820), JSON.stringify(broken));
  } finally {
    await server.close();
  }
});
