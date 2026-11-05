import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("avatar hero helpers normalize size, height, and contrast", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const hero = await server.ssrLoadModule("/src/avatarHero.ts");
    assert.equal(hero.normalizeAvatarHero("large"), "large");
    assert.equal(hero.normalizeAvatarHero("SMALL"), "small");
    assert.equal(hero.normalizeAvatarHero("off"), "off");
    assert.equal(hero.normalizeAvatarHero("classic"), "small");
    assert.equal(hero.normalizeAvatarHero(null), "small");
    assert.equal(hero.avatarHeroHeightPx("large", 1200), 400);
    assert.equal(hero.avatarHeroHeightPx("small", 1200), 320);
    assert.equal(hero.avatarHeroHeightPx("large", 800), 220);
    assert.equal(hero.avatarHeroHeightPx("small", 800), 220);
    assert.equal(hero.avatarHeroHeightPx("off", 1200), 0);
    assert.deepEqual(hero.AVATAR_HERO_BASE, { width: 2400, height: 1200 });
    assert.equal(hero.AVATAR_HERO_CHOICES.length, 3);

    const light = hero.heroTextContrast([0x20, 0x22, 0x2b], [0xf5, 0xf6, 0xf8]);
    const dark = hero.heroTextContrast([0xf3, 0xf3, 0xf5], [0x12, 0x12, 0x14]);
    assert.ok(light >= 4.5, `light contrast ${light}`);
    assert.ok(dark >= 4.5, `dark contrast ${dark}`);
    const bare = hero.contrastRatio(
      hero.relativeLuminance(0x20, 0x22, 0x2b),
      hero.relativeLuminance(128, 128, 128),
    );
    assert.ok(bare < 4.5, `bare mid-gray should fail without fade: ${bare}`);
  } finally {
    await server.close();
  }
});

test("avatar hero CSS uses early fade, mid blur, and no curved accent on the title", () => {
  const css = readFileSync("src/styles.css", "utf8");
  assert.match(css, /\.avatar-hero\[data-size="large"\]\s*\{\s*--avatar-hero-h:\s*400px/);
  assert.match(css, /\.avatar-hero\[data-size="small"\]\s*\{\s*--avatar-hero-h:\s*320px/);
  assert.match(css, /@media \(max-width: 860px\)[\s\S]*--avatar-hero-h:\s*220px/);
  assert.match(css, /backdrop-filter:\s*blur\(2px\)/);
  assert.match(css, /backdrop-filter:\s*blur\(8px\)/);
  assert.match(css, /backdrop-filter:\s*blur\(24px\)/);
  assert.match(css, /transparent 25%/);
  assert.match(css, /\.avatar-hero-copy/);
  assert.match(css, /\.avatar-hero-nav/);
  assert.doesNotMatch(css, /\.avatar-hero-scrim\s*\{/);
  assert.match(css, /avatar-hero[\s\S]*border-radius:\s*0/);
  assert.match(css, /\.avatar-hero-title::before\s*\{\s*content:\s*none/);
  assert.match(css, /prefers-reduced-motion: reduce[\s\S]*\.avatar-hero-media/);
  assert.match(css, /@supports not \(\(backdrop-filter/);
});

test("settings keep a design section for avatar hero only", () => {
  const nav = readFileSync("src/settingsNav.ts", "utf8");
  assert.match(nav, /id: "settings-design"/);
  const page = readFileSync("src/pages/SettingsPage.tsx", "utf8");
  assert.match(page, /id="settings-design"/);
  assert.match(page, /基準画像は \{AVATAR_HERO_BASE\.width\}×\{AVATAR_HERO_BASE\.height\}/);
  assert.doesNotMatch(page, /\bappearance\b|Liquid|テーマの切替|theme-liquid/);
  const detail = readFileSync("src/pages/AvatarDetailPage.tsx", "utf8");
  assert.match(detail, /AvatarHero/);
  assert.match(detail, /heroSize === "off"/);
  const types = readFileSync("src/types.ts", "utf8");
  assert.match(types, /avatarHero: "large" \| "small" \| "off"/);
  assert.match(types, /thumbnailPath: string \| null;/);
});
