import assert from "node:assert/strict";
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

test("avatar candidates leave out names that do not match and mark loose ones", async () => {
  await withModules(["/src/avatarSearch.ts"], ({ avatarHit, rankAvatars }) => {
    const index = [
      avatarHit({ id: "lime", displayName: "ライム", aliases: ["Lime"] }),
      avatarHit({ id: "plum", displayName: "プラム", aliases: ["Plum"] }),
      avatarHit({ id: "manuka", displayName: "マヌカ", aliases: ["Manuka", "마누카"] }),
    ];
    assert.deepEqual(rankAvatars(index, "存在しない名前", 30), { close: [], loose: [] }, "nothing is suggested for an unknown name");
    assert.deepEqual(rankAvatars(index, "   ", 30), { close: [], loose: [] });
    const exact = rankAvatars(index, "lime", 30);
    assert.deepEqual(exact.close.map((hit) => hit.id), ["lime"], "an alias matches as a sure candidate");
    assert.deepEqual(exact.loose, []);
    const prefix = rankAvatars(index, "ライ", 30);
    assert.deepEqual(prefix.close.map((hit) => hit.id), ["lime"]);
    // Only one shared pair of characters: shown, but as an uncertain candidate.
    const loose = rankAvatars(index, "ライン", 30);
    assert.deepEqual(loose.close, []);
    assert.deepEqual(loose.loose.map((hit) => hit.id), ["lime"]);
  });
});

test("the genre field separates unchanged, automatic and fixed", async () => {
  await withModules(["/src/components/ProductEditDialog.tsx"], ({ genreChange }) => {
    const locked = { category: "hair", genre: "hair", genreLocked: true };
    const automatic = { category: "hair", genre: "hair", genreLocked: false };
    assert.deepEqual(genreChange(locked, "hair", "hair"), {}, "an untouched fixed genre is not sent");
    assert.deepEqual(genreChange(locked, "hair", ""), { resetGenre: true }, "「自動」 unlocks a fixed genre");
    assert.deepEqual(genreChange(locked, "hair", "accessory"), { genre: "accessory" });
    assert.deepEqual(genreChange(automatic, "hair", ""), {}, "an automatic genre left on 「自動」 is not sent");
    assert.deepEqual(genreChange(automatic, "hair", "hair"), { genre: "hair" }, "choosing the current value fixes it");
    // A new category with 「自動」 picks the genre again, so no leftover genre stays.
    assert.deepEqual(genreChange(automatic, "world", ""), { resetGenre: true });
    assert.deepEqual(genreChange(locked, "outfit", "hair"), { genre: "hair" }, "a kept fixed genre is fixed again under the new category");
  });
});

test("the edit dialog sends 「自動」 and says when no avatar matches", async () => {
  const { window } = parseHTML("<!doctype html><html><body><div id=\"root\"></div></body></html>");
  window.location = new URL("http://127.0.0.1:1420/#/assets");
  for (const name of ["window", "document", "location", "HTMLElement", "HTMLInputElement", "Event", "CustomEvent", "EventTarget"]) {
    Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { act, createElement } = await import("react");
    const { createRoot } = await import("react-dom/client");
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { ShellProvider } = await server.ssrLoadModule("/src/App.tsx");
    const { ProductEditDialog } = await server.ssrLoadModule("/src/components/ProductEditDialog.tsx");
    const product = (extra) => ({
      boothId: "424242",
      title: "見本のリボン",
      shop: "見本",
      url: "https://booth.pm/ja/items/424242",
      thumbnailUrl: null,
      thumbnailPath: null,
      category: "hair",
      genre: "hair",
      genreLocked: true,
      isGeneric: false,
      linkedAvatarIds: [],
      ...extra,
    });
    let current = product({});
    const updates = [];
    api.productForEdit = async () => current;
    api.knowledge = async () => ({ avatars: [{ id: "lime", displayName: "ライム", aliases: ["Lime"] }], baseBodies: [] });
    api.updateProduct = async (update) => { updates.push(update); };
    const shell = { revision: 0, refresh() {}, notify() {}, fail(message) { throw new Error(message); }, builtinBrowser: false, refreshAccount() {}, workInProgress: false, searchQuery: "", setSearchQuery() {} };
    const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
    const props = (node) => node[Object.keys(node).find((name) => name.startsWith("__reactProps$"))];
    const change = async (node, value) => {
      await act(async () => {
        props(node).onChange({ target: { value }, currentTarget: { value } });
        await tick();
      });
    };
    const click = async (node) => {
      await act(async () => {
        node.dispatchEvent(new window.Event("click", { bubbles: true }));
        await tick();
      });
    };
    const button = (name) => [...document.querySelectorAll("button")].find((node) => node.textContent === name);
    const open = async () => {
      root = createRoot(document.getElementById("root"));
      await act(async () => {
        root.render(createElement(ShellProvider, { value: shell }, createElement(ProductEditDialog, { boothId: "424242", onClose() {} })));
        await tick();
        await tick();
      });
    };

    await open();
    const genre = document.querySelector('select[aria-label="ジャンル"]');
    assert.equal(genre.value, "hair", "a fixed genre shows as its value");
    await change(genre, "");
    await click(button("保存"));
    assert.equal(updates.length, 1);
    assert.equal(updates[0].resetGenre, true, "「自動」 is sent as its own request");
    assert.equal(updates[0].genre ?? null, null);
    await act(async () => { root.render(null); });

    current = product({ genreLocked: false });
    await open();
    const auto = document.querySelector('select[aria-label="ジャンル"]');
    assert.equal(auto.value, "", "an automatic genre shows as 「自動」");
    assert.ok(auto.querySelector('option[value=""]').textContent.includes("自動（いまは髪型）"));
    const search = document.querySelector('input[aria-label="対応アバターを名前で探す"]');
    await change(search, "存在しない名前");
    assert.ok(document.body.textContent.includes("一致するアバターがありません"));
    assert.equal(document.querySelectorAll(".picker label").length, 0, "no unrelated candidate is offered");
    await change(search, "ライン");
    assert.ok(document.body.textContent.includes("名前が似ている候補"), "a loose candidate is marked as uncertain");
    assert.equal(document.querySelectorAll(".picker-loose label").length, 1);
    await click(button("保存"));
    assert.equal(updates.length, 2);
    assert.equal(updates[1].resetGenre ?? false, false, "an untouched automatic genre is not reset");
    assert.equal(updates[1].genre ?? null, null);
    await act(async () => { root.render(null); });

    // Escape は変更があるときだけ確かめ、「いいえ」なら開いたままにする。ブラウザ標準の cancel は
    // 直前の操作が無いと取り消せないので、キー入力の段階で止めていることも確かめる。
    let closed = 0;
    let answer = false;
    const asked = [];
    window.confirm = (message) => { asked.push(message); return answer; };
    const escape = async (node) => {
      const event = new window.Event("keydown", { bubbles: true, cancelable: true });
      Object.defineProperty(event, "key", { value: "Escape" });
      await act(async () => {
        node.dispatchEvent(event);
        await tick();
      });
      return event;
    };
    const openCounting = async () => {
      root = createRoot(document.getElementById("root"));
      await act(async () => {
        root.render(createElement(ShellProvider, { value: shell }, createElement(ProductEditDialog, { boothId: "424242", onClose() { closed++; } })));
        await tick();
        await tick();
      });
    };
    current = product({});
    await openCounting();
    await escape(document.querySelector('select[aria-label="ジャンル"]'));
    assert.deepEqual([closed, asked.length], [1, 0], "without changes Escape closes at once");
    await act(async () => { root.render(null); });

    await openCounting();
    const edited = document.querySelector('select[aria-label="ジャンル"]');
    await change(edited, "");
    const kept = await escape(edited);
    assert.equal(kept.defaultPrevented, true, "the dialog's own cancel does not run");
    assert.deepEqual([closed, asked.length], [1, 1], "a declined discard keeps the dialog and its changes");
    assert.equal(document.querySelector('select[aria-label="ジャンル"]').value, "");
    answer = true;
    await escape(edited);
    assert.deepEqual([closed, asked.length], [2, 2], "an accepted discard closes");
    await act(async () => { root.render(null); });
  } finally {
    await server.close();
  }
});

test("tab keys, the dialog Tab loop and conflict confirmation", async () => {
  await withModules(["/src/keyboard.ts", "/src/remoteCopy.ts", "/src/settingsNav.ts"], async ({ tabKeyTarget, trapTab }, { copyWithConfirm }, { settingsNavEdges }) => {
    assert.equal(tabKeyTarget("ArrowRight", 2, 3), 0, "wraps to the first tab");
    assert.equal(tabKeyTarget("ArrowLeft", 0, 3), 2, "wraps to the last tab");
    assert.equal(tabKeyTarget("Home", 2, 3), 0);
    assert.equal(tabKeyTarget("End", 0, 3), 2);
    assert.equal(tabKeyTarget("Enter", 0, 3), null);

    const { document: page } = parseHTML("<!doctype html><html><body><button id=outside>外</button><section id=dialog><button id=first>1</button><button id=skip tabindex=\"-1\">x</button><input id=last></section></body></html>");
    const container = page.getElementById("dialog");
    let focused = null;
    for (const node of page.querySelectorAll("button, input")) node.focus = () => { focused = node.id; };
    let prevented = 0;
    const key = (shiftKey) => ({ key: "Tab", shiftKey, preventDefault() { prevented += 1; } });
    assert.ok(trapTab(key(false), container, page.getElementById("last")), "Tab on the last item wraps");
    assert.equal(focused, "first");
    assert.ok(trapTab(key(true), container, page.getElementById("first")), "Shift+Tab on the first item wraps");
    assert.equal(focused, "last");
    assert.ok(trapTab(key(false), container, page.getElementById("outside")), "focus outside is brought back in");
    assert.equal(focused, "first");
    assert.equal(trapTab(key(false), container, page.getElementById("first")), false, "Tab inside moves normally");
    assert.equal(prevented, 3);

    const calls = [];
    const conflict = { status: "conflict", sourceSize: 5, existingSize: 6 };
    const run = async (overwrite) => {
      calls.push(overwrite);
      return overwrite ? { status: "copied", sourceSize: null, existingSize: null } : conflict;
    };
    assert.match(await copyWithConfirm("push", "a.zip", run, () => false), /コピーしませんでした/);
    assert.deepEqual(calls, [false], "declining leaves the other file alone");
    assert.match(await copyWithConfirm("push", "a.zip", run, (question) => question.includes("置き換えますか")), /リモートへコピーしました/);
    assert.deepEqual(calls, [false, false, true], "overwrite happens only after the question");

    assert.deepEqual(settingsNavEdges(false, { clientWidth: 300, scrollWidth: 800, scrollLeft: 0 }), { start: false, end: false }, "the wide column has no edges");
    assert.deepEqual(settingsNavEdges(true, { clientWidth: 300, scrollWidth: 800, scrollLeft: 0 }), { start: false, end: true });
    assert.deepEqual(settingsNavEdges(true, { clientWidth: 300, scrollWidth: 800, scrollLeft: 250 }), { start: true, end: true });
    assert.deepEqual(settingsNavEdges(true, { clientWidth: 300, scrollWidth: 800, scrollLeft: 500 }), { start: true, end: false });
    assert.deepEqual(settingsNavEdges(true, { clientWidth: 800, scrollWidth: 800, scrollLeft: 0 }), { start: false, end: false });
  });
});

test("setup steps report not done, running and done from the library itself", async () => {
  await withModules(["/src/pages/SetupWizard.tsx", "/src/components/BoothImportSettings.tsx", "/src/components/RemoteSyncReport.tsx"], ({ guideStatus, guideNextLabel, compactGuide }, { extensionReceiveText }, { remoteSyncSummary }) => {
    const stats = (extra) => ({ products: 0, ownedAvatars: 0, unextracted: 0, unlinkedFiles: 0, registeredFiles: 0, ...extra });
    const idle = { jobs: {}, seq: 0 };
    const running = (job) => ({ jobs: { [job]: { job, current: 1, total: 3, message: "分類中 1 / 3", state: "running", background: false, seq: 1, endedAt: null } }, seq: 1 });
    assert.equal(guideStatus("library", stats({}), idle, null).state, "todo");
    assert.equal(guideNextLabel(guideStatus("library", stats({}), idle, null)), "あとで行う");
    assert.equal(guideStatus("library", stats({ products: 3 }), idle, null).state, "done");
    assert.equal(guideNextLabel(guideStatus("library", stats({ products: 3 }), idle, null)), "次へ");
    assert.equal(guideStatus("classify", stats({ products: 3, unextracted: 2 }), idle, null).text, "未分類の商品が 2 件あります。");
    assert.equal(guideStatus("classify", stats({ products: 3, unextracted: 2 }), running("extract"), null).state, "running");
    assert.equal(guideNextLabel(guideStatus("classify", stats({ products: 3 }), running("extract"), null)), "次へ（処理は続きます）");
    assert.equal(guideStatus("classify", stats({ products: 3 }), idle, null).state, "done");
    assert.match(guideStatus("classify", stats({}), idle, null).text, /商品がまだない/);
    assert.equal(guideStatus("files", stats({ registeredFiles: 2 }), idle, null).state, "done");
    assert.equal(guideStatus("manual", stats({}), idle, null), null, "the optional step has no status");
    assert.equal(compactGuide(920, 640), true, "the minimum window size uses the slim guide");
    assert.equal(compactGuide(1440, 900), false);

    const link = { token: "ABCD-EFGH", port: 17321, listening: true, error: null };
    const integration = (extra) => ({ mode: "extension", paired: false, expectedExtensionVersion: "1", extensionVersion: null, extensionLastSeenAt: null, extensionImport: null, browserImport: null, ...extra });
    assert.equal(extensionReceiveText(link, integration({})).received, false, "an open receiver is not a received extension");
    assert.match(extensionReceiveText(link, integration({})).text, /まだ拡張機能から届いていません/);
    assert.equal(extensionReceiveText(link, integration({ extensionLastSeenAt: "2026-11-11T00:00:00Z" })).received, true);
    assert.equal(extensionReceiveText({ ...link, listening: false, error: "使用中" }, null).text, "使用中");

    const sync = { running: false, copied: 1, skipped: 2, missing: 1, missingFiles: ["1/a.zip"], conflicts: 1, conflictFiles: [], sourceMissing: 1, sourceMissingFiles: [], failed: 1, failures: [], lastCompletedAt: "2026-11-11T00:00:00Z", error: null };
    assert.match(remoteSyncSummary(sync), /コピー 1 件、リモートにあり 2 件、確認が必要 4 件/);
    assert.equal(remoteSyncSummary({ ...sync, error: "届きません" }), "同期を再試行します: 届きません");
  });
});

