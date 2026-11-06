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

test("avatar hero measures cached images and never defaults to wide before measuring", () => {
  const hero = readFileSync("src/components/AvatarHero.tsx", "utf8");
  // 未計測の既定は wide ではない
  assert.match(hero, /useState<AvatarHeroLayoutState>\("pending"\)/);
  assert.doesNotMatch(hero, /useState<AvatarHeroLayout>\("wide"\)/);
  // path/url/version 変更で pending にリセット
  assert.match(hero, /setLayout\("pending"\);\s*\}, \[path, url, version\]\);/);
  assert.doesNotMatch(hero, /setLayout\("wide"\)/);
  // complete 済み画像でも計測（onLoad を取り逃しても）
  assert.match(hero, /img\.complete/);
  assert.match(hero, /measure\(probeRef\.current\)/);
  assert.match(hero, /ref=\{probeRef\}/);
  // probe はレイアウトに関係なく常設、onLoad/onError を持つ
  const probe = hero.slice(hero.indexOf('className="avatar-hero-probe"') - 200, hero.indexOf('className="avatar-hero-probe"') + 300);
  assert.match(probe, /\{src \? \(/);
  assert.match(probe, /onLoad=\{onImageLoad\}/);
  assert.match(probe, /onError=/);
  // 未計測中は全幅 cover 画像を出さない
  assert.match(hero, /src && !pending \?/);
});

test("a custom hero image wins over the thumbnail and busts the cache by version", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const hero = await server.ssrLoadModule("/src/avatarHero.ts");
    const thumb = { thumbnailPath: "/thumbs/1.jpg", thumbnailUrl: "https://booth.pximg.net/1.jpg" };
    assert.deepEqual(hero.avatarHeroSource({ ...thumb, heroImagePath: null, heroImageVersion: null }), {
      path: "/thumbs/1.jpg",
      url: "https://booth.pximg.net/1.jpg",
      version: null,
    });
    // カスタムのときはリモートの URL に落ちない
    assert.deepEqual(hero.avatarHeroSource({ ...thumb, heroImagePath: "/hero/lime.png", heroImageVersion: 7 }), {
      path: "/hero/lime.png",
      url: null,
      version: 7,
    });
    assert.equal(hero.versionedHeroSrc("asset://a.png", 7), "asset://a.png?v=7");
    assert.equal(hero.versionedHeroSrc("asset://a.png?x=1", 7), "asset://a.png?x=1&v=7");
    assert.equal(hero.versionedHeroSrc("asset://a.png", null), "asset://a.png");
    assert.equal(hero.versionedHeroSrc(null, 7), null);
  } finally {
    await server.close();
  }
});

test("avatar detail passes the custom image to the hero and keeps the toolbar unchanged", () => {
  const detail = readFileSync("src/pages/AvatarDetailPage.tsx", "utf8");
  assert.match(detail, /avatarHeroSource\(detail\)/);
  assert.match(detail, /path=\{heroImage\.path\}/);
  assert.match(detail, /url=\{heroImage\.url\}/);
  assert.match(detail, /version=\{heroImage\.version\}/);
  assert.doesNotMatch(detail, /path=\{detail\.thumbnailPath\}/);
  // メニューはヒーローの中だけ。旧デザインの枝には出さない
  const heroBranch = detail.slice(detail.indexOf("<AvatarHero\n"), detail.indexOf("<AvatarHeroToolbar primary"));
  assert.match(heroBranch, /<AvatarHeroMenu/);
  assert.equal(detail.match(/<AvatarHeroMenu/g).length, 1);
  // 操作行は Unity / BOOTH / アイテムを追加 だけ
  const toolbar = detail.slice(detail.indexOf("function AvatarHeroToolbar"), detail.indexOf("function AvatarAssetsPanel"));
  assert.equal(toolbar.match(/<button\b/g).length, 1);
  assert.equal(toolbar.match(/<UnityOpenButton\b/g).length, 1);
  assert.doesNotMatch(toolbar, /画像を変更|サムネに戻す|AvatarHeroMenu/);
});

test("hero image menu stays quiet until hover or focus and draws no accent", () => {
  const menu = readFileSync("src/components/AvatarHeroMenu.tsx", "utf8");
  assert.match(menu, /画像を変更/);
  assert.match(menu, /サムネに戻す/);
  assert.match(menu, /disabled=\{!hasCustomImage\}/);
  assert.match(menu, /chooseHeroImage/);
  assert.match(menu, /role="menu"/);
  const api = readFileSync("src/api.ts", "utf8");
  assert.match(api, /extensions: \["png", "jpg", "jpeg", "webp"\]/);
  assert.match(api, /"set_avatar_hero_image", \{ avatarId, sourcePath \}/);
  assert.match(api, /"clear_avatar_hero_image", \{ avatarId \}/);
  const css = readFileSync("src/styles.css", "utf8");
  // .icon-button より後ろの定義に負けないこと
  assert.match(css, /\.icon-button\.avatar-hero-menu-trigger \{[^}]*opacity: 0/);
  assert.match(css, /\.avatar-hero:hover \.icon-button\.avatar-hero-menu-trigger/);
  assert.match(css, /\.avatar-hero-menu:focus-within \.icon-button\.avatar-hero-menu-trigger/);
  assert.match(css, /@media \(hover: none\) \{\s*\.icon-button\.avatar-hero-menu-trigger \{ opacity: 1; \}/);
  assert.match(css, /prefers-reduced-motion: reduce[\s\S]*\.icon-button\.avatar-hero-menu-trigger \{ transition: none; \}/);
  const block = css.slice(css.indexOf(".avatar-hero-menu {"), css.indexOf("/* ヒーロー直下の操作行"));
  assert.doesNotMatch(block, /--line-accent|--accent/);
});

test("hero images stay out of remote sync", () => {
  const sync = readFileSync("docs/remote-sync.md", "utf8");
  const excluded = sync.slice(sync.indexOf("## 含めないもの"), sync.indexOf("## 変更メタ"));
  assert.match(excluded, /アバターのカスタムヒーロー画像（ファイルとパス）/);
  for (const file of ["src-tauri/src/sync_export.rs", "src-tauri/src/sync_meta.rs"]) {
    const code = readFileSync(file, "utf8");
    const production = code.slice(0, code.indexOf("#[cfg(test)]"));
    assert.doesNotMatch(production, /hero_image/, file);
  }
});
