import assert from "node:assert/strict";
import test from "node:test";
import { parseHTML } from "linkedom";
import { createServer } from "vite";

test("browser controls preserve per-tab history, saved bookmarks, and navigation errors", async (t) => {
  const { window } = parseHTML(
    '<html><body><div id="root"></div></body></html>',
  );
  window.location = new URL("http://localhost:1420/");
  window.HTMLInputElement.prototype.select = () => {};
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
    value: { userAgent: "test" },
  });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  const { act, createElement, StrictMode } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({
    server: { middlewareMode: true, hmr: false },
    appType: "custom",
  });
  let root;
  const tick = () => new Promise((resolve) => setTimeout(resolve, 10));
  try {
    const { api } = await server.ssrLoadModule("/src/api.ts");
    const { BrowserChrome } = await server.ssrLoadModule(
      "/src/components/BrowserChrome.tsx",
    );
    const wait = async (check) => {
      for (let i = 0; i < 200; i++) {
        if (check()) return;
        await act(tick);
      }
      assert.fail(`Browser DOM did not settle: ${document.body.textContent}`);
    };
    const byLabel = (label) =>
      document.querySelector(`[aria-label="${label}"]`);
    const button = (text) =>
      [...document.querySelectorAll("button")].find(
        (node) => node.textContent === text,
      );
    const click = async (node) => {
      assert.ok(node && !node.disabled, "enabled control exists");
      await act(async () => {
        node.dispatchEvent(new window.Event("click", { bubbles: true }));
        if (node.type === "submit")
          node
            .closest("form")
            .dispatchEvent(
              new window.Event("submit", { bubbles: true, cancelable: true }),
            );
        await tick();
      });
    };
    const address = () => byLabel("アドレスバー").value;
    const navigate = async (url) => {
      await act(async () => {
        await api.browserAction({ kind: "navigate", url });
        await tick();
      });
      await wait(() => address() === new URL(url).href);
    };
    await api.openBoothBrowser();
    root = createRoot(document.getElementById("root"));
    await act(async () =>
      root.render(
        createElement(
          StrictMode,
          null,
          createElement(BrowserChrome, { embedded: true }),
        ),
      ),
    );
    await wait(() => document.querySelectorAll('[role="tab"]').length === 1);
    const first = (await api.browserState()).activeId;

    await t.test(
      "embedded controls do not expose host window buttons or drag regions",
      async () => {
        assert.equal(document.querySelector(".browser-window-controls"), null);
        assert.equal(document.querySelector("[data-tauri-drag-region]"), null);
        assert.equal(document.querySelector(".browser-import"), null);
      },
    );

    await t.test(
      "switching tabs and closing an inactive tab preserves each native-history model",
      async () => {
        await navigate("https://one.example/start");
        await navigate("https://one.example/next");
        await click(byLabel("戻る"));
        assert.equal(address(), "https://one.example/start");
        assert.ok(!byLabel("進む").disabled);
        await click(byLabel("新しいタブ"));
        await wait(
          () => document.querySelectorAll('[role="tab"]').length === 2,
        );
        const second = (await api.browserState()).activeId;
        await navigate("https://two.example/");
        await click(document.querySelectorAll('[role="tab"]')[0]);
        assert.equal(address(), "https://one.example/start");
        await click(byLabel("進む"));
        assert.equal(address(), "https://one.example/next");
        await act(async () => {
          await api.browserAction({ kind: "closeTab", tabId: second });
          await tick();
        });
        await wait(
          () => document.querySelectorAll('[role="tab"]').length === 1,
        );
        assert.equal((await api.browserState()).activeId, first);
        assert.equal(address(), "https://one.example/next");
      },
    );

    await t.test(
      "standalone titlebar includes window controls; macOS reserves native button space",
      async () => {
        await act(async () => root.render(createElement(BrowserChrome)));
        await wait(() => document.querySelector(".browser-titlebar"));
        assert.ok(document.querySelector(".browser-import").disabled);
        assert.equal(
          document
            .querySelector(".browser-titlebar")
            .getAttribute("data-tauri-drag-region"),
          "deep",
        );
        assert.equal(
          document.querySelectorAll(".browser-window-controls button").length,
          3,
        );
        assert.ok(byLabel("ウィンドウを最小化"));
        assert.ok(byLabel("ウィンドウを最大化"));
        assert.ok(byLabel("ブラウザウィンドウを閉じる"));
        assert.equal(
          document
            .querySelector(".browser-window-controls")
            .getAttribute("data-tauri-drag-region"),
          "false",
        );
        navigator.userAgent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)";
        await act(async () => root.render(createElement(BrowserChrome)));
        assert.ok(document.querySelector(".browser-titlebar.macos"));
        assert.equal(document.querySelector(".browser-window-controls"), null);
        navigator.userAgent = "test";
        await act(async () =>
          root.render(createElement(BrowserChrome, { embedded: true })),
        );
        await wait(() => !document.querySelector(".browser-titlebar"));
      },
    );

    await t.test(
      "bookmark add, edit, duplicate merge, and delete are reflected in controls",
      async () => {
        assert.equal(document.querySelector(".browser-import"), null);
        await click(byLabel("このページをブックマーク"));
        await click(button("保存"));
        await wait(() => byLabel("one.exampleを編集"));
        assert.equal(
          (await api.browserState()).bookmarks.filter(
            (b) => b.url === address(),
          ).length,
          1,
        );
        await act(async () => {
          await api.saveBrowserBookmark(
            address(),
            "Saved page",
            "https://saved.example/",
          );
          await tick();
        });
        await wait(() => button("Saved page"));
        await click(button("Saved page"));
        assert.equal(address(), "https://saved.example/");
        await act(async () => {
          await api.saveBrowserBookmark(null, "Renamed page", address());
          await tick();
        });
        await wait(() => button("Renamed page"));
        assert.equal(
          (await api.browserState()).bookmarks.filter(
            (b) => b.url === address(),
          ).length,
          1,
        );
        await click(byLabel("Renamed pageを編集"));
        await click(button("削除"));
        await wait(() => !button("Renamed page"));
        assert.ok(
          !(await api.browserState()).bookmarks.some(
            (b) => b.url === address(),
          ),
        );
      },
    );

    await t.test(
      "failed navigation displays an error and leaves the selected page intact",
      async () => {
        const previous = address();
        const original = api.browserAction;
        api.browserAction = async () => {
          throw new Error("テスト用の移動エラー");
        };
        await click(byLabel("購入ライブラリを開く"));
        await wait(() => document.querySelector('[role="alert"]'));
        assert.match(
          document.querySelector('[role="alert"]').textContent,
          /テスト用の移動エラー/,
        );
        assert.equal(address(), previous);
        api.browserAction = original;
        for (const url of [
          "javascript:alert(1)",
          "file:///tmp/page",
          "https://user:pass@example.com/",
          "http://localhost:1420/",
          "https://tauri.localhost/",
        ]) {
          await assert.rejects(api.browserAction({ kind: "navigate", url }));
        }
        assert.equal((await api.browserState()).tabs[0].url, previous);
      },
    );

    await t.test(
      "closing the last tab closes the surface and keeps saved bookmarks",
      async () => {
        const bookmarks = (await api.browserState()).bookmarks;
        await click(document.querySelector(".browser-tab-close"));
        await wait(
          () => document.querySelectorAll('[role="tab"]').length === 0,
        );
        assert.equal((await api.browserState()).open, false);
        await click(byLabel("新しいタブ"));
        await wait(
          () => document.querySelectorAll('[role="tab"]').length === 1,
        );
        assert.deepEqual((await api.browserState()).bookmarks, bookmarks);
        assert.notEqual((await api.browserState()).activeId, first);
      },
    );
  } finally {
    if (root) await act(async () => root.unmount());
    await server.close();
    delete globalThis.IS_REACT_ACT_ENVIRONMENT;
  }
});
