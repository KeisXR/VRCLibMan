import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("avatar hero helpers normalize size, height, layout, and contrast", async () => {
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
    assert.equal(hero.AVATAR_HERO_WIDE_ASPECT, 1.6);
    assert.equal(hero.avatarHeroLayout(2400, 1200), "wide");
    assert.equal(hero.avatarHeroLayout(1024, 1024), "block");
    assert.equal(hero.avatarHeroLayout(819, 1024), "block");
    assert.equal(hero.avatarHeroLayout(0, 0), "wide");
    assert.equal(hero.avatarHeroLowRes("wide", 1024), true);
    assert.equal(hero.avatarHeroLowRes("wide", 2400), false);
    assert.equal(hero.avatarHeroLowRes("block", 600), false);
    assert.equal(hero.avatarHeroBlockAspect(819, 1024) >= 0.75, true);
    assert.equal(hero.readAssetsOpenPref({ getItem: () => null }), true);
    assert.equal(hero.readAssetsOpenPref({ getItem: () => "0" }), false);
    const store = { value: "", setItem(_k, v) { this.value = v; }, getItem() { return this.value; } };
    hero.writeAssetsOpenPref(false, store);
    assert.equal(store.value, "0");

    const lightTitle = hero.heroTextContrast([0x20, 0x22, 0x2b], [0xf5, 0xf6, 0xf8]);
    const lightSub = hero.heroTextContrast([0x5d, 0x61, 0x6c], [0xf5, 0xf6, 0xf8]);
    const darkTitle = hero.heroTextContrast([0xf3, 0xf3, 0xf5], [0x12, 0x12, 0x14]);
    const darkSub = hero.heroTextContrast([0xb4, 0xb4, 0xbe], [0x12, 0x12, 0x14]);
    assert.ok(lightTitle >= 4.5, `light title contrast ${lightTitle}`);
    assert.ok(lightSub >= 4.5, `light sub contrast ${lightSub}`);
    assert.ok(darkTitle >= 4.5, `dark title contrast ${darkTitle}`);
    assert.ok(darkSub >= 4.5, `dark sub contrast ${darkSub}`);
    const bare = hero.contrastRatio(
      hero.relativeLuminance(0x20, 0x22, 0x2b),
      hero.relativeLuminance(128, 128, 128),
    );
    assert.ok(bare < 4.5, `bare mid-gray should fail without fade: ${bare}`);
  } finally {
    await server.close();
  }
});

test("avatar hero CSS uses hybrid layouts, veil, and no curved accent on the title", () => {
  const css = readFileSync("src/styles.css", "utf8");
  assert.match(css, /\.avatar-hero\[data-size="large"\]\s*\{\s*--avatar-hero-h:\s*400px/);
  assert.match(css, /\.avatar-hero\[data-size="small"\]\s*\{\s*--avatar-hero-h:\s*320px/);
  assert.match(css, /@media \(max-width: 860px\)[\s\S]*--avatar-hero-h:\s*220px/);
  assert.match(css, /backdrop-filter:\s*blur\(2px\)/);
  assert.match(css, /backdrop-filter:\s*blur\(8px\)/);
  assert.match(css, /backdrop-filter:\s*blur\(24px\)/);
  assert.match(css, /color-mix\(in srgb, var\(--bg\) 45%, transparent\)/);
  assert.match(css, /color-mix\(in srgb, var\(--bg\) 82%, transparent\) 60%/);
  assert.match(css, /--avatar-hero-sub:\s*#5d616c/);
  assert.match(css, /--avatar-hero-sub:\s*#b4b4be/);
  assert.match(css, /\.avatar-hero-copy/);
  assert.match(css, /\.avatar-hero-nav/);
  assert.match(css, /\.avatar-hero-block/);
  assert.match(css, /\.avatar-hero-sharp/);
  assert.match(css, /border-radius:\s*12px/);
  assert.match(css, /\.avatar-hero-toolbar/);
  assert.match(css, /\.avatar-assets/);
  assert.doesNotMatch(css, /\.avatar-hero-scrim\s*\{/);
  assert.doesNotMatch(css, /\.avatar-hero-actions/);
  assert.match(css, /avatar-hero[\s\S]*border-radius:\s*0/);
  assert.match(css, /\.avatar-hero-title::before\s*\{\s*content:\s*none/);
  assert.match(css, /prefers-reduced-motion: reduce[\s\S]*\.avatar-hero-media/);
  assert.match(css, /@supports not \(\(backdrop-filter/);
});

test("avatar detail keeps actions and assets under the hero, not inside it", () => {
  const hero = readFileSync("src/components/AvatarHero.tsx", "utf8");
  assert.match(hero, /data-layout=\{layout\}/);
  assert.match(hero, /avatar-hero-sharp/);
  assert.doesNotMatch(hero, /\bactions\b/);
  const detail = readFileSync("src/pages/AvatarDetailPage.tsx", "utf8");
  assert.match(detail, /AvatarHero/);
  assert.match(detail, /heroSize === "off"/);
  assert.match(detail, /avatar-hero-toolbar/);
  assert.match(detail, /AvatarAssetsPanel/);
  assert.match(detail, /アセットファイル/);
  assert.match(detail, /BOOTH で開く/);
  assert.match(detail, /readAssetsOpenPref/);
  assert.doesNotMatch(detail, /バージョンを選んでダウンロード|新版をダウンロード/);
  assert.doesNotMatch(detail, /actions=\{addAction\}/);
});

test("settings keep a design section for avatar hero only", () => {
  const nav = readFileSync("src/settingsNav.ts", "utf8");
  assert.match(nav, /id: "settings-design"/);
  const page = readFileSync("src/pages/SettingsPage.tsx", "utf8");
  assert.match(page, /id="settings-design"/);
  assert.match(page, /基準画像は \{AVATAR_HERO_BASE\.width\}×\{AVATAR_HERO_BASE\.height\}/);
  assert.doesNotMatch(page, /\bappearance\b|Liquid|テーマの切替|theme-liquid/);
  const types = readFileSync("src/types.ts", "utf8");
  assert.match(types, /avatarHero: "large" \| "small" \| "off"/);
  assert.match(types, /thumbnailPath: string \| null;/);
});
