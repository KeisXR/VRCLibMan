import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

async function withModule(path, run) {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    await run(await server.ssrLoadModule(path));
  } finally {
    await server.close();
  }
}

const avatar = (extra = {}) => ({
  id: "urara",
  displayName: "うらら",
  baseBody: null,
  baseBodyName: null,
  aliases: ["Urara"],
  isBuiltin: false,
  isPlaceholder: false,
  owned: true,
  thumbnailPath: null,
  thumbnailUrl: null,
  outfitCount: 1,
  hairCount: 0,
  gimmickCount: 0,
  genericCount: 0,
  ...extra,
});

const product = (extra = {}) => ({
  boothId: "1234567",
  title: "Oneric Eye texture",
  shop: "Oneric",
  url: "https://booth.pm/ja/items/1234567",
  descriptionSnippet: "うらら対応のアイテクスチャです",
  thumbnailUrl: null,
  thumbnailPath: null,
  category: "outfit",
  genre: null,
  classificationPinned: false,
  isGeneric: false,
  confidence: null,
  viaBaseBody: false,
  localFiles: [],
  localFileCount: 0,
  variations: [],
  downloads: [],
  isFree: false,
  isAdult: false,
  ...extra,
});

test("search tokens split on whitespace and fold case", async () => {
  await withModule("/src/searchUtils.ts", (s) => {
    assert.deepEqual(s.searchTokens("  Eye   TEXTURE "), ["eye", "texture"]);
    assert.deepEqual(s.searchTokens(""), []);
    assert.deepEqual(s.searchTokens("   "), []);
  });
});

test("avatar search matches name, base body and aliases with AND tokens", async () => {
  await withModule("/src/searchUtils.ts", (s) => {
    const cards = [avatar(), avatar({ id: "lime", displayName: "ライム", aliases: ["Lime"], baseBodyName: "素体A" })];
    assert.equal(s.filterAvatarsByQuery(cards, "うらら").length, 1);
    assert.equal(s.filterAvatarsByQuery(cards, "urara").length, 1, "alias match, case-insensitive");
    assert.equal(s.filterAvatarsByQuery(cards, "素体a").length, 1, "base body name match");
    assert.equal(s.filterAvatarsByQuery(cards, "ライム lime").length, 1, "every token must match");
    assert.equal(s.filterAvatarsByQuery(cards, "ライム urara").length, 0, "tokens across items do not combine");
    assert.equal(s.filterAvatarsByQuery(cards, "").length, 0, "empty query matches nothing");
  });
});

test("product search matches title, shop, description, genre label and booth id", async () => {
  await withModule("/src/searchUtils.ts", (s) => {
    const items = [product(), product({ boothId: "7654321", title: "星空ワールド", shop: "Hoshi", descriptionSnippet: "寝転べる場所", genre: "world" })];
    assert.equal(s.filterProductsByQuery(items, "oneric").length, 1, "shop match");
    assert.equal(s.filterProductsByQuery(items, "うらら").length, 1, "description match");
    assert.equal(s.filterProductsByQuery(items, "eye oneric").length, 1, "tokens may hit different fields");
    assert.equal(s.filterProductsByQuery(items, "7654321").length, 1, "booth id match");
    assert.equal(s.filterProductsByQuery(items, "ワールド").length >= 1, true);
    assert.equal(s.filterProductsByQuery(items, "").length, 0);
    assert.equal(s.filterProductsByQuery(items, "存在しない").length, 0);
  });
});

test("search page groups avatar, asset and world hits from the shell query", async () => {
  const { window } = parseHTML("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  window.location = new URL("http://127.0.0.1:1420/#/search");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test", clipboard: { writeText: async () => {} } } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { act, createElement } = await import("react");
    const { createRoot } = await import("react-dom/client");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { SearchPage } = await server.ssrLoadModule("/src/pages/SearchPage.tsx");
    api.avatars = async () => [avatar()];
    api.productsByGenre = async (kind) =>
      kind === "asset"
        ? { items: [product()], counts: {}, total: 1 }
        : { items: [product({ boothId: "7654321", title: "星空ワールド", shop: "Hoshi", descriptionSnippet: "寝転べる場所" })], counts: {}, total: 1 };
    const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
    const render = async (searchQuery) => {
      const shell = {
        revision: 0,
        refresh() {},
        notify() {},
        fail(message) { throw new Error(message); },
        builtinBrowser: false,
        refreshAccount() {},
        workInProgress: false,
        searchQuery,
        setSearchQuery() {},
      };
      await act(async () => {
        root.render(createElement(ShellProvider, { value: shell }, createElement(SearchPage)));
        await tick();
      });
    };
    root = createRoot(document.getElementById("root"));
    await render("うらら");
    const text = document.body.textContent;
    assert.ok(text.includes("アバター（1）"), "avatar group with count");
    assert.ok(text.includes("アセット（1）"), "asset group hits via description");
    assert.ok(!text.includes("ワールド（"), "world group hidden without hits");
    assert.ok(text.includes("2 件見つかりました"), "total count line");
    await render("");
    assert.ok(document.body.textContent.includes("アバター 1 件、アセット 1 件、ワールド 1 件から探せます"), "empty query shows library counts");
    await render("存在しないもの");
    assert.ok(document.body.textContent.includes("一致するものがありません"), "no-hit message");
    await act(async () => { root.render(null); });
  } finally {
    await server.close();
  }
});

test("the shell wires the search page, nav item and sidebar box", () => {
  const app = readFileSync("src/App.tsx", "utf8");
  assert.match(app, /href: "#\/search", label: "検索"/, "nav item");
  assert.match(app, /route\.page === "search" && <SearchPage \/>/, "route renders the search page");
  assert.match(app, /className="sidebar-search"/, "sidebar search box");
  assert.match(app, /onCompositionStart/, "sidebar box tracks IME composition");
  assert.match(app, /onCompositionEnd/, "sidebar box navigates after IME composition ends");
  const icons = readFileSync("src/components/navIcons.tsx", "utf8");
  assert.match(icons, /"search"/, "search icon name exists");
});

test("search page separates loading, failure and success, retries, and adds whole-description matches", async () => {
  const { window } = parseHTML("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  window.location = new URL("http://127.0.0.1:1420/#/search");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test", clipboard: { writeText: async () => {} } } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { act, createElement } = await import("react");
    const { createRoot } = await import("react-dom/client");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { SearchPage } = await server.ssrLoadModule("/src/pages/SearchPage.tsx");
    let failWorlds = true;
    let failAll = false;
    api.avatars = async () => {
      if (failAll) throw new Error("アバターを読めません");
      return [avatar()];
    };
    api.productsByGenre = async (kind) => {
      if (failAll) throw new Error("商品を読めません");
      if (kind === "world" && failWorlds) throw new Error("ワールドを読めません");
      return kind === "asset"
        ? { items: [product(), product({ boothId: "2222222", title: "長い説明の服", shop: "Long", descriptionSnippet: "先頭だけ" })], counts: {}, total: 2 }
        : { items: [product({ boothId: "7654321", title: "星空ワールド", shop: "Hoshi", descriptionSnippet: "寝転べる場所" })], counts: {}, total: 1 };
    };
    // Rust reads the whole description; the card only has its first 500 characters.
    api.searchProductIds = async (query) => (query === "後半にだけある語" ? ["2222222"] : []);
    const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    let shellQuery = "うらら";
    const shell = () => ({
      revision: 0,
      refresh() {},
      notify() {},
      fail(message) { throw new Error(message); },
      builtinBrowser: false,
      refreshAccount() {},
      workInProgress: false,
      searchQuery: shellQuery,
      setSearchQuery() {},
    });
    const render = async (query, settle = 20) => {
      shellQuery = query;
      await act(async () => {
        root.render(createElement(ShellProvider, { value: shell() }, createElement(SearchPage)));
        await wait(20);
      });
      // Effects run when the first act ends; the debounced full-text search needs its own wait.
      await act(async () => { await wait(settle); });
    };
    const retry = () => [...document.querySelectorAll("button")].find((node) => node.textContent === "もう一度読み込む");
    root = createRoot(document.getElementById("root"));
    await render("うらら");
    let text = document.body.textContent;
    assert.ok(text.includes("ワールドを読み込めませんでした。読み込めた種類は検索できます"), "the failed kind is named");
    assert.ok(text.includes("アセット（1）"), "loaded kinds stay searchable");
    assert.ok(!text.includes("読み込み中"), "a failure is not shown as loading");
    assert.ok(retry(), "retry is offered");

    await render("後半にだけある語", 250);
    assert.ok(document.body.textContent.includes("長い説明の服"), "a match after the card snippet is found");

    failWorlds = false;
    await act(async () => {
      retry().dispatchEvent(new window.Event("click", { bubbles: true }));
      await wait(20);
    });
    await render("");
    text = document.body.textContent;
    assert.ok(!text.includes("読み込めませんでした"), "the error clears after a successful retry");
    assert.ok(text.includes("アバター 1 件、アセット 2 件、ワールド 1 件から探せます"));
    await act(async () => { root.render(null); });

    failAll = true;
    root = createRoot(document.getElementById("root"));
    await render("うらら");
    text = document.body.textContent;
    assert.ok(text.includes("ライブラリを読み込めませんでした"), "everything failing says so");
    assert.ok(!text.includes("読み込み中"));
    assert.ok(retry());
    await act(async () => { root.render(null); });
  } finally {
    await server.close();
  }
});

test("app search finds sidebar items, page items, genres and settings with AND tokens", async () => {
  await withModule("/src/appSearchIndex.ts", (s) => {
    const titles = (query) => s.filterAppEntries(s.APP_SEARCH_ENTRIES, query).map((entry) => entry.title);
    assert.ok(titles("強制ダーク").includes("強制ダークモード"), "a setting row by its label");
    assert.ok(titles("だーく").includes("強制ダークモード"), "hiragana finds katakana");
    assert.ok(titles("ｕｎｉｔｙ").includes("既定の Unity"), "full-width letters are folded");
    assert.ok(titles("nextcloud").includes("リモートへ自動同期"), "keywords are searched");
    assert.ok(titles("購入履歴").includes("購入履歴"), "a sidebar item");
    assert.ok(titles("衣装").includes("衣装"), "an asset genre");
    assert.ok(titles("skybox").includes("Skybox"), "a world genre");
    assert.ok(titles("ペアリング").includes("ペアリングコード"));
    assert.deepEqual(titles("インポート 再分類"), ["すべて再分類", "2. 分類"], "tokens may hit the path; title hits come first");
    assert.deepEqual(titles("設定").slice(0, 3), ["設定", "アセットの更新設定", "ジャンル未設定"], "an exact title first, then titles with the word, genres last");
    assert.equal(titles("その他")[0], "その他", "an exact genre name still comes first");
    assert.deepEqual(titles(""), []);
    assert.deepEqual(titles("   "), []);
    assert.deepEqual(titles("存在しない項目"), []);
    assert.ok(!titles("").includes("検索"), "the search page itself is not listed");
  });
});

test("app search entries point at ids that exist and cover every settings category", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const s = await server.ssrLoadModule("/src/appSearchIndex.ts");
    const { SETTINGS_NAV_ITEMS } = await server.ssrLoadModule("/src/settingsNav.ts");
    const keys = s.APP_SEARCH_ENTRIES.map((entry) => entry.key);
    assert.equal(new Set(keys).size, keys.length, "keys are unique");
    const walk = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((item) =>
      item.isDirectory() ? walk(`${dir}/${item.name}`) : item.name.endsWith(".tsx") ? [`${dir}/${item.name}`] : []);
    const source = walk("src").filter((file) => !file.startsWith("src/preview")).map((file) => readFileSync(file, "utf8")).join("\n");
    for (const entry of s.APP_SEARCH_ENTRIES) {
      for (const id of [entry.target, entry.fallback].filter(Boolean)) {
        assert.match(source, new RegExp(`(?:\\bid|pairingId)="${id}"`), `${entry.key}: #${id} exists in a screen`);
      }
      assert.match(entry.href, /^#\//, `${entry.key}: href is an app route`);
      assert.ok(entry.path.length > 0, `${entry.key}: has a location`);
    }
    const targets = new Set(s.APP_SEARCH_ENTRIES.filter((entry) => entry.href === "#/settings").map((entry) => entry.target));
    for (const item of SETTINGS_NAV_ITEMS) assert.ok(targets.has(item.id), `settings category ${item.id} is searchable`);
  } finally {
    await server.close();
  }
});

test("entries that the current mode hides carry a note and lead to their parent", async () => {
  await withModule("/src/appSearchIndex.ts", (s) => {
    const entry = (title) => s.APP_SEARCH_ENTRIES.find((item) => item.title === title);
    const dark = entry("強制ダークモード");
    const off = { builtinBrowser: false, extension: true };
    const on = { builtinBrowser: true, extension: false };
    assert.equal(s.appEntryNote(dark, off), "内蔵ブラウザを有効にすると表示されます");
    assert.deepEqual(s.appEntryTargets(dark, off), { target: "settings-booth" }, "goes to the section instead");
    assert.equal(s.appEntryNote(dark, on), null);
    assert.deepEqual(s.appEntryTargets(dark, on), { target: "settings-force-dark", fallback: "settings-booth" });
    const pairing = entry("ペアリングコード");
    assert.equal(s.appEntryNote(pairing, on), "読み込み方で拡張機能を選ぶと表示されます");
    assert.equal(s.appEntryNote(pairing, { builtinBrowser: true, extension: null }), null, "an unknown mode adds no note");
    assert.equal(s.appEntryNote(entry("ライブラリのフォルダ"), off), null);
  });
});

test("jumping to an app entry opens closed details, scrolls and marks the row for a moment", async () => {
  const { window } = parseHTML(`<!doctype html><html><body>
    <details class="settings-section" id="settings-debug"><summary>デバッグ</summary>
      <div class="setting-row" id="settings-maintenance"><div class="setting-copy"><strong>メンテナンス</strong></div><div class="setting-control"><button type="button">初期データを追加</button></div></div>
    </details>
    <section class="settings-section" id="settings-booth"><header><h2>BOOTH</h2></header></section>
  </body></html>`);
  window.location = new URL("http://127.0.0.1:1420/#/search");
  for (const name of ["window", "document", "location", "HTMLElement"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  window.requestAnimationFrame = undefined;
  const scrolled = [];
  window.HTMLElement.prototype.scrollIntoView = function (options) { scrolled.push([this.id, options.block]); };
  await withModule("/src/appSearchJump.ts", async (jump) => {
    const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    jump.jumpToAppLocation("#/settings", "settings-maintenance", "settings-debug");
    await wait(40);
    const row = document.getElementById("settings-maintenance");
    assert.ok(document.getElementById("settings-debug").hasAttribute("open"), "the closed debug section is opened");
    assert.ok(row.classList.contains(jump.APP_JUMP_CLASS), "the row is marked");
    // Scrolling waits until the page height has been quiet for a moment (sections load late).
    await wait(300);
    assert.equal(window.location.hash, "#/settings");
    assert.deepEqual(scrolled.at(-1), ["settings-maintenance", "center"]);
    assert.equal(jump.jumpBlock(document.getElementById("settings-booth")), "start", "sections scroll to their top");
    jump.jumpToAppLocation("#/settings", "settings-booth");
    await wait(300);
    assert.deepEqual(scrolled.at(-1), ["settings-booth", "start"]);
  });
});

test("the search page lists screens and settings first, notes hidden ones and expands the rest", async () => {
  const { window } = parseHTML("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  window.location = new URL("http://127.0.0.1:1420/#/search");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test", clipboard: { writeText: async () => {} } } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { act, createElement } = await import("react");
    const { createRoot } = await import("react-dom/client");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { SearchPage, APP_RESULTS_SHOWN } = await server.ssrLoadModule("/src/pages/SearchPage.tsx");
    api.avatars = async () => [avatar()];
    api.productsByGenre = async () => ({ items: [product({ title: "ダークな衣装" })], counts: {}, total: 1 });
    api.searchProductIds = async () => [];
    api.integrationStatus = async () => ({ mode: "extension" });
    const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
    const render = async (searchQuery) => {
      const shell = {
        revision: 0,
        refresh() {},
        notify() {},
        fail(message) { throw new Error(message); },
        builtinBrowser: false,
        refreshAccount() {},
        workInProgress: false,
        searchQuery,
        setSearchQuery() {},
      };
      await act(async () => {
        root.render(createElement(ShellProvider, { value: shell }, createElement(SearchPage)));
        await wait(20);
      });
      await act(async () => { await wait(20); });
    };
    root = createRoot(document.getElementById("root"));
    await render("ダーク");
    let text = document.body.textContent;
    const groups = [...document.querySelectorAll(".search-group > h2")].map((node) => node.textContent);
    assert.match(groups[0], /^画面と設定（\d+）$/, "screens and settings come first");
    assert.ok(groups.some((title) => title.startsWith("アセット")), "library hits still follow");
    assert.ok(text.includes("強制ダークモード"));
    assert.ok(text.includes("設定 › BOOTH の読み込み方"), "the location is shown");
    assert.ok(text.includes("内蔵ブラウザを有効にすると表示されます"), "a setting hidden in this mode is noted");
    const link = [...document.querySelectorAll(".app-search-link")].find((node) => node.textContent.includes("強制ダークモード"));
    assert.equal(link.getAttribute("href"), "#/settings");

    await render("設定");
    const shown = () => document.querySelectorAll(".app-search-link").length;
    assert.equal(shown(), APP_RESULTS_SHOWN, "a long list starts short");
    const more = [...document.querySelectorAll("button")].find((node) => node.textContent.startsWith("さらに表示"));
    assert.ok(more, "the rest can be shown");
    await act(async () => {
      more.dispatchEvent(new window.Event("click", { bubbles: true }));
      await wait(10);
    });
    assert.ok(shown() > APP_RESULTS_SHOWN, "all screens and settings are listed after expanding");
    text = document.body.textContent;
    assert.ok(!text.includes("さらに表示"));
    await act(async () => { root.render(null); });
  } finally {
    await server.close();
  }
});
