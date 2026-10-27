import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

const delay = () => new Promise((resolve) => setTimeout(resolve, 10));

test("setup, import and version flows with synthetic local data", async (t) => {
  const { window } = parseHTML(
    '<!doctype html><html><body><div id="root"></div></body></html>',
  );
  window.location = new URL("http://127.0.0.1:1420/");
  window.confirm = () => true;
  for (const name of [
    "window",
    "document",
    "location",
    "HTMLElement",
    "HTMLInputElement",
    "Event",
    "CustomEvent",
    "EventTarget",
  ]) {
    Object.defineProperty(globalThis, name, {
      configurable: true,
      writable: true,
      value: window[name],
    });
  }
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: { userAgent: "test", clipboard: { writeText: async () => {} } },
  });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
  });
  let root;
  try {
    await server.ssrLoadModule("/src/preview/api.ts");
    await server.ssrLoadModule("/package.json");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { default: App } = await server.ssrLoadModule("/src/App.tsx");
    const { extensionVersionMessage } =
      await server.ssrLoadModule("/src/versions.ts");
    const { modelInstalled } = await server.ssrLoadModule("/src/models.ts");
    const wait = async (check) => {
      for (let i = 0; i < 300; i++) {
        if (await check()) return;
        await act(delay);
      }
      assert.fail(
        `DOM did not settle: ${document.body.textContent.slice(0, 600)}`,
      );
    };
    const text = () => document.body.textContent;
    const button = (name, scope = document) =>
      [...scope.querySelectorAll("button")].find(
        (node) => node.textContent === name,
      );
    const typeInto = async (node, value) => {
      await act(async () => {
        const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
        setter.call(node, value);
        const propsKey = Object.keys(node).find((name) => name.startsWith("__reactProps$"));
        const onChange = propsKey ? node[propsKey].onChange : null;
        if (onChange) onChange({ target: node, currentTarget: node });
        else node.dispatchEvent(new window.Event("input", { bubbles: true }));
        await delay();
      });
    };
    const click = async (node) => {
      assert.ok(node, "click target exists");
      assert.ok(!node.disabled, "click target is enabled");
      await act(async () => {
        if (node.type === "radio") node.checked = true;
        node.dispatchEvent(new window.Event("click", { bubbles: true }));
        await delay();
      });
    };
    const radio = (name, scope = document) =>
      [...scope.querySelectorAll("label")]
        .find((label) => label.textContent.includes(name))
        ?.querySelector('input[type="radio"]');
    const route = async (hash) =>
      act(async () => {
        location.hash = hash;
        window.dispatchEvent(new window.Event("hashchange"));
        await delay();
      });
    const pulls = [];
    const originalPull = api.pullOllamaModel;
    api.pullOllamaModel = async (model) => {
      pulls.push(model);
      return originalPull(model);
    };
    root = createRoot(document.getElementById("root"));
    await act(async () => root.render(createElement(App)));
    await wait(
      () =>
        document.querySelector(".setup-screen") &&
        button("次へ") &&
        !button("次へ").disabled,
    );

    await t.test(
      "an installed default does not hide candidates; only confirmed target is pulled",
      async () => {
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
        assert.equal(
          document.querySelectorAll(".model-picker input[type=radio]").length,
          4,
        );
        await click(radio("qwen3.5:4b"));
        assert.deepEqual(pulls, []);
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
        window.confirm = () => false;
        await click(button("ダウンロードする"));
        assert.deepEqual(pulls, []);
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
        let prompt = "";
        window.confirm = (message) => {
          prompt = message;
          return true;
        };
        await click(button("ダウンロードする"));
        await wait(() => text().includes("qwen3.5:4b は導入済み"));
        assert.deepEqual(pulls, ["qwen3.5:4b"]);
        assert.match(prompt, /3\.3〜4\.0 GB/);
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:4b");
        await click(radio("qwen3.5:9b"));
        await click(button("次へ"));
        await wait(() => radio("アプリの内蔵ブラウザ"));
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
        assert.deepEqual(pulls, ["qwen3.5:4b"]);
        assert.ok(modelInstalled(["custom:latest"], "custom"));
        assert.ok(!modelInstalled(["qwen3.5:4b-extra"], "qwen3.5:4b"));
      },
    );

    await t.test(
      "import choices persist without altering model or exposing browser-only pairing",
      async () => {
        window.confirm = () => false;
        await click(radio("アプリの内蔵ブラウザ"));
        assert.equal((await api.integrationStatus()).mode, "extension");
        window.confirm = () => true;
        await click(radio("アプリの内蔵ブラウザ"));
        await wait(
          () =>
            !document
              .querySelector(".setup-screen")
              .textContent.includes("ペアリングコード"),
        );
        assert.equal((await api.integrationStatus()).mode, "browser");
        assert.ok((await api.settings()).builtinBrowser);
        assert.ok(document.querySelector('.setup-screen a[href="#/browser"]'));
        await click(radio("両方使う"));
        await wait(() =>
          document
            .querySelector(".setup-screen")
            .textContent.includes("ペアリングコード"),
        );
        assert.equal((await api.integrationStatus()).mode, "both");
        await click(radio("Chrome / Edge の拡張機能"));
        assert.ok(!(await api.settings()).builtinBrowser);
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
        await click(button("次へ", document.querySelector(".setup-screen")));
        await click(button("あとで設定で選ぶ"));
        await wait(() => !document.querySelector(".setup-screen"));
      },
    );

    await t.test(
      "import follows the primary flow; destructive reclassification requires confirmation and can stop",
      async () => {
        await route("#/import");
        await wait(() => text().includes("最後の受信:"));
        assert.deepEqual(
          [
            ...document.querySelectorAll("main .settings-section > header h2"),
          ].map((h) => h.textContent),
          ["1. BOOTH から読み込む", "2. 分類", "3. 手元のファイル", "その他"],
        );
        assert.ok(text().includes("決まらない商品だけ分類モデルへ渡します"));
        const drop = document.querySelector(".drop");
        assert.ok(drop.closest("details"));
        assert.ok(!drop.closest("details").hasAttribute("open"));
        let forcedCalls = 0;
        let finish;
        api.extractProducts = (target) => {
          assert.equal(target, "all");
          forcedCalls++;
          return new Promise((resolve) => {
            finish = resolve;
          });
        };
        window.confirm = () => false;
        await click(button("すべて再分類"));
        assert.equal(forcedCalls, 0);
        window.confirm = () => true;
        await click(button("すべて再分類"));
        assert.equal(forcedCalls, 1);
        let stopped = false;
        api.cancelExtract = async () => {
          stopped = true;
          finish({ processed: 1, failed: 0, cancelled: true, errors: [] });
        };
        await click(button("中断"));
        assert.ok(stopped);
        await wait(() => !button("すべて再分類").disabled);
        api.extractProducts = async () => ({
          processed: 6,
          failed: 2,
          cancelled: false,
          errors: ["商品A: 分類モデルの応答の詳細", "商品B: 接続エラーの詳細"],
          boothSettled: 4,
          modelUsed: 2,
        });
        await click(button("すべて再分類"));
        await wait(() => document.querySelector(".banner.error")?.textContent.includes("2 件"));
        assert.ok(!document.querySelector(".banner.error").textContent.includes("商品A"));
        const stats = [...document.querySelectorAll("main p.meta")].find((node) =>
          node.textContent.includes("カテゴリとタグで 4 件、モデルで 2 件"),
        );
        assert.ok(stats);
        const errorDetails = [...document.querySelectorAll("main details")].find((node) => node.querySelector("summary")?.textContent === "分類結果の詳細");
        assert.ok(errorDetails);
        assert.ok(!errorDetails.hasAttribute("open"));
        assert.ok(errorDetails.textContent.includes("商品A: 分類モデルの応答の詳細"));
        const event = new window.Event("drop", {
          bubbles: true,
          cancelable: true,
        });
        event.dataTransfer = {
          files: [
            {
              text: async () =>
                JSON.stringify([
                  {
                    boothId: "9991234",
                    title: "ダミーの帽子",
                    shop: "テスト",
                    descriptionSnippet: "テスト用",
                    url: "https://booth.pm/ja/items/9991234",
                  },
                ]),
            },
          ],
        };
        await act(async () => {
          drop.dispatchEvent(event);
          await delay();
        });
        await wait(() => text().includes("直前の JSON インポート結果"));
        assert.ok(text().includes("無料商品を登録"));
        assert.ok(button("サムネイルを保存"));
      },
    );

    await t.test(
      "settings and sidebar read build versions; mismatch and unknown versions are distinguished",
      async () => {
        await route("#/settings");
        await wait(() => text().includes("最後に通信した拡張"));
        const manifest = JSON.parse(
          readFileSync("extension/manifest.json", "utf8"),
        );
        const packageJson = JSON.parse(readFileSync("package.json", "utf8"));
        assert.equal(
          document.querySelector(".app-version").textContent,
          `v${packageJson.version}`,
        );
        assert.ok(text().includes(`アプリ v${packageJson.version}`));
        assert.ok(text().includes(`推奨する拡張 v${manifest.version}`));
        assert.ok(text().includes("拡張を更新してください"));
        assert.match(extensionVersionMessage("0.1.12", "0.1.12"), /一致/);
        assert.match(extensionVersionMessage("0.1.12", "0.1.11"), /更新/);
        assert.match(extensionVersionMessage("0.1.12", null), /未確認/);
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
      },
    );
    await t.test(
      "settings selections apply immediately without a save button",
      async () => {
        assert.equal(button("設定を保存"), undefined);
        assert.ok(text().includes("一般設定は自動保存"));
        assert.ok(text().includes("対応表の編集は保存ボタンで確定"));
        await click(radio("qwen3.5:4b"));
        await wait(async () => (await api.settings()).ollamaModel === "qwen3.5:4b");
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:4b");
        await click(radio("qwen3.5:9b"));
        await wait(async () => (await api.settings()).ollamaModel === "qwen3.5:9b");
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
        assert.equal(pulls.length, 1);
      },
    );

    await t.test("debug export copies classification feedback without secrets", async () => {
      await route("#/settings");
      await wait(() => document.querySelector("details.debug-section"));
      const details = document.querySelector("details.debug-section");
      assert.equal(details.hasAttribute("open"), false);
      assert.ok(details.querySelector("summary").textContent.includes("デバッグ"));
      assert.ok(button("分類改善用データを書き出す", details));
      assert.ok(button("クリップボードにコピー", details));
      const scope = details.querySelector(".scope-choices");
      assert.equal(scope.querySelector('input[value="both"]').checked, true);
      let copied = "";
      navigator.clipboard.writeText = async (value) => {
        copied = String(value);
      };
      const copy = () => click(button("クリップボードにコピー", details));
      const read = async () => {
        copied = "";
        await copy();
        await wait(() => copied.length > 0 && !button("クリップボードにコピー", details).disabled);
        return JSON.parse(copied);
      };
      const keys = (value, found = []) => {
        if (Array.isArray(value)) value.forEach((item) => keys(item, found));
        else if (value && typeof value === "object") {
          for (const [key, child] of Object.entries(value)) {
            found.push(key);
            keys(child, found);
          }
        }
        return found;
      };
      const both = await read();
      assert.equal(both.format, "vrclibman.classification-feedback");
      assert.equal(both.scope, "both");
      assert.equal(both.descriptionHeadChars, 400);
      assert.ok(both.purpose.includes("トークン"));
      assert.ok(both.products.some((item) => item.boothId === "8800105"));
      assert.ok(both.products.some((item) => item.boothId === "8800201"));
      assert.ok(both.products.every((item) => item.category == null || item.category === "other"));
      const otherProduct = both.products.find((item) => item.boothId === "8800201");
      assert.equal(otherProduct.title, "分類できなかった小物");
      assert.equal(otherProduct.shop, "サンプル工房");
      assert.equal(otherProduct.category, "other");
      assert.equal(otherProduct.classificationStatus, "classified");
      assert.equal([...otherProduct.descriptionHead].length, 400);
      assert.equal(otherProduct.descriptionHead.includes("末尾マーカー"), false);
      const unclassified = both.products.find((item) => item.boothId === "8800105");
      assert.equal(unclassified.classificationStatus, "unclassified");
      assert.equal(unclassified.category, null);
      assert.deepEqual(
        both.manualClassifications.map((item) => item.boothId),
        ["8800131", "8800202"],
      );
      assert.ok(both.manualClassifications.every((item) => item.classificationStatus === "manual"));
      assert.equal(both.corrections[0].boothId, "8800202");
      assert.equal(both.corrections[0].action, "move");
      assert.equal(both.corrections[0].from.category, "other");
      assert.equal(both.corrections[0].to.genre, "hair");
      for (const secret of ["C:\\Assets", "\\\\nas", "K7NP-4Q2M", "年齢制限", "thumbnail", "unitypackage"]) {
        assert.equal(copied.includes(secret), false, secret);
      }
      for (const key of ["url", "thumbnailUrl", "thumbnailPath", "path", "token", "localPath"]) {
        assert.equal(keys(both).includes(key), false, key);
      }

      await click(scope.querySelector('input[value="other"]'));
      const otherOnly = await read();
      assert.ok(otherOnly.products.some((item) => item.boothId === "8800201"));
      assert.ok(otherOnly.products.every((item) => item.category === "other"));
      assert.equal(otherOnly.manualClassifications.length, 2);
      await click(scope.querySelector('input[value="unclassified"]'));
      const unclassifiedOnly = await read();
      assert.ok(unclassifiedOnly.products.some((item) => item.boothId === "8800105"));
      assert.ok(unclassifiedOnly.products.every((item) => item.category == null));
      await click(scope.querySelector('input[value="both"]'));

      const before = both.corrections.length;
      const snapshot = await api.moveProductGenre("8800201", "asset", "hair");
      await api.restoreProductClassification(snapshot);
      const moved = await read();
      assert.ok(moved.corrections.some((row) => row.boothId === "8800201" && row.action === "move" && row.to.genre === "hair"));
      assert.ok(moved.corrections.some((row) => row.boothId === "8800201" && row.action === "restore" && row.to.category === "other"));
      assert.ok(moved.products.some((item) => item.boothId === "8800201" && item.category === "other"));
      const pin = await api.unpinProductClassification("8800202");
      await api.restoreProductClassification(pin);
      const unpinned = await read();
      assert.ok(unpinned.corrections.some((row) => row.boothId === "8800202" && row.action === "unpin" && row.to == null));
      assert.ok(unpinned.manualClassifications.some((item) => item.boothId === "8800202"));
      assert.ok(unpinned.corrections.length > before);
    });

    await t.test(
      "irrelevant products can be excluded, remain excluded on reimport, and be restored",
      async () => {
        await route("#/import");
        await wait(() => document.querySelector(".exclusion-list"));
        const row = () =>
          [...document.querySelectorAll(".exclusion-list li")].find((node) =>
            node.textContent.includes("9991234"),
          );
        await wait(row);
        const before = await api.stats();
        await click(button("除外", row()));
        await wait(() => button("元に戻す", row()));
        assert.equal((await api.stats()).products, before.products - 1);
        assert.ok(
          !(await api.productOptions()).some((p) => p.boothId === "9991234"),
        );
        await act(async () => {
          await api.importExport(
            JSON.stringify([
              {
                boothId: "9991234",
                title: "ダミーの帽子",
                shop: "テスト",
                descriptionSnippet: "更新した説明",
              },
            ]),
          );
          await delay();
        });
        assert.ok(
          (await api.managedProducts()).find((p) => p.boothId === "9991234")
            .excluded,
        );
        assert.ok(
          !(await api.productOptions()).some((p) => p.boothId === "9991234"),
        );
        await click(button("元に戻す", row()));
        await wait(() => button("除外", row()));
        assert.equal((await api.stats()).products, before.products);
        assert.ok(
          (await api.productOptions()).some((p) => p.boothId === "9991234"),
        );
      },
    );
    await t.test("product exclusion requires opening other actions first", async () => {
      const worlds = await api.productsByCategory("world");
      const target = worlds[0];
      await route("#/worlds");
      const card = () => [...document.querySelectorAll(".item")].find((node) => node.textContent.includes(target.title));
      await wait(card);
      const trigger = () => card().querySelector(".product-actions-trigger");
      assert.equal(button("管理対象から除外", card()), undefined);
      await click(trigger());
      assert.equal(trigger().getAttribute("aria-expanded"), "true");
      assert.ok(button("管理対象から除外", card()));
      assert.ok(!(await api.managedProducts()).find((p) => p.boothId === target.boothId).excluded);
      await act(async () => {
        const event = new window.Event("keydown", { bubbles: true });
        event.key = "Escape";
        document.dispatchEvent(event);
        await delay();
      });
      assert.equal(trigger().getAttribute("aria-expanded"), "false");
      await click(trigger());
      await act(async () => {
        document.body.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
        await delay();
      });
      assert.equal(button("管理対象から除外", card()), undefined);
      await click(trigger());
      await click(button("管理対象から除外", card()));
      await wait(() => !card());
      assert.ok((await api.managedProducts()).find((p) => p.boothId === target.boothId).excluded);
      await api.setProductExcluded(target.boothId, false);
    });
    await t.test(
      "avatar pages separate adjustment candidates and keep worlds in their library",
      async () => {
        await route("#/");
        await wait(() => document.querySelector(".avatar-card"));
        assert.ok(
          [...document.querySelectorAll(".avatar-card .pill")].every(
            (node) => !node.textContent.includes("ワールド"),
          ),
        );
        const expected = await api.avatar("manuka");
        assert.ok(expected.unsupportedOutfits.length > 0);
        assert.ok(
          expected.sections.every((section) => section.category !== "world"),
        );
        assert.ok(
          expected.genericItems.every((item) => item.category !== "world"),
        );
        const worlds = await api.productsByCategory("world");
        assert.ok(worlds.length > 0);
        await route("#/worlds");
        await wait(
          () => document.querySelector("main h1")?.textContent === "ワールド",
        );
        await wait(() => text().includes(worlds[0].title));
        await route("#/avatar/manuka");
        await wait(() => document.querySelector(".unsupported-outfits"));
        const candidates = document.querySelector(".unsupported-outfits");
        assert.ok(
          candidates
            .querySelector("summary")
            .textContent.includes(String(expected.unsupportedOutfits.length)),
        );
        assert.equal(candidates.querySelectorAll(".item").length, 0);
        await act(async () => {
          candidates.open = true;
          candidates.dispatchEvent(new window.Event("toggle"));
          await delay();
        });
        await wait(() => candidates.querySelector(".item"));
        assert.equal(
          candidates.querySelectorAll(".item").length,
          Math.min(30, expected.unsupportedOutfits.length),
        );
        assert.ok(candidates.textContent.includes("もちふぃった～"));
        assert.ok(
          candidates.textContent.includes(expected.unsupportedOutfits[0].title),
        );
        await route("#/avatar/lime");
        await wait(
          () => document.querySelector("main h1")?.textContent === "ライム",
        );
        await wait(
          () =>
            document.querySelector(".unsupported-outfits") == null ||
            !document.querySelector(".unsupported-outfits").open,
        );
        assert.ok(
          !document
            .querySelector(".unsupported-outfits")
            ?.querySelector(".item"),
        );
      },
    );
    await t.test(
      "asset page browses products by genre through the hash route",
      async () => {
        const main = () => document.querySelector("main");
        const tabs = () => [...document.querySelectorAll(".genre-rail-nav a")];
        const activeTab = () => document.querySelector('.genre-rail-nav a[aria-current="page"]');
        assert.ok(document.querySelector('.rail a[href="#/assets"]'));
        const all = await api.productsByGenre("asset");
        assert.ok(all.total > 0);
        assert.ok(all.items.every((item) => item.category && item.category !== "avatar" && item.category !== "world"));
        await route("#/assets");
        await wait(() => main().querySelector("h1")?.textContent === "アセット");
        await wait(() => main().querySelector(".item"));
        assert.equal(tabs().length, 16);
        assert.equal(activeTab().textContent, `すべて${all.total}`);
        assert.equal(main().querySelectorAll(".item").length, all.items.length);
        assert.ok(!main().textContent.includes("見本のワールド小物"));
        assert.ok(!main().textContent.includes("説明がまだないアクセサリ"));

        const eye = await api.productsByGenre("asset", "eye_texture");
        assert.ok(eye.items.length > 0);
        assert.ok(eye.items.every((item) => item.genre === "eye_texture"));
        assert.equal(eye.total, all.total);
        assert.ok(tabs().some((node) => node.getAttribute("href") === "#/assets/eye_texture"));
        await route("#/assets/eye_texture");
        await wait(() => activeTab()?.textContent.startsWith("アイテクスチャ"));
        await wait(() => main().querySelectorAll(".item").length === eye.items.length);
        assert.ok(main().textContent.includes(eye.items[0].title));
        assert.ok(activeTab().textContent.endsWith(String(eye.counts.eye_texture)));

        await route("#/assets/pose");
        await wait(() => main().textContent.includes("「ポーズ」の商品はまだありません。"));
        assert.ok(activeTab().classList.contains("genre-link-zero"));

        await route("#/assets/not-a-genre");
        await wait(() => main().textContent.includes("指定されたジャンルが見つからないため"));
        await wait(() => activeTab()?.textContent.startsWith("すべて"));

        const { filterItems, genreHref } = await server.ssrLoadModule("/src/components/GenreBrowser.tsx");
        assert.equal(filterItems(all.items, "一致しない見本文字列").length, 0);
        assert.equal(filterItems(all.items, "  ").length, all.items.length);
        assert.ok(filterItems(all.items, eye.items[0].shop).length > 0);
        assert.equal(genreHref("#/assets", null), "#/assets");
        assert.equal(genreHref("#/assets", "eye_texture"), "#/assets/eye_texture");
      },
    );
    await t.test(
      "world page browses world products by genre and keeps the old route",
      async () => {
        const main = () => document.querySelector("main");
        const tabs = () => [...document.querySelectorAll(".genre-rail-nav a")];
        const activeTab = () => document.querySelector('.genre-rail-nav a[aria-current="page"]');
        const items = () => main().querySelectorAll(".item");
        const legacy = await api.productsByCategory("world");
        const all = await api.productsByGenre("world");
        assert.equal(all.total, legacy.length);
        assert.deepEqual(
          all.items.map((item) => item.boothId).sort(),
          legacy.map((item) => item.boothId).sort(),
        );
        assert.ok(all.items.every((item) => item.category === "world"));
        assert.ok(all.counts.unset > 0);

        await route("#/worlds");
        await wait(() => main().querySelector("h1")?.textContent === "ワールド");
        await wait(() => items().length === all.items.length);
        assert.ok(document.querySelector('.rail a[href="#/worlds"]').classList.contains("active"));
        assert.equal(activeTab().textContent, `すべて${all.total}`);
        // すべて + 10 world genres + 無料 + ジャンル未設定 (未設定は件数があるときだけ)
        assert.equal(tabs().length, 13);
        assert.ok(tabs().some((node) => node.getAttribute("href") === "#/worlds/furniture_props"));
        assert.ok(tabs().some((node) => node.getAttribute("href") === "#/worlds/unset"));
        assert.ok(tabs().some((node) => node.getAttribute("href") === "#/worlds/free"));
        assert.ok(main().querySelector(".product-actions-trigger"));

        const sky = await api.productsByGenre("world", "skybox");
        assert.ok(sky.items.length > 0);
        assert.ok(sky.items.every((item) => item.genre === "skybox"));
        await route("#/worlds/skybox");
        await wait(() => activeTab()?.textContent.startsWith("Skybox"));
        await wait(() => items().length === sky.items.length);
        assert.ok(main().textContent.includes(sky.items[0].title));
        assert.ok(document.querySelector('.rail a[href="#/worlds"]').classList.contains("active"));

        const unset = await api.productsByGenre("world", "unset");
        assert.equal(unset.items.length, all.counts.unset);
        assert.ok(unset.items.every((item) => item.genre == null));
        await route("#/worlds/unset");
        await wait(() => activeTab()?.textContent.startsWith("ジャンル未設定"));
        await wait(() => items().length === unset.items.length);
        assert.ok(main().textContent.includes("ジャンルが決まっていないワールドの商品です。"));
        assert.ok(!main().textContent.includes(sky.items[0].title));

        await route("#/worlds/lighting_postprocess");
        await wait(() => main().textContent.includes("「ライティング・ポストプロセス」の商品はまだありません。"));
        assert.ok(activeTab().classList.contains("genre-link-zero"));

        await route("#/worlds/hair");
        await wait(() => main().textContent.includes("指定されたジャンルが見つからないため"));
        await wait(() => activeTab()?.textContent.startsWith("すべて"));

        const assets = await api.productsByGenre("asset");
        assert.ok(!assets.counts.unset);
        await route("#/assets");
        await wait(() => main().querySelector("h1")?.textContent === "アセット");
        await wait(() => tabs().length === 16);
        assert.ok(!tabs().some((node) => node.getAttribute("href") === "#/assets/unset"));
        assert.ok(tabs().some((node) => node.getAttribute("href") === "#/assets/free"));

        const freeAssets = await api.productsByGenre("asset", "free");
        assert.ok(freeAssets.items.some((item) => item.boothId === "8800111" && item.isFree));
        assert.ok(freeAssets.items.every((item) => item.isFree && !item.classificationPinned));
        assert.ok(!freeAssets.items.some((item) => item.boothId === "8800130" || item.boothId === "8800131"));
        const keptAccessory = await api.productsByGenre("asset", "accessory");
        assert.ok(keptAccessory.items.some((item) => item.boothId === "8800130" && item.isFree));
        assert.ok(keptAccessory.items.some((item) => item.boothId === "8800132" && item.unpublished));
        assert.ok(!keptAccessory.items.some((item) => item.boothId === "8800111"));
        const pinnedFree = await api.productsByGenre("asset", "outfit");
        assert.ok(pinnedFree.items.some((item) => item.boothId === "8800131" && item.classificationPinned && item.isFree));
        const freeWorlds = await api.productsByGenre("world", "free");
        assert.ok(freeWorlds.items.some((item) => item.boothId === "8901004" && item.unpublished));
        assert.ok(!freeWorlds.items.some((item) => item.boothId === "8901001"));
        const furniture = await api.productsByGenre("world", "furniture_props");
        assert.ok(!furniture.items.some((item) => item.boothId === "8901004"));

        await route("#/assets/free");
        await wait(() => activeTab()?.textContent.startsWith("無料"));
        await wait(() => main().textContent.includes("無料のリボン"));
        await wait(() => main().textContent.includes("ダウンロードされていない無料のアセット"));
        assert.ok(!main().textContent.includes("手元にある無料の小物"));
        assert.ok(!main().textContent.includes("手動で移した無料の小物"));
        assert.equal(activeTab().getAttribute("data-drop-genre"), null);
        const freeSearch = main().querySelector(".genre-list-search");
        await typeInto(freeSearch, "一致しない見本文字列");
        await wait(() => main().textContent.includes("条件に合う商品はありません"));
        await typeInto(freeSearch, "リボン");
        await wait(() => main().textContent.includes("無料のリボン"));
        assert.equal(main().querySelectorAll(".item").length, 1);
        await typeInto(freeSearch, "");

        await route("#/assets/accessory");
        await wait(() => main().textContent.includes("非公開の見本アクセサリ"));
        const hidden = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("非公開の見本アクセサリ"));
        assert.ok(hidden.classList.contains("item-unpublished"));
        assert.ok([...hidden.querySelectorAll(".badge")].some((node) => node.textContent === "非公開"));
        const booth = [...hidden.querySelectorAll("button")].find((node) => node.textContent === "BOOTH");
        assert.ok(booth && !booth.disabled);
        assert.ok(main().textContent.includes("手元にある無料の小物"));
        assert.ok([...main().querySelectorAll(".badge")].some((node) => node.textContent === "無料"));

        await route("#/worlds/free");
        await wait(() => main().textContent.includes("未入手の無料ワールド小物"));
        const hiddenWorld = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("未入手の無料ワールド小物"));
        assert.ok(hiddenWorld.classList.contains("item-unpublished"));
        assert.ok([...hiddenWorld.querySelectorAll(".badge")].some((node) => node.textContent === "非公開"));
        assert.ok([...hiddenWorld.querySelectorAll(".badge")].some((node) => node.textContent === "無料"));
        assert.equal(document.querySelector('.genre-rail-nav a[aria-current="page"]')?.getAttribute("href"), "#/worlds/free");

        await route("#/avatar/lime");
        await wait(() => main().textContent.includes("非公開の見本アクセサリ"));
        const detailCard = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("非公開の見本アクセサリ"));
        assert.ok(detailCard.classList.contains("item-unpublished"));
        assert.ok([...detailCard.querySelectorAll("button")].some((node) => node.textContent === "BOOTH" && !node.disabled));

        const { selectedGenre, filterItems } = await server.ssrLoadModule("/src/components/GenreBrowser.tsx");
        assert.equal(selectedGenre("world", "skybox"), "skybox");
        assert.equal(selectedGenre("world", "unset"), "unset");
        assert.equal(selectedGenre("world", "free"), "free");
        assert.equal(selectedGenre("asset", "free"), "free");
        assert.equal(selectedGenre("world", "hair"), null);
        assert.equal(selectedGenre("asset", "skybox"), null);
        assert.equal(selectedGenre("asset", null), null);
        assert.equal(filterItems(freeAssets.items, "リボン").length, 1);
        assert.equal(filterItems(freeAssets.items, "一致しない見本文字列").length, 0);
      },
    );
    await t.test(
      "import guidance follows the selected method and logout explains its deletion scope",
      async () => {
        const { requestRefresh } = await server.ssrLoadModule("/src/api.ts");
        await act(async () => {
          await api.setBoothImportMode("browser");
          requestRefresh();
          await delay();
        });
        await route("#/import");
        await wait(() => document.querySelector('main a[href="#/browser"]'));
        await wait(() => !document.querySelector("main").textContent.includes("ペアリング"));
        assert.ok(!document.querySelector("main").textContent.includes("拡張機能の使い方・受信履歴"));
        await route("#/browser");
        await wait(() => document.querySelector("main").textContent.includes("3秒以上あけて確認します"));
        assert.ok(document.querySelector("main").textContent.includes("年齢制限の商品は確認しません"));
        await route("#/import");
        await wait(() => document.querySelector(".account-menu .account-pill"));
        await click(document.querySelector(".account-menu .account-pill"));
        await click(button("ログアウト", document.querySelector(".account-popover")));
        assert.match(document.querySelector(".account-confirm").textContent, /全サイト.*閲覧データ.*削除/);
        await click(button("キャンセル", document.querySelector(".account-confirm")));
        await act(async () => {
          document.body.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
          await api.setBoothImportMode("extension");
          requestRefresh();
          await delay();
        });
        await wait(() => document.querySelector("main").textContent.includes("拡張機能の使い方・受信履歴"));
        assert.ok(!document.querySelector('main a[href="#/browser"]'));
      },
    );
    await t.test("genre move menu pins a product and can be undone from the notice", async () => {
      const { requestRefresh } = await server.ssrLoadModule("/src/api.ts");
      const main = () => document.querySelector("main");
      const refresh = async () => {
        await act(async () => {
          requestRefresh();
          await delay();
        });
      };
      await route("#/assets/eye_texture");
      await wait(() => main().textContent.includes("瞳のテクスチャ集"));
      const all = document.querySelector('.genre-rail-nav a[href="#/assets"]');
      const pose = document.querySelector('.genre-rail-nav a[href="#/assets/pose"]');
      assert.equal(all.getAttribute("data-drop-genre"), null);
      assert.equal(pose.getAttribute("data-drop-genre"), "pose");
      assert.ok(pose.classList.contains("genre-link-zero"));
      assert.equal(document.querySelector(".genre-rail-other"), null);
      assert.equal(document.querySelector(".item").getAttribute("draggable"), "true");

      const card = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("瞳のテクスチャ集"));
      await click(card.querySelector(".product-actions-trigger"));
      await wait(() => button("移動", card));
      await click(button("移動", card));
      const panel = card.querySelector(".genre-move-panel");
      assert.ok(panel);
      assert.ok(button("アセット", panel));
      assert.ok(button("ワールド", panel));
      await click(button("ポーズ", panel));
      await wait(() => button("元に戻す"));
      await wait(() => !main().textContent.includes("瞳のテクスチャ集"));

      await route("#/assets/pose");
      await wait(() => main().textContent.includes("瞳のテクスチャ集"));
      await wait(() => [...main().querySelectorAll(".badge")].some((node) => node.textContent === "手動"));
      const moved = await api.productsByGenre("asset", "pose");
      assert.ok(moved.items.some((item) => item.classificationPinned && item.genre === "pose" && item.category === "gimmick"));

      await click(button("元に戻す"));
      await refresh();
      await route("#/assets/eye_texture");
      await wait(() => main().textContent.includes("瞳のテクスチャ集"));
      assert.ok(!main().textContent.includes("手動"));

      const dragged = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("瞳のテクスチャ集"));
      assert.equal(dragged.getAttribute("draggable"), "true");
      for (const image of dragged.querySelectorAll("img")) {
        assert.equal(image.getAttribute("draggable"), "false");
      }
      const store = new Map();
      const dataTransfer = {
        effectAllowed: "none",
        dropEffect: "none",
        types: [],
        setData(type, value) {
          store.set(type, value);
          this.types = [...store.keys()];
        },
        getData: (type) => store.get(type) ?? "",
      };
      const drag = (node, type) => {
        const event = new window.Event(type, { bubbles: true, cancelable: true });
        Object.defineProperty(event, "dataTransfer", { value: dataTransfer });
        node.dispatchEvent(event);
      };
      await act(async () => {
        drag(dragged.querySelector(".product-actions-trigger"), "dragstart");
        await delay();
      });
      assert.equal(store.size, 0);
      assert.equal(dataTransfer.effectAllowed, "none");
      await act(async () => {
        drag(dragged, "dragstart");
        await delay();
      });
      assert.equal(dataTransfer.effectAllowed, "move");
      assert.equal(dataTransfer.getData("text/plain"), "8800120");
      await wait(() => document.querySelector(".genre-rail-other"));
      const poseAgain = document.querySelector('.genre-rail-nav a[href="#/assets/pose"]');
      await act(async () => {
        drag(poseAgain, "dragover");
        await delay();
      });
      await wait(() => poseAgain.classList.contains("drag-over"));
      await act(async () => {
        drag(poseAgain, "drop");
        drag(dragged, "dragend");
        await delay();
      });
      await wait(() => button("元に戻す"));
      await route("#/assets/pose");
      await wait(() => main().textContent.includes("瞳のテクスチャ集"));
      await click(button("元に戻す"));
      await refresh();
      await route("#/assets/eye_texture");
      await wait(() => main().textContent.includes("瞳のテクスチャ集"));

      const again = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("瞳のテクスチャ集"));
      await click(again.querySelector(".product-actions-trigger"));
      await click(button("移動", again));
      await click(button("ワールド", again.querySelector(".genre-move-panel")));
      await click(button("家具・小物（ベッドなど）", again.querySelector(".genre-move-panel")));
      await wait(() => button("元に戻す"));
      const worlds = await api.productsByGenre("world", "furniture_props");
      assert.ok(worlds.items.some((item) => item.boothId === "8800120" && item.category === "world" && item.classificationPinned));
      await route("#/worlds/furniture_props");
      await wait(() => main().textContent.includes("瞳のテクスチャ集"));
      const worldCard = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("瞳のテクスチャ集"));
      await click(worldCard.querySelector(".product-actions-trigger"));
      await click(button("自動分類に戻す", worldCard));
      await wait(() => text().includes("自動分類の対象に戻しました"));
      await wait(() => {
        const current = [...main().querySelectorAll(".item")].find((node) => node.textContent.includes("瞳のテクスチャ集"));
        return current && !current.textContent.includes("手動");
      });
    });
    await t.test("avatar detail attaches an item without pinning its classification", async () => {
      const main = () => document.querySelector("main");
      await route("#/avatar/lime");
      await wait(() => main().querySelector("h1")?.textContent === "ライム");
      await wait(() => main().textContent.includes("見本の本体.unitypackage"));
      const bodyCard = [...main().querySelectorAll(".item")].find((node) =>
        node.textContent.includes("見本の本体.unitypackage"),
      );
      assert.ok(bodyCard?.querySelector(".download-row"));
      assert.equal(bodyCard.getAttribute("draggable"), "false");
      assert.ok(!bodyCard.textContent.includes("手動"));
      await click(bodyCard.querySelector(".product-actions-trigger"));
      await wait(() => button("管理対象から除外", bodyCard));
      assert.equal(button("移動", bodyCard), undefined);
      await act(async () => {
        document.body.dispatchEvent(new window.Event("pointerdown", { bubbles: true }));
        await delay();
      });
      const owned = (await api.avatar("lime")).ownedProducts.find((row) => row.boothId === "4876459");
      assert.ok(owned.downloads.some((file) => file.filename === "見本の本体.unitypackage"));
      assert.equal(document.querySelector(".avatar-add-panel"), null);
      await click(button("アイテムを追加"));
      await wait(() => button("汎用ペンライト", document.querySelector(".avatar-add-panel")));
      const panel = document.querySelector(".avatar-add-panel");
      assert.ok(!panel.textContent.includes("見本のワールド小物"));
      assert.ok(!panel.textContent.includes("オリジナル3Dモデル"));
      const input = panel.querySelector("input");
      await typeInto(input, "見本のワールド");
      await wait(() => panel.textContent.includes("追加できる商品がありません"));
      await typeInto(input, "汎用ペンライト");
      await wait(() => button("汎用ペンライト", panel));
      await click(button("汎用ペンライト", panel));
      await wait(() => text().includes("アイテムを追加しました"));
      await wait(() => {
        const section = [...main().querySelectorAll(".section")].find((node) => node.querySelector("h2")?.textContent === "ギミック");
        return section?.textContent.includes("汎用ペンライト");
      });
      const card = [...main().querySelectorAll(".section .item")].find((node) => node.textContent.includes("汎用ペンライト"));
      assert.ok(card);
      assert.ok(!card.textContent.includes("手動"));
      const detail = await api.avatar("lime");
      const item = detail.sections.flatMap((section) => section.items).find((row) => row.boothId === "8800103");
      assert.equal(item.category, "gimmick");
      assert.equal(item.classificationPinned, false);
      await assert.rejects(() => api.attachProductToAvatar("lime", "8901001"), /ワールド/);
      await assert.rejects(() => api.attachProductToAvatar("lime", "4876459"), /アバター本体/);
    });
    await t.test(
      "Apple model setup remains available with Ollama offline",
      async () => {
        const { requestRefresh } = await server.ssrLoadModule("/src/api.ts");
        api.testOllama = async () => {
          throw new Error("Ollama offline (fixture)");
        };
        await api.reopenSetup();
        await act(async () => {
          requestRefresh();
          await delay();
        });
        await wait(
          () =>
            document.querySelector(".setup-screen") &&
            button("Apple のオンデバイスモデルを使う") &&
            text().includes("Ollama offline"),
        );
        const before = pulls.length;
        await click(button("Apple のオンデバイスモデルを使う"));
        await wait(() =>
          document.querySelector(".setup-screen .import-methods"),
        );
        assert.equal((await api.settings()).llmBackend, "apple");
        assert.equal(pulls.length, before);
      },
    );
  } finally {
    if (root) await act(async () => root.unmount());
    await server.close();
    delete globalThis.IS_REACT_ACT_ENVIRONMENT;
  }
});

test("extension popup displays manifest version and sends it only on local requests", async () => {
  const { document } = parseHTML(readFileSync("extension/popup.html", "utf8"));
  const manifest = JSON.parse(readFileSync("extension/manifest.json", "utf8"));
  const requests = [];
  const context = vm.createContext({
    document,
    setTimeout,
    URL,
    Blob,
    chrome: {
      runtime: { getManifest: () => manifest },
      storage: {
        local: {
          get: async () => ({
            pairingToken: "DUMMY-CODE",
            routeDownloads: false,
          }),
          set: async () => {},
        },
      },
    },
    fetch: async (url, options) => {
      requests.push({ url, options });
      return { ok: true, status: 200, json: async () => ({ enabled: false }) };
    },
  });
  vm.runInContext(readFileSync("extension/request-policy.js", "utf8"), context);
  vm.runInContext(readFileSync("extension/popup.js", "utf8"), context);
  for (let i = 0; i < 20 && requests.length < 2; i++) await delay();
  assert.equal(
    document.querySelector("#extension-version").textContent,
    `拡張機能 v${manifest.version}`,
  );
  assert.equal(requests.length, 2);
  for (const request of requests) {
    assert.ok(request.url.startsWith("http://127.0.0.1:17321/"));
    assert.equal(
      request.options.headers["X-VRCLibMan-Extension-Version"],
      manifest.version,
    );
  }
});
