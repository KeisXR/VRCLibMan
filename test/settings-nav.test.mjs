import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("active settings section follows the scroll position", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const { activeSettingsSection, SETTINGS_NAV_ITEMS } = await server.ssrLoadModule("/src/settingsNav.ts");
    const tops = [{ id: "a", top: -300 }, { id: "b", top: 20 }, { id: "c", top: 400 }];
    assert.equal(activeSettingsSection(tops, 60, false), "b");
    assert.equal(activeSettingsSection(tops, 10, false), "a");
    assert.equal(activeSettingsSection([{ id: "a", top: 100 }, { id: "b", top: 500 }], 60, false), "a", "before the first heading reaches the line, the first section is current");
    assert.equal(activeSettingsSection(tops, 60, true), "c", "at the end, the last section is current");
    assert.equal(activeSettingsSection([], 60, false), null);
    const ids = SETTINGS_NAV_ITEMS.map((item) => item.id);
    for (const id of ["settings-unity", "settings-asset-updates", "settings-knowledge", "settings-debug"]) assert.ok(ids.includes(id), id);
    assert.equal(new Set(ids).size, ids.length);
  } finally {
    await server.close();
  }
});

test("narrow settings nav stays as a sticky one-line bar with an accent underline", () => {
  const css = readFileSync("src/styles.css", "utf8");
  const narrow = css.slice(css.indexOf("@container (max-width: 800px)"));
  const block = narrow.slice(0, narrow.indexOf("\n}\n"));
  assert.doesNotMatch(css, /\.settings-nav\s*\{[^}]*position:\s*static/, "the nav never loses sticky");
  assert.match(css, /\.settings-nav\s*\{[^}]*position:\s*sticky;\s*top:\s*0/);
  assert.match(block, /\.settings-nav\s*\{[^}]*flex-direction:\s*row;[^}]*flex-wrap:\s*nowrap;[^}]*overflow-x:\s*auto;[^}]*background:\s*var\(--bg\)/);
  assert.match(block, /\.settings-nav button\[aria-current\]\s*\{\s*box-shadow:\s*inset 0 -2px 0 var\(--line-accent\)/);
  assert.match(block, /scroll-margin-top:\s*calc\(var\(--settings-nav-h/);
  assert.match(css, /\.settings-nav button\[aria-current\]::before\s*\{[^}]*background:\s*var\(--line-accent\)/, "wide list marks the current section with the accent bar");
  const page = readFileSync("src/pages/SettingsPage.tsx", "utf8");
  assert.match(page, /<SettingsNav \/>/);
  assert.match(page, /id="settings-knowledge"/);
  assert.match(page, /id="settings-debug"/);
  const nav = readFileSync("src/components/SettingsNav.tsx", "utf8");
  assert.match(nav, /prefers-reduced-motion: reduce/);
  assert.match(nav, /aria-current=\{active === id \? "location" : undefined\}/);
});
