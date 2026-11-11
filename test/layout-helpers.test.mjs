import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";

async function withModules(paths, run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(...(await Promise.all(paths.map((path) => server.ssrLoadModule(path)))));
  } finally {
    await server.close();
  }
}

test("an empty preview floats as a small note in the top-right corner", async () => {
  await withModules(["/src/floatingPanel.ts"], ({ compactFloatRect, FLOAT_PANEL_MARGIN: margin }) => {
    // The app's smallest window: the note sits where the panel opens and ends above the first product card (268px).
    const small = compactFloatRect(920, 640);
    assert.deepEqual(small, { left: 920 - 16 - 320, top: 16, width: 320, height: 136 });
    // A window narrower than the note keeps it whole and inside the margin.
    const tiny = compactFloatRect(300, 120);
    assert.ok(tiny.left >= margin && tiny.top >= margin);
    assert.ok(tiny.left + tiny.width <= 300 - margin && tiny.top + tiny.height <= 120 - margin);
  });
});

test("a tab strip brings the selected tab into view and reports hidden sides", async () => {
  await withModules(["/src/scrollEdges.ts"], ({ overflowEdges, revealScrollLeft }) => {
    const strip = { clientWidth: 160, scrollWidth: 300, scrollLeft: 0 };
    assert.deepEqual(overflowEdges(true, strip), { start: false, end: true });
    // A tab past the right edge scrolls in, leaving room for the fade.
    assert.equal(revealScrollLeft({ left: 200, width: 84 }, strip), 200 + 84 + 16 - 160);
    // A tab before the left edge scrolls back.
    assert.equal(revealScrollLeft({ left: 20, width: 84 }, { clientWidth: 160, scrollLeft: 120 }), 4);
    // A visible tab does not move the strip.
    assert.equal(revealScrollLeft({ left: 40, width: 84 }, strip), null);
  });
});

test("the address field shows the host first and the label picker opens where there is room", async () => {
  await withModules(["/src/browserAddress.ts", "/src/pages/PurchasesPage.tsx"], ({ readableAddress }, { labelPanelPlacement }) => {
    assert.equal(readableAddress("https://accounts.booth.pm/library"), "accounts.booth.pm/library");
    assert.equal(readableAddress("https://booth.pm/"), "booth.pm");
    assert.equal(readableAddress("http://example.test/"), "http://example.test/", "an insecure page keeps its scheme");
    assert.equal(readableAddress(""), "");

    // Plenty of room below: open down with the full list height.
    assert.deepEqual(labelPanelPlacement({ top: 100, bottom: 120 }, 800), { up: false, listMax: 280 });
    // Near the bottom of an 800px window: open up, and the list fits the space above.
    const low = labelPanelPlacement({ top: 700, bottom: 720 }, 800);
    assert.equal(low.up, true);
    assert.ok(low.listMax <= 280 && low.listMax >= 120);
    // Cramped on both sides: never below a usable list height.
    assert.equal(labelPanelPlacement({ top: 60, bottom: 80 }, 200).listMax, 120);
  });
});

test("the edit dialog knows when closing would lose changes", async () => {
  await withModules(["/src/components/ProductEditDialog.tsx"], ({ editIsDirty }) => {
    const product = { category: "gimmick", genre: "gimmick", genreLocked: false, isGeneric: false, linkedAvatarIds: ["lime"] };
    const form = (extra) => ({ category: "gimmick", genre: "", generic: false, selected: new Set(["lime"]), ...extra });
    assert.equal(editIsDirty(product, form({})), false, "an untouched form closes without asking");
    assert.equal(editIsDirty(product, form({ category: "outfit" })), true);
    assert.equal(editIsDirty(product, form({ generic: true })), true);
    assert.equal(editIsDirty(product, form({ selected: new Set(["lime", "plum"]) })), true);
    assert.equal(editIsDirty(product, form({ genre: "pose" })), true);
  });
});

