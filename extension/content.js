// BOOTH のページに入るスクリプト。商品の読み取り、アプリに登録済みの商品への印と絞り込み、
// ダウンロード欄の「LibMan で取得」を受け持つ。表示は閉じた Shadow DOM に入れ、文字は textContent で出す。
// ペアリングコードとアプリの応答はページ側に置かない（サービスワーカー経由で、必要な項目だけを受け取る）。
(function () {
  const api = globalThis.VrcLibManCompanion;
  const extract = globalThis.VrcLibManExtract;
  if (!api || !extract || !api.boothUrl(location.href)) return;

  const STYLE = `
    :host { all: initial; display: block; font: 12px/1.5 -apple-system, system-ui, "Segoe UI", "Yu Gothic UI", "Hiragino Sans", "Noto Sans JP", "Meiryo", sans-serif; color: #20222b; }
    :host([data-inline]) { display: inline-flex; vertical-align: middle; margin-left: 8px; }
    .wrap { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin: 6px 0 2px; }
    .chip { display: inline-flex; align-items: center; border: 1px solid currentColor; border-radius: 9999px; padding: 0 7px; font-size: 11px; line-height: 1.7; white-space: nowrap; color: #858995; }
    .chip.good { color: #2f6f4e; }
    .chip.warn { color: #8a6a12; }
    .badge { display: inline-flex; align-items: center; border-radius: 9999px; background: #f2f3f6; color: #5d616c; padding: 0 7px; font-size: 11px; line-height: 1.85; white-space: nowrap; }
    .btn { font: inherit; font-size: 12px; color: #20222b; background: #f7f7f9; border: 0; box-shadow: inset 0 0 0 1px #e1e3e9; border-radius: 10px; padding: 3px 10px; white-space: nowrap; cursor: pointer; }
    .btn:hover { background: #eef0f3; }
    .btn:disabled { opacity: 0.45; cursor: default; }
    .btn:focus-visible, .icon:focus-visible { outline: 2px solid #20222b; outline-offset: 2px; }
    .status { font-size: 11.5px; color: #8a6a12; margin-left: 6px; }
    .status.good { color: #2f6f4e; }
    .bar { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; background: #ffffff; border-radius: 12px; box-shadow: 0 4px 16px rgba(0, 0, 0, 0.06); padding: 6px 8px 6px 10px; margin: 12px 0; font-size: 13px; }
    .bar label { color: #5d616c; font-size: 12px; }
    .bar .grow { flex: 1; min-width: 0; }
    .bar svg.mark { width: 20px; height: 20px; flex: none; }
    select { font: inherit; color: #20222b; background: #f7f7f9; border: 0; border-radius: 10px; padding: 5px 10px; }
    select:focus-visible { outline: 2px solid #a6361f; outline-offset: 2px; }
    .icon { width: 28px; height: 28px; display: inline-grid; place-items: center; border: 0; border-radius: 9999px; background: transparent; color: #858995; cursor: pointer; padding: 0; }
    .icon:hover { background: #f4f5f8; color: #20222b; }
    .icon svg { width: 16px; height: 16px; }
  `;
  const SVG = "http://www.w3.org/2000/svg";
  const modified = (event) => !event.isTrusted || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey;

  function send(message) {
    return chrome.runtime.sendMessage(message);
  }

  function el(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }

  function host(inline) {
    const node = document.createElement("span");
    node.setAttribute("data-vrclibman", "");
    if (inline) node.setAttribute("data-inline", "");
    const root = node.attachShadow({ mode: "closed" });
    root.append(el("style", "", STYLE));
    return { node, root };
  }

  function svg(parts, className) {
    const node = document.createElementNS(SVG, "svg");
    node.setAttribute("viewBox", "0 0 24 24");
    node.setAttribute("aria-hidden", "true");
    if (className) node.setAttribute("class", className);
    for (const [tag, attrs] of parts) {
      const part = document.createElementNS(SVG, tag);
      for (const [key, value] of Object.entries(attrs)) part.setAttribute(key, String(value));
      node.append(part);
    }
    return node;
  }

  // ブランドのマーク（public/vrclibman.svg と同じ形）。
  function brandMark() {
    return svg([
      ["rect", { x: 0.75, y: 0.75, width: 22.5, height: 22.5, rx: 4.9, fill: "#20222b" }],
      ["rect", { x: 5.25, y: 4.9, width: 11.6, height: 12.75, rx: 1.5, fill: "none", stroke: "#fff", "stroke-width": 0.95 }],
      ["rect", { x: 7.1, y: 7.1, width: 11.6, height: 12.75, rx: 1.5, fill: "#fff" }],
      ["path", { d: "m9.85 11.05 3.1 4.7 3.1-4.7", fill: "none", stroke: "#20222b", "stroke-width": 1.5, "stroke-linecap": "round", "stroke-linejoin": "round" }],
    ], "mark");
  }

  function refreshIcon() {
    return svg([
      ["path", { d: "M19.5 12a7.5 7.5 0 1 1-2.2-5.3", fill: "none", stroke: "currentColor", "stroke-width": 1.8, "stroke-linecap": "round" }],
      ["path", { d: "M19.5 4.5v4h-4", fill: "none", stroke: "currentColor", "stroke-width": 1.8, "stroke-linecap": "round", "stroke-linejoin": "round" }],
    ]);
  }

  // ---- ページの読み取り ----

  function pageNotes() {
    try {
      return (extract.extractPage(document, location.href) || [])
        .filter((product) => product && product.boothId)
        .map((product) => ({
          boothId: String(product.boothId),
          isFree: product.isFree === true,
          isAdult: product.isAdult === true,
          title: String(product.title || "").slice(0, 300),
        }));
    } catch {
      return [];
    }
  }

  // ダウンロードの完了通知に題と無料の印を付けるためのメモ。サービスワーカーが保存する。
  function rememberPage() {
    const notes = pageNotes();
    if (notes.length) send({ type: "page-notes", notes }).catch(() => {});
  }

  chrome.runtime.onMessage.addListener((message, _sender, respond) => {
    if (!message || message.type !== "extract-page") return undefined;
    try {
      const products = extract.extractPage(document, location.href);
      rememberPage();
      respond({ products });
    } catch (error) {
      respond({ error: String(error) });
    }
    return true;
  });

  // ---- 状態 ----

  let pageState = { paired: false, routeDownloads: false, showBadges: true };
  let statuses = new Map();
  let connected = false;
  let selected = "";
  const files = new Map();
  const badges = new Map();
  let bar = null;

  async function loadPageState() {
    try {
      pageState = { ...pageState, ...(await send({ type: "page-state" })) };
    } catch {
      pageState = { paired: false, routeDownloads: false, showBadges: false };
    }
  }

  function downloadIdOf(anchor) {
    const value = anchor?.getAttribute("href") || anchor?.getAttribute("data-href");
    if (!value) return "";
    try {
      return api.downloadId(new URL(value, location.href).href);
    } catch {
      return "";
    }
  }

  function productIdOf(link) {
    const url = api.boothUrl(link.href);
    return (url && /\/items\/(\d{4,12})(?:\/|$)/.exec(url.pathname)?.[1]) || "";
  }

  // 商品カード。カードの中の商品リンクが 1 種類の ID だけを指すものを数える。
  function cards() {
    const rows = [];
    for (const link of document.querySelectorAll('a[href*="/items/"]')) {
      const id = productIdOf(link);
      if (!id) continue;
      const card = link.closest(".item-card, article, .bg-white");
      if (!card || card === document.body || card === document.documentElement || rows.some((row) => row.card === card)) continue;
      const ids = new Set([...card.querySelectorAll('a[href*="/items/"]')].map(productIdOf).filter(Boolean));
      if (ids.size === 1) rows.push({ id, card, after: false });
    }
    if (extract.extractItem && /\/items\/\d{4,12}/.test(location.pathname) && !location.pathname.includes("/library")) {
      const id = /\/items\/(\d{4,12})/.exec(location.pathname)?.[1];
      const heading = document.querySelector("h1, h2");
      if (id && heading && !rows.some((row) => row.card === heading)) rows.push({ id, card: heading, after: true });
    }
    return rows;
  }

  // ---- 商品の印と絞り込み ----

  function libraryPage() {
    return extract.isLibraryUrl(location.href);
  }

  function registerButton(id, status) {
    const button = el("button", "btn", "LibMan に登録");
    button.type = "button";
    button.addEventListener("click", async (event) => {
      if (!event.isTrusted) return;
      button.disabled = true;
      status.className = "status";
      status.textContent = "LibMan を開いています…";
      try {
        const result = await send({ type: "import-product", boothId: id });
        status.textContent = result?.error || "LibMan へ送りました";
        status.className = result?.error ? "status" : "status good";
        if (!result?.error) refresh();
      } catch {
        status.textContent = "拡張機能を更新した場合はページを再読み込みしてください";
      } finally {
        button.disabled = false;
      }
    });
    return button;
  }

  function paintCard(info, row) {
    const wrap = el("div", "wrap");
    if (row?.registered) {
      for (const mark of api.chips(row)) wrap.append(el("span", mark.tone ? `chip ${mark.tone}` : "chip", mark.text));
      for (const name of api.avatarNames(row)) wrap.append(el("span", "badge", name));
    } else if (libraryPage() && row) {
      const status = el("span", "status");
      wrap.append(el("span", "chip warn", "未登録"), registerButton(info.id, status), status);
    }
    info.body.replaceChildren(...(wrap.childElementCount ? [wrap] : []));
    info.host.node.hidden = wrap.childElementCount === 0;
  }

  function avatarsOnPage() {
    const names = new Map();
    for (const row of statuses.values()) for (const avatar of row.avatars || []) names.set(avatar.id, avatar.name);
    return names;
  }

  function placeBar(rows) {
    const listed = rows.filter((row) => !row.after);
    const names = avatarsOnPage();
    const wanted = pageState.showBadges && connected && listed.length >= 2 && names.size > 0;
    if (!wanted) {
      bar?.node.remove();
      bar = null;
      selected = "";
      return;
    }
    if (!bar) {
      const made = host(false);
      const box = el("div", "bar");
      const label = el("label", "", "所持アバターで絞り込む");
      const select = document.createElement("select");
      select.id = "vrclibman-avatar";
      label.htmlFor = select.id;
      const status = el("span", "grow");
      status.setAttribute("role", "status");
      const again = el("button", "icon");
      again.type = "button";
      again.setAttribute("aria-label", "状態を更新");
      again.title = "状態を更新";
      again.append(refreshIcon());
      again.addEventListener("click", () => refresh());
      select.addEventListener("change", () => {
        selected = select.value;
        applyFilter();
      });
      box.append(brandMark(), label, select, status, again);
      made.root.append(box);
      bar = { ...made, select, status };
    }
    const container = listed[0].card.parentElement;
    const anchor = container?.parentElement ? container : listed[0].card;
    if (bar.node.nextSibling !== anchor) anchor.parentElement?.insertBefore(bar.node, anchor);
    const options = [new Option("すべて表示", "")];
    for (const [id, name] of names) options.push(new Option(name, id));
    bar.select.replaceChildren(...options);
    if (!names.has(selected)) selected = "";
    bar.select.value = selected;
  }

  function applyFilter() {
    let hidden = 0;
    for (const [card, info] of badges) {
      if (!card.isConnected) {
        badges.delete(card);
        continue;
      }
      if (info.after) continue;
      // Hide only cards with confirmed compatibility; unknown products stay visible.
      const row = statuses.get(info.id);
      const filtered = Boolean(selected) && (row?.avatars?.length || 0) > 0 && !api.matches(row, selected);
      card.hidden = filtered || info.wasHidden;
      if (filtered) hidden += 1;
    }
    if (bar) bar.status.textContent = hidden ? `${hidden} 件を非表示` : "";
  }

  function clearBadges() {
    for (const [card, info] of badges) {
      card.hidden = info.wasHidden;
      info.host.node.remove();
    }
    badges.clear();
    bar?.node.remove();
    bar = null;
    selected = "";
  }

  // ---- ダウンロード欄 ----

  function paintFile(anchor, info) {
    const row = statuses.get(info.productId);
    const parts = [];
    if (connected && api.saved(row, info.id)) {
      parts.push(el("span", "chip good", "保存記録あり"));
    } else if (pageState.routeDownloads) {
      const button = el("button", "btn", "LibMan で取得");
      button.type = "button";
      button.addEventListener("click", async (event) => {
        if (!event.isTrusted) return;
        button.disabled = true;
        info.status.className = "status";
        info.status.textContent = "";
        try {
          const result = await send({ type: "start-download", downloadableId: info.id });
          info.status.textContent = result?.ok ? "取得を始めました" : result?.error || "取得を開始できません";
          info.status.className = result?.ok ? "status good" : "status";
        } catch {
          info.status.textContent = "拡張機能を更新した場合はページを再読み込みしてください";
        } finally {
          button.disabled = false;
        }
      });
      parts.push(button);
    }
    info.body.replaceChildren(...parts, info.status);
    info.host.node.hidden = parts.length === 0 && !info.status.textContent;
  }

  function decorateFiles() {
    if (!pageState.paired) return;
    let products = [];
    try {
      products = extract.extractPage(document, location.href);
    } catch {
      products = [];
    }
    for (const anchor of document.querySelectorAll("a[href], [data-href]")) {
      if (files.has(anchor)) continue;
      const id = downloadIdOf(anchor);
      if (!id) continue;
      const selection = api.choose(products, id);
      if (!selection || !api.isAsset(selection.file.filename)) continue;
      const made = host(true);
      const body = el("span", "wrap");
      const status = el("span", "status");
      status.setAttribute("role", "status");
      made.root.append(body);
      const info = { id, productId: String(selection.product.boothId), host: made, body, status };
      files.set(anchor, info);
      anchor.after(made.node);
      paintFile(anchor, info);
    }
  }

  function showFileStatus(anchor, text, good) {
    const info = files.get(anchor);
    if (!info) return;
    info.status.textContent = text;
    info.status.className = good ? "status good" : "status";
    info.host.node.hidden = false;
  }

  // ---- 状態の照会 ----

  let busy = false;
  let again = false;
  let generation = 0;

  async function refresh() {
    if (busy) {
      again = true;
      return;
    }
    busy = true;
    const gen = ++generation;
    try {
      await loadPageState();
      if (!pageState.paired) {
        connected = false;
        statuses = new Map();
        clearBadges();
        return;
      }
      decorateFiles();
      const rows = cards();
      const ids = [...new Set([...rows.map((row) => row.id), ...[...files.values()].map((info) => info.productId)])];
      const next = new Map();
      try {
        for (let index = 0; index < ids.length; index += 100) {
          const result = await send({ type: "product-status", ids: ids.slice(index, index + 100) });
          if (result?.error || !Array.isArray(result?.products)) throw new Error("unreachable");
          for (const row of result.products) next.set(row.boothId, row);
        }
        connected = true;
      } catch {
        // Never keep an old state as if it were current.
        connected = false;
      }
      if (gen !== generation) return;
      statuses = connected ? next : new Map();
      if (!connected || !pageState.showBadges) {
        clearBadges();
      } else {
        for (const { id, card, after } of rows) {
          const old = badges.get(card);
          if (old?.id === id) continue;
          if (old) {
            old.host.node.remove();
            card.hidden = old.wasHidden;
          }
          const made = host(false);
          const body = el("div");
          made.root.append(body);
          if (after) card.after(made.node);
          else card.append(made.node);
          badges.set(card, { id, after, host: made, body, wasHidden: card.hidden });
        }
        for (const [card, info] of badges) {
          if (card.isConnected) paintCard(info, statuses.get(info.id));
        }
        placeBar(rows);
        applyFilter();
      }
      for (const [anchor, info] of files) {
        if (anchor.isConnected) paintFile(anchor, info);
        else files.delete(anchor);
      }
    } finally {
      busy = false;
      if (again) {
        again = false;
        refresh();
      }
    }
  }

  // ---- 本人のクリック ----

  // Observe the original click; do not cancel, replace, or repeat the download.
  document.addEventListener("click", (event) => {
    if (modified(event)) return;
    const anchor = event.target.closest?.("a[href], [data-href]");
    const id = downloadIdOf(anchor);
    if (!id) return;
    send({ type: "remember-download", downloadableId: id }).catch(() => {});
    if (!pageState.paired) return;
    showFileStatus(anchor, "LibMan を開いています…");
    send({ type: "import-download", downloadableId: id })
      .then((result) => showFileStatus(anchor, result?.error || "LibMan へ送りました", !result?.error))
      .catch(() => showFileStatus(anchor, "拡張機能を更新した場合はページを再読み込みしてください"));
  }, true);

  let timer;
  let signature = "";
  const observer = new MutationObserver((records) => {
    if (records.every((record) => [...record.addedNodes, ...record.removedNodes].every((node) => node.nodeType === 1 && node.hasAttribute?.("data-vrclibman")))) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      const rows = cards();
      const next = rows.map((row) => row.id).join(",");
      const unseenFile = [...document.querySelectorAll('a[href*="/downloadables/"], [data-href*="/downloadables/"]')].some((anchor) => !files.has(anchor));
      if (next !== signature || unseenFile || rows.some((row) => !badges.has(row.card) && pageState.showBadges && connected)) {
        signature = next;
        refresh();
      }
    }, 400);
  });
  observer.observe(document.body, { subtree: true, childList: true });
  window.addEventListener("focus", () => refresh());

  rememberPage();
  refresh();
})();
