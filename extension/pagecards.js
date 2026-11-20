// 開いているページのカード（商品・ファイル・購入ライブラリ・購入履歴）。ポップアップとサイドパネルが同じものを出す。
// view は画面の状態（tab・kind・products・pageStale・statuses・stored・busy）、
// act は画面ごとの操作（startJob・send・say・openUrl・reloadPage・message）。
(function (root) {
  const shared = root.VrcLibManShared;
  const companion = root.VrcLibManCompanion;
  const ui = root.VrcLibManUi;
  const { h } = ui;

  // 今見えているページから、商品とバリエーションが一意に分かるファイルだけ。
  function pageFiles(products) {
    const files = [];
    for (const product of products || []) {
      for (const file of product.downloads || []) {
        if (!companion.choose(products, file.downloadableId) || !companion.isAsset(file.filename)) continue;
        files.push({ product, file });
      }
    }
    return files;
  }

  // アプリのメイン画面で、その商品（アセット・ワールドの一覧の中の商品、またはアバターの画面）を開く。
  async function openInApp(act, boothId) {
    const result = await act.send({ type: "open-in-app", id: String(boothId) });
    if (result?.ok) act.say("VRCLibMan でこの商品を開きました。ウィンドウが前に出ないときは、タスクバーの VRCLibMan を押してください。", "good");
    else act.say(result?.error || "アプリで開けませんでした", "warn");
  }

  function lastImport(view, kinds) {
    const entry = (view.stored.importHistory || []).find((row) => kinds.includes(row.kind));
    if (!entry) return null;
    const counts = entry.kind === "purchases"
      ? `新しい注文 ${entry.added ?? 0} 件`
      : `追加 ${entry.inserted ?? 0} 件`;
    return h("p", { class: "note num", text: `最後の取り込み：${shared.formatTime(entry.at)}（${counts}）` });
  }

  function itemCard(view, act, extra) {
    // 拡張のスクリプトが無いタブでは商品を読めないので、できる操作は再読み込みだけ。
    if (view.pageStale) {
      return h("section", { class: "card" },
        h("span", { class: "label", text: "開いている商品" }),
        h("span", { class: "title", text: "商品ページ" }),
        h("p", { class: "note warn", text: "このページは、拡張機能を入れる前か更新する前に開いたものです。再読み込みすると送れます。" }),
        h("button", { class: "btn primary block", type: "button", text: "ページを再読み込み", on: { click: act.reloadPage } }),
      );
    }
    const product = view.products?.[0];
    const row = product ? view.statuses.get(String(product.boothId)) : null;
    return h("section", { class: "card" },
      h("span", { class: "label", text: "開いている商品" }),
      h("span", { class: "title", text: product?.title || "商品ページ" }),
      row ? h("div", { class: "wrap" }, ui.statusMarks(row, companion)) : null,
      extra || null,
      h("button", {
        class: "btn primary block",
        type: "button",
        text: "この商品を送る",
        disabled: view.busy || !product,
        on: { click: () => act.startJob("page") },
      }),
      row?.registered
        ? h("button", { class: "btn secondary block", type: "button", text: "アプリで開く", on: { click: () => openInApp(act, product.boothId) } })
        : null,
      Array.isArray(view.products) && !product ? h("p", { class: "note", text: "このページから商品を見つけられませんでした。" }) : null,
      act.message(),
    );
  }

  function filesCard(view, act, routeHint) {
    const files = pageFiles(view.products);
    if (files.length === 0) return null;
    const routeOn = view.stored.routeDownloads === true;
    const rows = files.map(({ product, file }) => {
      const saved = companion.saved(view.statuses.get(String(product.boothId)), file.downloadableId);
      const sub = [file.variation, shared.formatBytes(file.size), saved ? "保存記録あり" : ""].filter(Boolean).join("・");
      let action = null;
      // 保存記録のあるファイルは、取得の代わりにアプリのその商品を開ける。
      if (saved) {
        action = h("button", { class: "btn secondary small", type: "button", text: "アプリで開く", on: { click: () => openInApp(act, product.boothId) } });
      } else if (routeOn) {
        action = h("button", {
          class: "btn secondary small",
          type: "button",
          text: "取得",
          on: {
            click: async (event) => {
              event.currentTarget.disabled = true;
              const result = await act.send({ type: "start-download", tabId: view.tab.id, downloadableId: file.downloadableId });
              act.say(result?.ok ? "ブラウザで取得を始めました。終わるとアプリへ知らせます。" : result?.error || "取得を開始できません", result?.ok ? "good" : "warn");
            },
          },
        });
      }
      return h("div", { class: "item" },
        h("div", { class: "file" }, h("b", { text: file.filename, title: file.filename }), sub ? h("span", { text: sub }) : null),
        action,
      );
    });
    return h("section", { class: "card" },
      h("span", { class: "sec", text: "このページのファイル" }),
      h("div", { class: "list files" }, rows),
      routeOn ? null : h("p", { class: "note", text: routeHint }),
    );
  }

  function libraryCard(view, act, extra) {
    return h("section", { class: "card" },
      h("span", { class: "label", text: "開いているページ" }),
      h("span", { class: "title", text: shared.librarySectionLabel(view.tab.url) }),
      extra || null,
      lastImport(view, ["library", "library-new"]),
      h("button", { class: "btn primary block", type: "button", text: "新しい購入を送る", disabled: view.busy, on: { click: () => act.startJob("library-new") } }),
      h("div", { class: "row" },
        h("button", { class: "btn secondary grow", type: "button", text: "すべて読み込む", disabled: view.busy, on: { click: () => act.startJob("library") } }),
        h("button", { class: "btn secondary grow", type: "button", text: "購入履歴を読み込む", disabled: view.busy, on: { click: () => act.startJob("purchases") } }),
      ),
      act.message(),
    );
  }

  function ordersCard(view, act) {
    return h("section", { class: "card" },
      h("span", { class: "label", text: "開いているページ" }),
      h("span", { class: "title", text: "購入履歴" }),
      lastImport(view, ["purchases"]),
      h("button", { class: "btn primary block", type: "button", text: "購入履歴を読み込む", disabled: view.busy, on: { click: () => act.startJob("purchases") } }),
      h("button", { class: "btn secondary block", type: "button", text: "購入ライブラリを開く", on: { click: () => act.openUrl(shared.LIBRARY_URL) } }),
      act.message(),
    );
  }

  function otherCard(view, act) {
    const booth = view.kind === "booth";
    return h("section", { class: "card" },
      h("span", { class: "title", text: booth ? "BOOTH" : "BOOTH を開くと使えます" }),
      h("p", { class: "note", text: "商品ページか購入ライブラリを開くと、VRCLibMan へ商品情報を送れます。" }),
      h("button", { class: "btn primary block", type: "button", text: "購入ライブラリを開く", on: { click: () => act.openUrl(shared.LIBRARY_URL) } }),
      act.message(),
    );
  }

  root.VrcLibManPageCards = { pageFiles, openInApp, itemCard, filesCard, libraryCard, ordersCard, otherCard };
})(globalThis);
