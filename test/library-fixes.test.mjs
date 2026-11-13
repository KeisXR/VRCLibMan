import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
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

test("the acquisition dialog cannot confirm without a chosen kind and genre", async () => {
  await withModules(["/src/components/BrowserAutoClassification.tsx"], ({ autoClassificationReady, SUGGESTION_SOURCE_LABEL }) => {
    assert.equal(autoClassificationReady("", ""), false, "no blind default");
    assert.equal(autoClassificationReady("asset", ""), false, "an asset still needs its genre");
    assert.equal(autoClassificationReady("asset", "outfit"), true);
    assert.equal(autoClassificationReady("world", "skybox"), true);
    assert.equal(autoClassificationReady("avatar", ""), true, "an avatar body has no genre");
    assert.deepEqual(Object.keys(SUGGESTION_SOURCE_LABEL).sort(), ["booth", "classified", "rules"]);
  });
  const dialog = readFileSync("src/components/BrowserAutoClassification.tsx", "utf8");
  assert.doesNotMatch(dialog, /useState<[^>]*>\("asset"\)|useState\("outfit"\)/, "the dialog does not start from 衣装");
});

test("purchased edition choices stay inside the product's avatar scope", async () => {
  await withModules(["/src/components/ProductEditDialog.tsx"], ({ editionCandidates }) => {
    const index = [{ id: "lime" }, { id: "plum" }, { id: "manuka" }];
    assert.deepEqual(editionCandidates(index, new Set(["lime", "manuka"]), []).map(({ avatar }) => avatar.id), ["lime", "manuka"]);
    assert.deepEqual(editionCandidates(index, new Set(), []), [], "no scope: nothing to choose yet");
    // 範囲から外したあとも、選んでいた版は外せるように残す。
    const kept = editionCandidates(index, new Set(["lime"]), ["plum"]);
    assert.deepEqual(kept.map(({ avatar, outside }) => [avatar.id, outside]), [["lime", false], ["plum", true]]);
  });
});

test("the avatar list says what to do next when no avatar is owned yet", async () => {
  await withModules(["/src/pages/AvatarsPage.tsx"], ({ ownedAvatarsHint }) => {
    const stats = (extra) => ({ products: 0, ownedAvatars: 0, unextracted: 0, unlinkedFiles: 0, registeredFiles: 0, ...extra });
    assert.deepEqual(ownedAvatarsHint(stats({}), 0), { kind: "no-products" });
    assert.deepEqual(ownedAvatarsHint(stats({ products: 9, unextracted: 4 }), 0), { kind: "unclassified", count: 4 });
    assert.deepEqual(ownedAvatarsHint(stats({ products: 9 }), 0), { kind: "no-body" });
    assert.equal(ownedAvatarsHint(stats({ products: 9 }), 2), null, "owned avatars: no hint");
    assert.equal(ownedAvatarsHint(null, 0), null, "nothing is said before the counts load");
  });
});

test("irreversible library actions confirm, and exclusion can be undone", async () => {
  await withModules(["/src/pages/AvatarDetailPage.tsx"], ({ mergeConfirmText, EDITION_UNSET_HINT }) => {
    const text = mergeConfirmText("らいむ", "ライム");
    assert.match(text, /「らいむ」を「ライム」へ統合/);
    assert.match(text, /元に戻せません/);
    assert.match(EDITION_UNSET_HINT, /購入した対応版/, "the badge says where to fix it");
  });
  const detail = readFileSync("src/pages/AvatarDetailPage.tsx", "utf8");
  const merge = detail.slice(detail.indexOf("async function merge()"), detail.indexOf("const linkedIds"));
  assert.match(merge, /window\.confirm\(mergeConfirmText/);
  assert.match(detail, /item\.editionUnset && <span className="badge" title=\{EDITION_UNSET_HINT\}>/, "the badge follows the Rust flag");
  const settings = readFileSync("src/pages/SettingsPage.tsx", "utf8");
  for (const name of ["removeAvatar", "removeBody"]) {
    const body = settings.slice(settings.indexOf(`async function ${name}()`), settings.indexOf("try {", settings.indexOf(`async function ${name}()`)));
    assert.match(body, /window\.confirm\(/, `${name} asks first`);
  }
  const exclusions = readFileSync("src/components/ProductExclusions.tsx", "utf8");
  assert.equal((exclusions.match(/restoreExcluded\(/g) ?? []).length, 3, "both exclusion paths offer 元に戻す");
});
