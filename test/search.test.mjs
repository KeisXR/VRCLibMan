import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
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
  const icons = readFileSync("src/components/navIcons.tsx", "utf8");
  assert.match(icons, /"search"/, "search icon name exists");
});
