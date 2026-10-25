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
        if (check()) return;
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
        const drop = document.querySelector(".drop");
        assert.ok(drop.closest("details"));
        assert.ok(!drop.closest("details").hasAttribute("open"));
        let forcedCalls = 0;
        let finish;
        api.extractProducts = (force) => {
          assert.equal(force, true);
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
        assert.ok(text().includes("無料の項目を追加"));
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
        assert.ok(text().includes(`想定する拡張 v${manifest.version}`));
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
        assert.ok(text().includes("変更は自動で保存・適用"));
        await click(radio("qwen3.5:4b"));
        await wait(() => text().includes("今の設定は qwen3.5:4b"));
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:4b");
        await click(radio("qwen3.5:9b"));
        await wait(() => text().includes("今の設定は qwen3.5:9b"));
        assert.equal((await api.settings()).ollamaModel, "qwen3.5:9b");
        assert.equal(pulls.length, 1);
      },
    );

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
      assert.equal(button("一覧から除外", card()), undefined);
      await click(trigger());
      assert.equal(trigger().getAttribute("aria-expanded"), "true");
      assert.ok(button("一覧から除外", card()));
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
      assert.equal(button("一覧から除外", card()), undefined);
      await click(trigger());
      await click(button("一覧から除外", card()));
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
      return { ok: true, json: async () => ({ enabled: false }) };
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
