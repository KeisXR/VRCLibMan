import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

test("long lists draw the first group at once and add the rest after each paint", async () => {
  const { window } = parseHTML('<!doctype html><html><body><div id="root"></div></body></html>');
  for (const name of ["window", "document", "HTMLElement", "Event"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  // No requestAnimationFrame here: afterPaint falls back to a short timer.
  window.setTimeout = globalThis.setTimeout.bind(globalThis);
  window.clearTimeout = globalThis.clearTimeout.bind(globalThis);
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { act, createElement } = await import("react");
    const { createRoot } = await import("react-dom/client");
    const list = await server.ssrLoadModule("/src/progressiveList.ts");
    const { LIST_STEP, chunked, chunksFor, useProgressiveCount } = list;

    assert.deepEqual(chunked([1, 2, 3, 4, 5], 2), [[1, 2], [3, 4], [5]]);
    assert.deepEqual(chunked([], 2), []);
    assert.equal(chunksFor(0), 0);
    assert.equal(chunksFor(1), 1);
    assert.equal(chunksFor(LIST_STEP), 1);
    assert.equal(chunksFor(LIST_STEP + 1), 2);

    const seen = {};
    function Probe({ listKey, total }) {
      const { count, reveal } = useProgressiveCount(listKey, total);
      seen.count = count;
      seen.reveal = reveal;
      return null;
    }
    root = createRoot(document.getElementById("root"));
    const render = (listKey, total) => act(async () => { root.render(createElement(Probe, { listKey, total })); });
    const settle = (ms = 40) => act(async () => { await new Promise((resolve) => setTimeout(resolve, ms)); });

    const total = LIST_STEP * 3 + 5;
    await render("asset:all", total);
    assert.equal(seen.count, LIST_STEP, "the first group only");
    for (let i = 0; i < 10 && seen.count < total; i++) await settle();
    assert.equal(seen.count, total, "the list ends up complete");

    // The same list read again (a refresh) keeps what is drawn, so the scroll position stays.
    await render("asset:all", total + 1);
    assert.ok(seen.count >= total);

    // Another genre or filter starts over, and a short list is drawn whole.
    await render("asset:outfit", total);
    assert.equal(seen.count, LIST_STEP);
    await render("asset:hair", 7);
    assert.equal(seen.count, 7);

    // An item opened from elsewhere or reached with the keys is drawn at once.
    await render("world:all", total);
    assert.equal(seen.count, LIST_STEP);
    await act(async () => { seen.reveal(LIST_STEP * 2 + 3); });
    assert.ok(seen.count >= LIST_STEP * 2 + 4);
    await act(async () => { seen.reveal(1); });
    assert.ok(seen.count >= LIST_STEP * 2 + 4, "revealing an item already drawn changes nothing");
  } finally {
    if (root) await (await import("react")).act(async () => root.unmount());
    await server.close();
  }
});

test("the genre list draws its cards in memoized groups and reveals focused or selected items", () => {
  const source = readFileSync("src/components/GenreBrowser.tsx", "utf8");
  assert.match(source, /const CardGroup = memo\(/);
  assert.match(source, /useProgressiveCount\(/);
  assert.match(source, /groups\.slice\(0, chunksFor\(drawnCount\)\)/);
  // A product opened from another screen and a keyboard move both reach items not drawn yet.
  assert.equal(source.match(/reveal\(index\)/g)?.length, 2);
});
