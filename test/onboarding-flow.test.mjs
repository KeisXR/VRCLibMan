import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

async function withModules(paths, run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(...(await Promise.all(paths.map((path) => server.ssrLoadModule(path)))));
  } finally {
    await server.close();
  }
}

const stats = (extra) => ({ products: 0, ownedAvatars: 0, unextracted: 0, unlinkedFiles: 0, registeredFiles: 0, ...extra });

test("finishing setup lands on the screen that fits the library", async () => {
  await withModules(["/src/pages/SetupWizard.tsx"], ({ setupLanding }) => {
    assert.deepEqual(
      [setupLanding(stats({}), false).hash, setupLanding(stats({}), false).section],
      ["#/import", "import-booth"],
      "no products yet: back to reading BOOTH",
    );
    assert.equal(setupLanding(null, false).hash, "#/import");
    const unsorted = setupLanding(stats({ products: 5, unextracted: 2 }), false);
    assert.deepEqual([unsorted.hash, unsorted.section], ["#/import", "import-classification"]);
    assert.match(unsorted.message, /未分類の商品が 2 件/);
    // 分類中なら、終わった商品から一覧に出るので探す画面へ移る。
    const running = setupLanding(stats({ products: 5, unextracted: 2, ownedAvatars: 1 }), true);
    assert.equal(running.hash, "#/");
    assert.match(running.message, /分類を続けています/);
    assert.equal(setupLanding(stats({ products: 5, ownedAvatars: 1 }), false).hash, "#/");
    assert.equal(setupLanding(stats({ products: 5 }), false).hash, "#/assets", "no avatar body: browse assets");
  });
});

test("the guide stops short of a right panel that reaches its height", async () => {
  await withModules(["/src/pages/SetupWizard.tsx"], ({ guideRightClearance }) => {
    const docked = { left: 974, top: 0, width: 306, height: 800 };
    assert.equal(guideRightClearance(1280, [docked], 600), 306);
    // 浮かせた小さいパネルが上だけにあるなら、下端の案内は避けなくてよい。
    assert.equal(guideRightClearance(900, [{ left: 568, top: 12, width: 320, height: 136 }], 500), 0);
    assert.equal(guideRightClearance(900, [{ left: 468, top: 12, width: 420, height: 616 }], 500), 432);
    assert.equal(guideRightClearance(1280, [{ left: 0, top: 0, width: 0, height: 0 }], 600), 0, "a closed panel takes no room");
    assert.equal(guideRightClearance(1280, [], 600), 0);
  });
});

test("guide styles keep the sidebar clear at every window size", () => {
  const css = readFileSync("src/styles.css", "utf8");
  const rule = css.slice(css.indexOf(".setup-guide {"), css.indexOf("}", css.indexOf(".setup-guide {")));
  assert.match(rule, /left:\s*max\(12px,\s*min\(calc\(var\(--guide-left/, "the guide starts right of the sidebar");
  assert.match(rule, /right:\s*calc\(var\(--guide-right/, "the guide ends before the right panel");
  assert.doesNotMatch(css, /\.setup-guide\.compact\s*\{/, "no separate placement for small windows");
});

test("a card download explains missing settings instead of failing", async () => {
  await withModules(["/src/downloads.ts"], ({ downloadBlocker }) => {
    const ready = { builtinBrowser: true, routeDownloads: true, libraryFolder: "C:\\Assets" };
    assert.equal(downloadBlocker(ready), null);
    assert.match(downloadBlocker({ ...ready, routeDownloads: false }), /ダウンロードを自動整理」をオンに/);
    assert.match(downloadBlocker({ ...ready, routeDownloads: false, libraryFolder: "  " }), /ライブラリのフォルダを選んで/);
    assert.match(downloadBlocker({ ...ready, builtinBrowser: false }), /内蔵ブラウザ/);
  });
});

test("a heading anchor scrolls its whole card into view", async () => {
  await withModules(["/src/anchorScroll.ts"], ({ anchorTarget }) => {
    const { document } = parseHTML(
      '<main><section class="settings-section" id="settings-library"><header><h2 id="import-booth">1</h2></header><div id="settings-app-update"></div></section></main>',
    );
    const card = document.querySelector("section");
    assert.equal(anchorTarget(document.getElementById("import-booth")), card, "the card top is not cut off");
    assert.equal(anchorTarget(card), card);
    assert.equal(anchorTarget(document.getElementById("settings-app-update")).id, "settings-app-update", "a block inside a card stays itself");
  });
});

test("links into settings go to their section, not the top of the page", () => {
  for (const file of ["src/pages/ImportPage.tsx", "src/pages/PurchasesPage.tsx", "src/App.tsx", "src/components/UnityOpenDialog.tsx"]) {
    const source = readFileSync(file, "utf8");
    assert.doesNotMatch(source, /href="#\/settings"/, `${file} uses SettingsLink`);
    assert.doesNotMatch(source, /hash = "#\/settings"/, `${file} uses openSettingsSection`);
  }
});
