import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("product links follow boothLinkTarget and fall back to the system browser", async () => {
  const server = await createServer({
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
  });
  try {
    const { openBoothLinkWith } = await server.ssrLoadModule("/src/api.ts");
    const url = "https://booth.pm/ja/items/1000001";

    async function open(settings, openBuiltin) {
      const calls = [];
      const target = await openBoothLinkWith(url, {
        settings: async () => settings,
        openBuiltin: async (opened) => {
          calls.push(["builtin", opened]);
          if (openBuiltin) await openBuiltin(opened);
        },
        openExternal: async (opened) => {
          calls.push(["external", opened]);
        },
      });
      return { target, calls };
    }

    const external = await open({ builtinBrowser: true, boothLinkTarget: "external" });
    assert.equal(external.target, "external");
    assert.deepEqual(external.calls, [["external", url]]);

    const browserOff = await open({ builtinBrowser: false, boothLinkTarget: "builtin" });
    assert.equal(browserOff.target, "external");
    assert.deepEqual(browserOff.calls, [["external", url]]);

    const builtin = await open({ builtinBrowser: true, boothLinkTarget: "builtin" });
    assert.equal(builtin.target, "builtin");
    assert.deepEqual(builtin.calls, [["builtin", url]]);

    const busy = await open({ builtinBrowser: true, boothLinkTarget: "builtin" }, async () => {
      throw new Error("別のファイルをダウンロード中です");
    });
    assert.equal(busy.target, "external");
    assert.deepEqual(busy.calls, [
      ["builtin", url],
      ["external", url],
    ]);

    const disabled = await open({ builtinBrowser: true, boothLinkTarget: "builtin" }, async () => {
      throw new Error("内蔵ブラウザは設定で有効にしてから使います");
    });
    assert.equal(disabled.target, "external");

    const unreadable = await openBoothLinkWith(url, {
      settings: async () => {
        throw new Error("設定を読めません");
      },
      openBuiltin: async () => {
        throw new Error("内蔵ブラウザは呼ばない");
      },
      openExternal: async (opened) => {
        assert.equal(opened, url);
      },
    });
    assert.equal(unreadable, "external");

    const detail = readFileSync("src/pages/AvatarDetailPage.tsx", "utf8");
    const queue = readFileSync("src/pages/QueuePage.tsx", "utf8");
    const settingsPage = readFileSync("src/pages/SettingsPage.tsx", "utf8");
    const updater = readFileSync("src/components/AppUpdater.tsx", "utf8");
    assert.match(detail, /openBoothLink\(item\.url\)/);
    assert.doesNotMatch(detail, /openExternal\(/);
    assert.match(queue, /openBoothLink\(product\.url\)/);
    assert.doesNotMatch(queue, /openExternal\(/);
    assert.match(settingsPage, /商品ページを開くブラウザ/);
    assert.match(settingsPage, /settings\.builtinBrowser &&/);
    assert.match(settingsPage, /openExternal\(catalog\.sourceUrl\)/);
    assert.match(settingsPage, /openExternal\(catalog\.termsUrl\)/);
    assert.match(updater, /openExternal\("https:\/\/github.com\/KeisXR\/VRCLibMan\/releases"\)/);
    assert.doesNotMatch(updater, /openBoothLink/);
  } finally {
    await server.close();
  }
});
