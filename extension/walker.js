// 長い読み込み（ライブラリ全取得、新しい購入、購入履歴）の手順。offscreen 文書で動き、Node のテストからも読む。
// BOOTH へは 1 件ずつ、設定した間隔で読む。並列にしない。429 / 503 は Retry-After を尊重して 1 回だけ再試行する。
// ログインを使うのはライブラリ（購入・ギフト・無料ダウンロード）と購入履歴・注文詳細のページだけ。説明の補完は Cookie を送らない。
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  root.VrcLibManWalker = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const LOGIN_NOTICE = "BOOTH のログインが切れているか、読み込み先のページではありません。BOOTH にログインしてから、もう一度読み込んでください";
  const DETAIL_UNREADABLE = "注文の詳細を読めませんでした。BOOTH のページの形が変わった可能性があります";
  const MAX_PAGE = 200;

  function stop(message) {
    const error = new Error(message);
    error.stopping = true;
    return error;
  }

  function cancelled() {
    const error = new Error("中止しました");
    error.cancelled = true;
    return error;
  }

  function messageOf(error) {
    return error instanceof Error ? error.message : String(error);
  }

  function ordersPath(href) {
    try {
      const url = new URL(href);
      if (url.protocol !== "https:" || url.hostname !== "accounts.booth.pm" || url.username || url.password || url.port) return null;
      const path = url.pathname.replace(/\/+$/, "");
      if (path === "/orders") return { list: true };
      const id = /^\/orders\/(\d{4,12})$/.exec(path)?.[1];
      return id ? { detail: id } : null;
    } catch {
      return null;
    }
  }

  function libraryPage(href) {
    try {
      const url = new URL(href);
      return url.protocol === "https:" && url.hostname === "accounts.booth.pm" && /^\/library(\/gifts|\/free_downloads)?\/?$/.test(url.pathname);
    } catch {
      return false;
    }
  }

  /**
   * deps: { fetch, DOMParser, extract, purchases, policy, shared, intervalMs, progress(info), isCancelled(),
   *         sleep?(ms), now?(), app?(path, body) }
   */
  function createWalker(deps) {
    const { extract, purchases, policy, shared } = deps;
    const sleep = deps.sleep || ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
    const clock = deps.now || (() => Date.now());
    const pace = policy.createPacer(policy.normalizeIntervalMs(deps.intervalMs), clock, sleep);
    const progress = (info) => {
      try {
        deps.progress(info);
      } catch {
        // 進み具合を書けなくても読み込みは続ける。
      }
    };

    function checkCancel() {
      if (deps.isCancelled()) throw cancelled();
    }

    async function waitInSteps(ms) {
      const end = clock() + ms;
      while (clock() < end) {
        checkCancel();
        await sleep(Math.min(1000, end - clock()));
      }
    }

    async function boothFetch(url, options) {
      for (let attempt = 0; attempt < 2; attempt += 1) {
        checkCancel();
        await pace();
        checkCancel();
        const response = await deps.fetch(url, options);
        if (!policy.isThrottled(response.status)) return response;
        if (attempt === 1) throw stop("BOOTH のアクセス制限が続いているため、ここまでで止めました");
        const requested = policy.retryAfterMs(response.headers && response.headers.get ? response.headers.get("retry-after") : null);
        const waitMs = requested ?? policy.DEFAULT_RETRY_MS;
        if (waitMs > policy.MAX_RETRY_MS) {
          throw stop("BOOTH から長い待機時間を指定されたため、再試行せずここまでで止めました");
        }
        progress({ line: `BOOTH のアクセス制限を受けたため ${Math.ceil(waitMs / 1000)} 秒待って一度だけ再試行します` });
        await waitInSteps(waitMs);
      }
      throw stop("BOOTH から応答が得られず、ここまでで止めました");
    }

    function parse(html, href) {
      const doc = new deps.DOMParser().parseFromString(extract.quietHtml(html), "text/html");
      return doc;
    }

    // ログイン画面へ移ったら、そこで止める。fetch はリダイレクトを追うので、着いた URL を見る。
    function landedOn(response, requested) {
      return response.url || requested;
    }

    async function readLibrarySection(section, knownIds, collected) {
      let lastPage = 1;
      for (let page = 1; page <= lastPage; page += 1) {
        const url = extract.libraryPageUrl(section.url, page);
        progress({
          line: page === 1 ? `${section.label} 1 ページ目` : `${section.label} ${page} / ${lastPage} ページ`,
          current: page,
          total: page === 1 ? 0 : lastPage,
          count: collected.products.length,
        });
        const response = await boothFetch(url, { credentials: "include" });
        const landed = landedOn(response, url);
        if (!libraryPage(landed)) throw stop(LOGIN_NOTICE);
        if (!response.ok) throw stop(`${section.label}の ${page} ページ目を読めませんでした (${response.status})`);
        const doc = parse(await response.text(), landed);
        const found = extract.extractPage(doc, landed);
        collected.pages += 1;
        if (page === 1) lastPage = Math.min(MAX_PAGE, Math.max(1, extract.libraryLastPage(doc, landed)));
        if (found.length === 0) return;
        const taken = extract.takeUntilKnown(found, knownIds);
        collected.products = extract.dedupeProducts(collected.products.concat(taken.products));
        if (taken.stopped) {
          collected.stopped = true;
          return;
        }
      }
    }

    // knownIds があれば、各一覧で登録済みの商品に出会うまで読む（新しい購入だけ）。
    async function walkLibrary({ knownIds = null } = {}) {
      const collected = { products: [], pages: 0, stopped: false };
      try {
        for (const section of shared.LIBRARY_SECTIONS) {
          await readLibrarySection(section, knownIds, collected);
        }
        // 3 つの一覧を最後まで読めたときだけ、ファイル一覧の比較に使える。
        return { ...collected, complete: !knownIds };
      } catch (error) {
        return {
          ...collected,
          complete: false,
          cancelled: Boolean(error.cancelled),
          error: error.cancelled ? "" : messageOf(error),
        };
      }
    }

    function needsDetail(product) {
      return (product.descriptionSnippet || "").length < 200 || !product.shop;
    }

    // BOOTH のカテゴリとタグは、商品ページの HTML では空の欄として返り、ページのスクリプトが埋める。
    // Cookie なしで取れるのは公開の JSON だけなので、カテゴリが無い商品は JSON も読む（年齢制限の商品は読まない）。
    function needsCategory(product) {
      return product.isAdult !== true && !product.category;
    }

    // 説明が短い商品は公開ページを、カテゴリが無い商品は公開の JSON を、Cookie なしで 1 件ずつ読む。
    // 公開ページが 401 / 403 のあとは JSON に切り替えない。
    async function enrich(products) {
      const targets = products.filter((product) => needsDetail(product) || needsCategory(product));
      for (let index = 0; index < targets.length; index += 1) {
        const product = targets[index];
        progress({ line: `説明とカテゴリを補完 ${index + 1} / ${targets.length} 件`, current: index + 1, total: targets.length, count: products.length });
        let htmlStatus = 0;
        let jsonStatus = 0;
        if (needsDetail(product)) {
          try {
            const response = await boothFetch(product.url, { credentials: "omit" });
            htmlStatus = response.status;
            if (response.ok) {
              const doc = parse(await response.text(), product.url);
              const parsed = extract.extractPage(doc, product.url);
              extract.mergeDetail(product, parsed[0]);
            }
          } catch (error) {
            if (error.stopping || error.cancelled) throw error;
          }
        }
        if (!policy.canUseJsonFallback(htmlStatus)) continue;
        if (needsDetail(product) || needsCategory(product)) {
          try {
            const response = await boothFetch(`https://booth.pm/ja/items/${product.boothId}.json`, {
              credentials: "omit",
              headers: { Accept: "application/json" },
            });
            jsonStatus = response.status;
            if (response.ok) extract.mergeDetail(product, extract.detailFromItemJson(await response.json(), product.boothId));
          } catch (error) {
            if (error.stopping || error.cancelled) throw error;
          }
        }
        extract.noteIfUnpublished(product, htmlStatus, jsonStatus);
      }
    }

    async function readOrdersPage(url, expect) {
      const response = await boothFetch(url, { credentials: "include" });
      const landed = landedOn(response, url);
      const path = ordersPath(landed);
      const fits = expect.list ? path && path.list : path && path.detail === expect.detail;
      if (!fits) throw stop(LOGIN_NOTICE);
      if (!response.ok) throw stop(`購入履歴を読めませんでした (${response.status})`);
      return { landed, doc: parse(await response.text(), landed) };
    }

    // 購入履歴：一覧を新しい順に 1 ページずつ読み、アプリに保存する。一度最後まで読んだあとは、
    // 新しい注文の無いページで一覧を止める。続けて、アプリが詳細を持っていない注文を 1 件ずつ読む。
    async function walkPurchases({ complete = false } = {}) {
      const report = { pages: 0, added: 0, removed: 0, details: 0, listComplete: false, notice: "", cancelled: false };
      try {
        let lastPage = 1;
        for (let page = 1; page <= lastPage; page += 1) {
          progress({
            line: `購入履歴 ${page} / ${lastPage} ページ（新しい注文 ${report.added} 件）`,
            current: page,
            total: page === 1 ? 0 : lastPage,
            count: report.added,
          });
          const url = page === 1 ? shared.ORDERS_URL : `${shared.ORDERS_URL}?page=${page}`;
          const { landed, doc } = await readOrdersPage(url, { list: true });
          const listed = purchases.orderList(doc, landed);
          checkCancel();
          const applied = await deps.app("/purchases/list", { href: landed, orders: listed.orders });
          report.pages += 1;
          report.added += Number(applied.added) || 0;
          report.removed += Number(applied.removed) || 0;
          if (page === 1) lastPage = Math.min(MAX_PAGE, Math.max(1, Number(listed.lastPage) || 1));
          if (listed.orders.length === 0 || page >= lastPage || (complete && !applied.news)) {
            report.listComplete = true;
            break;
          }
        }
        const pending = await deps.app("/purchases/pending");
        const ids = (Array.isArray(pending.ids) ? pending.ids : []).filter((id) => /^\d{4,12}$/.test(String(id)));
        for (let index = 0; index < ids.length; index += 1) {
          const id = String(ids[index]);
          progress({ line: `注文の詳細 ${index + 1} / ${ids.length}`, current: index + 1, total: ids.length, count: report.details });
          const { landed, doc } = await readOrdersPage(`${shared.ORDERS_URL}/${id}`, { detail: id });
          const detail = purchases.orderDetail(doc, landed);
          if (!detail || detail.orderId !== id) throw stop(DETAIL_UNREADABLE);
          checkCancel();
          const applied = await deps.app("/purchases/detail", { href: landed, detail });
          report.details += 1;
          if (applied.removed) report.removed += 1;
        }
      } catch (error) {
        if (error.cancelled) report.cancelled = true;
        else report.notice = messageOf(error);
      }
      if (report.listComplete || report.pages > 0 || report.details > 0) {
        try {
          await deps.app("/purchases/finish", { listComplete: report.listComplete });
        } catch {
          // 完了印を書けなくても、読んだ注文は保存済み。次は最初から一覧を読む。
        }
      }
      return report;
    }

    // 購入完了ページを見たあと、その注文の詳細を 1 回だけ読む。
    async function readOrderDetail(orderId) {
      const id = String(orderId || "");
      if (!/^\d{4,12}$/.test(id)) return null;
      const { landed, doc } = await readOrdersPage(`${shared.ORDERS_URL}/${id}`, { detail: id });
      const detail = purchases.orderDetail(doc, landed);
      return detail && detail.orderId === id ? { href: landed, detail } : null;
    }

    return { walkLibrary, enrich, walkPurchases, readOrderDetail, needsDetail };
  }

  function summaryLine(kind, result, report) {
    if (kind === "purchases") {
      const base = `購入履歴 ${result.pages} ページ、新しい注文 ${result.added} 件、詳細 ${result.details} 件`;
      if (result.cancelled) return `中止しました。${base}を保存しました`;
      return result.notice ? `${base}。${result.notice}` : base;
    }
    return report || "";
  }

  return { createWalker, summaryLine, LOGIN_NOTICE, DETAIL_UNREADABLE, ordersPath, libraryPage };
});
