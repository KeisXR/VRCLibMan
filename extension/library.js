// サイドパネルのライブラリ。アバターとジャンルで、アプリに登録済みの商品を並べる。
// 並べ方と分け方（どの商品がどのアバター・どのジャンルに出るか）はアプリが決めて返す。
// ここが持つのは、選択・絞り込み・描画と、見ているページのアバターを先頭に出すことだけ。
(function (root, factory) {
  const api = factory(root);
  if (typeof module === "object" && module.exports) module.exports = api;
  root.VrcLibManLibrary = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function (root) {
  const PAGE = 30;
  const THUMB_PARALLEL = 4;
  const THUMB_KEEP = 600;
  const KINDS = [
    { scope: "kind:asset", label: "すべて", count: "assetCount" },
    { scope: "kind:world", label: "ワールド", count: "worldCount" },
  ];

  function fold(text) {
    return String(text || "").normalize("NFKC").toLowerCase();
  }

  // 見ているページの商品に対応するアバターを、ページが返した順で先頭に出す。ほかは元の並びのまま。
  function orderAvatars(avatars, pageIds) {
    const list = Array.isArray(avatars) ? avatars : [];
    const front = (pageIds || []).map((id) => list.find((avatar) => avatar.id === id)).filter(Boolean);
    const seen = new Set(front.map((avatar) => avatar.id));
    return [
      ...front.map((avatar) => ({ ...avatar, onPage: true })),
      ...list.filter((avatar) => !seen.has(avatar.id)).map((avatar) => ({ ...avatar, onPage: false })),
    ];
  }

  // 空白で区切った語をすべて含む商品だけを残す（名前とショップ）。空になった見出しは落とす。
  function filterSections(sections, text) {
    const terms = fold(text).split(/\s+/).filter(Boolean);
    if (terms.length === 0) return sections || [];
    return (sections || [])
      .map((section) => ({
        ...section,
        items: section.items.filter((item) => {
          const hay = fold(`${item.title} ${item.shop}`);
          return terms.every((term) => hay.includes(term));
        }),
      }))
      .filter((section) => section.items.length > 0);
  }

  // 最初に選ぶ範囲。保存した選択が今も有効ならそれ、無ければ先頭のアバター、アバターが無ければ「すべて」。
  function defaultScope(avatars, saved) {
    const list = Array.isArray(avatars) ? avatars : [];
    if (KINDS.some((kind) => kind.scope === saved)) return saved;
    if (typeof saved === "string" && saved.startsWith("avatar:") && list.some((avatar) => `avatar:${avatar.id}` === saved)) return saved;
    return list.length ? `avatar:${list[0].id}` : KINDS[0].scope;
  }

  // 保存済みのサムネイルを、アプリから認証付きで 1 件ずつ受け取る。BOOTH の画像サーバーへは通信しない。
  function createThumbs(getToken) {
    const shared = root.VrcLibManShared;
    const cache = new Map();
    const queue = [];
    let active = 0;

    async function fetchOne(id) {
      const token = getToken();
      if (!token) return null;
      const response = await fetch(`${shared.LOCAL_BASE}/library/thumb`, {
        method: "POST",
        credentials: "omit",
        signal: AbortSignal.timeout(8000),
        headers: shared.localHeaders(token, chrome.runtime.getManifest().version, true),
        body: JSON.stringify({ id }),
      });
      if (!response.ok) return null;
      const blob = await response.blob();
      return /^image\/(jpeg|png|webp|gif)$/.test(blob.type) ? URL.createObjectURL(blob) : null;
    }

    function pump() {
      while (active < THUMB_PARALLEL && queue.length) {
        const run = queue.shift();
        active += 1;
        run().finally(() => {
          active -= 1;
          pump();
        });
      }
    }

    function forget(entry) {
      entry.then((url) => url && URL.revokeObjectURL(url));
    }

    function load(id) {
      if (!cache.has(id)) {
        cache.set(id, new Promise((resolve) => {
          queue.push(() => fetchOne(id).catch(() => null).then(resolve));
          pump();
        }));
        // 古いものから手放す。表示済みの画像は消えない。
        while (cache.size > THUMB_KEEP) {
          const [oldest, entry] = cache.entries().next().value;
          cache.delete(oldest);
          forget(entry);
        }
      }
      return cache.get(id);
    }

    function clear() {
      for (const entry of cache.values()) forget(entry);
      cache.clear();
      queue.length = 0;
    }

    return { load, clear };
  }

  function create({ send, getToken, openItem, onScope, savedScope }) {
    const shared = root.VrcLibManShared;
    const ui = root.VrcLibManUi;
    const { h } = ui;
    const thumbs = createThumbs(getToken);
    const state = {
      reachable: false,
      avatars: null,
      counts: { assetCount: 0, worldCount: 0 },
      scope: "",
      sections: null,
      loading: false,
      error: "",
      filter: "",
      open: new Map(),
      shown: new Map(),
      follow: true,
      pageProduct: "",
      pageIds: [],
    };
    let generation = 0;

    const refreshButton = h("button", { class: "iconbtn", type: "button", "aria-label": "ライブラリを読み直す", title: "ライブラリを読み直す", on: { click: () => refresh() } }, ui.icon("refresh"));
    const chips = h("div", { class: "scopes", role: "group", "aria-label": "アバター" });
    const filterInput = h("input", {
      class: "field",
      type: "search",
      placeholder: "名前・ショップで絞り込む",
      "aria-label": "名前・ショップで絞り込む",
      autocomplete: "off",
      spellcheck: false,
      on: {
        input: (event) => {
          state.filter = event.target.value;
          renderList();
        },
      },
    });
    const list = h("div", { class: "sections" });
    const node = h("section", { class: "lib", "aria-label": "ライブラリ" },
      h("div", { class: "row" }, h("span", { class: "sec grow", text: "ライブラリ" }), refreshButton),
      chips,
      filterInput,
      list,
    );

    const watcher = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        watcher.unobserve(entry.target);
        showThumb(entry.target);
      }
    }, { rootMargin: "200px" });

    async function showThumb(holder) {
      const url = await thumbs.load(holder.dataset.thumb);
      if (url && holder.isConnected) holder.replaceChildren(h("img", { src: url, alt: "", loading: "lazy", decoding: "async" }));
    }

    function thumb(id, has) {
      const holder = h("span", { class: "thumb", "aria-hidden": "true" });
      if (has && id) {
        holder.dataset.thumb = id;
        watcher.observe(holder);
      }
      return holder;
    }

    function renderChips() {
      const pageIds = state.follow ? state.pageIds : [];
      const buttons = orderAvatars(state.avatars, pageIds).map((avatar) => {
        const scope = `avatar:${avatar.id}`;
        return h("button", {
          class: "scope",
          type: "button",
          "aria-pressed": state.scope === scope ? "true" : "false",
          title: avatar.displayName,
          on: { click: () => select(scope, true) },
        },
          avatar.thumbnailId ? thumb(avatar.thumbnailId, true) : null,
          h("span", { class: "scope-name", text: avatar.displayName }),
          h("span", { class: "num scope-count", text: String(avatar.itemCount) }),
          avatar.onPage ? h("span", { class: "scope-page", text: "このページ" }) : null,
        );
      });
      for (const kind of KINDS) {
        buttons.push(h("button", {
          class: "scope",
          type: "button",
          "aria-pressed": state.scope === kind.scope ? "true" : "false",
          on: { click: () => select(kind.scope, true) },
        },
          h("span", { class: "scope-name", text: kind.label }),
          h("span", { class: "num scope-count", text: String(state.counts[kind.count] || 0) }),
        ));
      }
      chips.replaceChildren(...buttons);
    }

    function row(item) {
      const marks = [
        item.isAdult ? ui.badge("R-18") : null,
        item.viaBaseBody ? ui.badge("共通素体") : null,
        item.unpublished ? ui.badge("非公開") : null,
      ].filter(Boolean);
      const url = shared.itemUrl(item.boothId);
      const open = (event) => {
        event.preventDefault();
        openItem(url, event);
      };
      return h("a", {
        class: "prod",
        href: url,
        title: item.title,
        on: {
          click: open,
          auxclick: (event) => {
            if (event.button === 1) open(event);
          },
        },
      },
        thumb(item.boothId, item.hasThumbnail),
        h("span", { class: "prod-text" },
          h("b", { text: item.title }),
          h("span", { class: "prod-sub" }, item.shop ? h("span", { class: "prod-shop", text: item.shop }) : null, marks),
        ),
      );
    }

    function fill(body, section, key) {
      const shown = state.shown.get(key) || PAGE;
      const rest = section.items.length - shown;
      const nodes = section.items.slice(0, shown).map(row);
      if (rest > 0) {
        nodes.push(h("button", {
          class: "btn secondary small more num",
          type: "button",
          text: `さらに表示（残り ${rest} 件）`,
          on: {
            click: () => {
              state.shown.set(key, shown + PAGE);
              fill(body, section, key);
            },
          },
        }));
      }
      body.replaceChildren(...nodes);
    }

    function note(text, tone) {
      return h("p", { class: tone ? `note ${tone}` : "note", role: "status", text });
    }

    function renderList() {
      const usable = state.reachable && !state.error;
      filterInput.hidden = !usable || !state.sections?.length;
      chips.hidden = !usable || !state.avatars;
      refreshButton.hidden = !state.reachable;
      if (!state.reachable) {
        list.replaceChildren(note("アプリを起動すると、登録済みの商品がここに並びます。"));
        return;
      }
      if (state.error) {
        list.replaceChildren(note(state.error, "warn"));
        return;
      }
      if (!state.sections) {
        list.replaceChildren(note("読み込んでいます…"));
        return;
      }
      if (state.sections.length === 0) {
        const empty = state.scope.startsWith("avatar:")
          ? "このアバター向けの商品は、まだ登録されていません。"
          : "まだ商品がありません。購入ライブラリを開いて「すべて読み込む」を押すと、ここに並びます。";
        list.replaceChildren(note(empty));
        return;
      }
      const filtering = fold(state.filter).trim() !== "";
      const sections = filterSections(state.sections, state.filter);
      if (sections.length === 0) {
        list.replaceChildren(note("絞り込みに合う商品がありません。"));
        return;
      }
      // アバターの一覧は開いて、件数の多い「すべて」「ワールド」は見出しだけで出す。
      const openByDefault = state.scope.startsWith("avatar:");
      list.replaceChildren(...sections.map((section) => {
        const key = `${state.scope}/${section.key}`;
        const open = filtering || (state.open.has(key) ? state.open.get(key) : openByDefault);
        const body = h("div", { class: "prods" });
        const details = h("details", { class: "genre", open },
          h("summary", null, h("span", { class: "grow", text: section.label }), h("span", { class: "num genre-count", text: String(section.items.length) })),
          body,
        );
        if (open) fill(body, section, key);
        details.addEventListener("toggle", () => {
          if (!filtering) state.open.set(key, details.open);
          if (details.open && body.childElementCount === 0) fill(body, section, key);
        });
        return details;
      }));
    }

    async function loadItems() {
      const mine = ++generation;
      const scope = state.scope;
      state.loading = true;
      const result = await send(scope.startsWith("avatar:")
        ? { type: "library-items", avatar: scope.slice("avatar:".length) }
        : { type: "library-items", kind: scope.slice("kind:".length) });
      if (mine !== generation) return;
      state.loading = false;
      if (result?.error || !Array.isArray(result?.sections)) {
        state.sections = null;
        state.error = result?.error || "ライブラリを読み込めませんでした";
      } else {
        state.sections = result.sections;
        state.error = "";
      }
      renderList();
    }

    function select(scope, byUser) {
      if (byUser) onScope(scope);
      if (scope === state.scope) return;
      state.scope = scope;
      state.sections = null;
      renderChips();
      renderList();
      if (state.reachable && state.avatars) loadItems();
    }

    // アバターの並びと、今選んでいる範囲の一覧を読み直す。
    async function refresh() {
      if (!state.reachable) return;
      const mine = ++generation;
      const result = await send({ type: "library-avatars" });
      if (mine !== generation) return;
      if (result?.error || !Array.isArray(result?.avatars)) {
        state.avatars = null;
        state.sections = null;
        state.error = result?.error || "ライブラリを読み込めませんでした";
        renderChips();
        renderList();
        return;
      }
      state.avatars = result.avatars;
      state.counts = { assetCount: Number(result.assetCount) || 0, worldCount: Number(result.worldCount) || 0 };
      state.error = "";
      state.scope = defaultScope(state.avatars, state.scope || savedScope);
      renderChips();
      await loadItems();
    }

    function setReachable(reachable) {
      if (reachable === state.reachable) return;
      state.reachable = reachable;
      if (!reachable) {
        // 古い一覧を今の状態として残さない。
        generation += 1;
        state.avatars = null;
        state.sections = null;
        state.error = "";
        thumbs.clear();
        renderChips();
        renderList();
        return;
      }
      renderList();
      refresh();
    }

    // 見ているページの商品と、その商品に対応する所持アバター。商品が変わったときだけ自動で選ぶ。
    function setPage({ productId, avatarIds }) {
      const ids = Array.isArray(avatarIds) ? avatarIds : [];
      const changed = productId !== state.pageProduct;
      const sameIds = ids.length === state.pageIds.length && ids.every((id, index) => id === state.pageIds[index]);
      state.pageProduct = productId;
      state.pageIds = ids;
      if (state.follow && changed && productId && ids.length) select(`avatar:${ids[0]}`, false);
      if (!sameIds) renderChips();
    }

    function setFollow(follow) {
      if (follow === state.follow) return;
      state.follow = follow;
      renderChips();
    }

    renderChips();
    renderList();
    return { node, refresh, setReachable, setPage, setFollow, state };
  }

  return { PAGE, KINDS, fold, orderAvatars, filterSections, defaultScope, create };
});
