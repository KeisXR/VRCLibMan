(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) {
    module.exports = api;
  }
  root.VrcLibManExtract = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const PREFERRED = /対応|素体|衣装|髪|ギミック|アバター|モデル|同梱/;
  // 360x360 と素の 300x300 は pximg が 403 を返す。150x150 は jpeg も png も返る。
  const CARD_PXIMG_SIZE = "150x150";

  function clean(value) {
    return String(value || "").replace(/\s+/g, " ").trim();
  }

  function validId(value) {
    return /^\d{4,12}$/.test(value);
  }

  function idFromUrl(value) {
    const match = String(value || "").match(/\/items\/(\d{4,12})/);
    return match ? match[1] : "";
  }

  function absoluteUrl(value, base) {
    try {
      return new URL(value, base || "https://booth.pm").toString();
    } catch {
      return "";
    }
  }

  function readJsonLd(document) {
    const nodes = document.querySelectorAll('script[type="application/ld+json"]');
    for (const node of nodes) {
      let data;
      try {
        data = JSON.parse(node.textContent || "");
      } catch {
        continue;
      }
      const list = Array.isArray(data) ? data : [data];
      for (const item of list) {
        if (!item || typeof item !== "object") continue;
        const type = item["@type"];
        const types = Array.isArray(type) ? type : [type];
        if (types.includes("Product")) return item;
      }
    }
    return null;
  }

  function brandName(product) {
    const brand = product && product.brand;
    if (!brand) return "";
    if (typeof brand === "string") return clean(brand);
    return clean(brand.name);
  }

  function imageUrl(value) {
    if (!value) return "";
    if (typeof value === "string") return value;
    if (Array.isArray(value)) return imageUrl(value[0]);
    if (typeof value === "object" && value.url) return String(value.url);
    return "";
  }

  function cardImage(raw) {
    const value = String(raw || "").trim();
    if (!value) return null;
    let url;
    try {
      url = new URL(value);
    } catch {
      return value;
    }
    if (!url.hostname.endsWith("pximg.net")) return url.toString();
    if (url.pathname.startsWith("/c/")) {
      const rest = url.pathname.slice(3);
      const slash = rest.indexOf("/");
      if (slash >= 0) url.pathname = `/c/${CARD_PXIMG_SIZE}${rest.slice(slash)}`;
    } else {
      url.pathname = `/c/${CARD_PXIMG_SIZE}${url.pathname}`;
    }
    return url.toString();
  }

  function itemTitle(document, linked) {
    const named = document.querySelector("h1, .item-name");
    const namedText = clean(named && named.textContent);
    if (namedText) return namedText.slice(0, 300);
    const headings = document.querySelectorAll("h2");
    for (const heading of headings) {
      if (heading.closest && heading.closest("section.shop__text, .item-card")) continue;
      const text = clean(heading.textContent);
      if (text && !/^(商品説明|素体について|利用規約)$/.test(text)) return text.slice(0, 300);
    }
    return clean(linked && linked.name).slice(0, 300);
  }

  function sectionBlocks(document) {
    const sections = document.querySelectorAll("section.shop__text");
    const blocks = [];
    for (const section of sections) {
      const lines = [];
      for (const node of section.querySelectorAll("h2, h3, p")) {
        const text = clean(node.textContent);
        if (text) lines.push(text);
      }
      if (lines.length > 0) blocks.push(lines.join("\n"));
    }
    return blocks;
  }

  function detailBlocks(document) {
    const root = document.querySelector(".js-market-item-detail-description");
    if (!root) return [];
    const nodes = root.querySelectorAll("h2, h3, p");
    const source = nodes.length > 0 ? nodes : [root];
    const lines = [];
    for (const node of source) {
      const text = clean(node.textContent);
      if (text) lines.push(text);
    }
    return lines.length > 0 ? [lines.join("\n")] : [];
  }

  function mainShop(document) {
    const nodes = document.querySelectorAll(".shop-name");
    for (const node of nodes) {
      if (node.closest && node.closest(".item-card, footer, .footer__menu")) continue;
      const text = clean(node.textContent);
      if (text) return text.slice(0, 120);
    }
    return "";
  }

  function trimDescription(text) {
    const normalized = String(text || "").replace(/\r\n/g, "\n").trim();
    if (normalized.length <= 8000) return normalized;
    const chunks = normalized
      .split(/\n{2,}/)
      .map((chunk) => chunk.trim())
      .filter((chunk) => chunk && !chunk.includes("利用規約"));
    const preferred = chunks.filter((chunk) => PREFERRED.test(chunk));
    const rest = chunks.filter((chunk) => !preferred.includes(chunk));
    let output = "";
    for (const chunk of preferred.concat(rest)) {
      const next = output ? `${output}\n\n${chunk}` : chunk;
      if (next.length > 8000) break;
      output = next;
    }
    return output.slice(0, 8000);
  }

  function makeProduct(fields) {
    return {
      boothId: fields.boothId,
      title: fields.title,
      shop: fields.shop || "",
      descriptionSnippet: trimDescription(fields.description || ""),
      url: fields.url || `https://booth.pm/ja/items/${fields.boothId}`,
      thumbnailUrl: cardImage(fields.image),
    };
  }

  function extractItem(document, href) {
    const boothId = idFromUrl(href);
    if (!validId(boothId)) return null;
    const linked = readJsonLd(document);
    const title = itemTitle(document, linked);
    if (!title) return null;
    const blocks = sectionBlocks(document);
    const detail = blocks.length > 0 ? blocks : detailBlocks(document);
    const description = detail.length > 0 ? detail.join("\n\n") : clean(linked && linked.description);
    const og = document.querySelector('meta[property="og:image"]');
    const image = imageUrl(linked && linked.image) || (og && og.getAttribute("content")) || "";
    return makeProduct({
      boothId,
      title,
      shop: brandName(linked) || mainShop(document),
      description,
      url: absoluteUrl(href, "https://booth.pm"),
      image,
    });
  }

  function detailFromItemJson(data, boothId) {
    if (!data || typeof data !== "object") return null;
    const id = String(data.id || boothId || "");
    if (!validId(id)) return null;
    const shop = data.shop && typeof data.shop === "object" ? data.shop.name : "";
    return makeProduct({
      boothId: id,
      title: clean(data.name) || `item ${id}`,
      shop: clean(shop),
      description: typeof data.description === "string" ? data.description : "",
      url: clean(data.url) || `https://booth.pm/ja/items/${id}`,
      image: "",
    });
  }

  function mergeDetail(product, detail) {
    if (!product || !detail) return product;
    if ((detail.descriptionSnippet || "").length > (product.descriptionSnippet || "").length) {
      product.descriptionSnippet = detail.descriptionSnippet;
    }
    if (!product.shop && detail.shop) product.shop = detail.shop;
    if (!product.thumbnailUrl && detail.thumbnailUrl) product.thumbnailUrl = detail.thumbnailUrl;
    return product;
  }

  function cardRoot(image) {
    let best = image.parentElement;
    let node = image.parentElement;
    const body = image.ownerDocument && image.ownerDocument.body;
    while (node && node !== body) {
      const thumbs = node.querySelectorAll("img.l-library-item-thumbnail").length;
      if (thumbs > 1) break;
      best = node;
      node = node.parentElement;
    }
    return best;
  }

  function libraryTitle(scope, boothId, baseHref) {
    const links = scope.querySelectorAll ? scope.querySelectorAll("a[href*='/items/']") : [];
    for (const link of links) {
      const href = absoluteUrl(link.getAttribute("href") || link.href || "", baseHref);
      const text = clean(link.textContent);
      if (text && idFromUrl(href) === boothId) return text;
    }
    const titled = scope.querySelector && scope.querySelector(".text-text-default");
    return clean(titled && titled.textContent);
  }

  function cardFrom(anchor, root, baseHref) {
    const href = absoluteUrl(anchor.getAttribute("href") || anchor.href || "", baseHref);
    const boothId = idFromUrl(href);
    if (!validId(boothId)) return null;
    const scope = root || anchor;
    const title = libraryTitle(scope, boothId, baseHref)
      || clean(anchor.getAttribute("title"))
      || clean(anchor.textContent);
    if (!title) return null;
    const shopNode = scope.querySelector && scope.querySelector(
      ".text-text-gray600, .item-card__shop-name-anchor, .item-card__shop-name",
    );
    const imageNode = scope.querySelector && scope.querySelector("img");
    const image = imageNode ? imageNode.getAttribute("src") || imageNode.src || "" : "";
    return makeProduct({
      boothId,
      title: title.slice(0, 300),
      shop: clean(shopNode && shopNode.textContent).slice(0, 120),
      description: "",
      url: href,
      image,
    });
  }

  function libraryProduct(image, baseHref) {
    const card = cardRoot(image) || image.parentElement;
    const imageAnchor = image.closest && image.closest("a[href*='/items/']");
    const anchor = imageAnchor || (card && card.querySelector("a[href*='/items/']"));
    if (!anchor) return null;
    const href = absoluteUrl(anchor.getAttribute("href") || anchor.href || "", baseHref);
    const boothId = idFromUrl(href);
    if (!validId(boothId)) return null;
    const title = libraryTitle(card, boothId, baseHref) || clean(image.getAttribute("alt"));
    if (!title) return null;
    const shopNode = card.querySelector(
      ".text-text-gray600, .item-card__shop-name-anchor, .item-card__shop-name",
    );
    return makeProduct({
      boothId,
      title: title.slice(0, 300),
      shop: clean(shopNode && shopNode.textContent).slice(0, 120),
      description: "",
      url: href,
      image: image.getAttribute("src") || image.src || "",
    });
  }

  function extractLibrary(document, href) {
    const products = [];
    const seen = new Set();
    function push(product) {
      if (!product || seen.has(product.boothId)) return;
      seen.add(product.boothId);
      products.push(product);
    }
    for (const image of document.querySelectorAll("img.l-library-item-thumbnail")) {
      push(libraryProduct(image, href));
    }
    let libraryPath = false;
    try {
      libraryPath = new URL(href).pathname.includes("/library");
    } catch {
      libraryPath = false;
    }
    if (libraryPath && products.length === 0) {
      for (const card of document.querySelectorAll(".bg-white, .item-card, li, article")) {
        const image = card.querySelector("img");
        if (image) {
          push(libraryProduct(image, href));
          continue;
        }
        const anchor = card.querySelector("a[href*='/items/']");
        if (anchor) push(cardFrom(anchor, card, href));
      }
    }
    return products;
  }

  function isItemPage(href) {
    try {
      const url = new URL(href);
      return /\/items\/\d+/.test(url.pathname) && !url.pathname.includes("/library");
    } catch {
      return false;
    }
  }

  function extractPage(document, href) {
    if (isItemPage(href)) {
      const item = extractItem(document, href);
      return item ? [item] : [];
    }
    let libraryPath = false;
    try {
      libraryPath = new URL(href).pathname.includes("/library");
    } catch {
      libraryPath = false;
    }
    const hasLibraryThumb = Boolean(document.querySelector("img.l-library-item-thumbnail"));
    if (libraryPath || hasLibraryThumb) return extractLibrary(document, href);
    return [];
  }

  function quietHtml(html) {
    return String(html || "")
      .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, "")
      .replace(/<link\b[^>]*\brel=(["']?)preload\1[^>]*>/gi, "");
  }

  function isLibraryUrl(href) {
    try {
      const url = new URL(href);
      return /(^|\.)booth\.pm$/.test(url.hostname) && url.pathname.includes("/library");
    } catch {
      return false;
    }
  }

  function libraryPageUrl(href, page) {
    const url = new URL(href);
    url.searchParams.set("page", String(page));
    url.hash = "";
    return url.toString();
  }

  function libraryLastPage(document, baseHref) {
    let max = 1;
    const links = document.querySelectorAll("a[href*='page=']");
    for (const link of links) {
      try {
        const url = new URL(link.getAttribute("href") || "", baseHref || "https://accounts.booth.pm/library");
        const page = Number(url.searchParams.get("page"));
        if (Number.isInteger(page) && page > max && page <= 200) max = page;
      } catch {
        // 壊れたリンクはページ数に数えない。
      }
    }
    return max;
  }

  const UNAVAILABLE_TEXT = "公開終了（取得できず）";

  function noteIfUnpublished(product, htmlStatus, jsonStatus) {
    if (!product) return product;
    const hasDescription = (product.descriptionSnippet || "").trim().length > 0;
    if (!hasDescription && htmlStatus === 404 && jsonStatus === 404) {
      product.descriptionSnippet = UNAVAILABLE_TEXT;
      product.unavailable = "not-found";
    }
    return product;
  }

  function takeUntilKnown(products, knownIds) {
    if (!knownIds) return { products: products || [], stopped: false };
    const fresh = [];
    for (const product of products || []) {
      const id = product && product.boothId ? String(product.boothId) : "";
      if (id && knownIds.has(id)) return { products: fresh, stopped: true };
      fresh.push(product);
    }
    return { products: fresh, stopped: false };
  }

  function dedupeProducts(products) {
    const seen = new Set();
    const unique = [];
    for (const product of products || []) {
      if (!product || !product.boothId || seen.has(product.boothId)) continue;
      seen.add(product.boothId);
      unique.push(product);
    }
    return unique;
  }

  return {
    extractPage,
    extractItem,
    trimDescription,
    cardImage,
    detailFromItemJson,
    mergeDetail,
    quietHtml,
    isLibraryUrl,
    libraryPageUrl,
    libraryLastPage,
    dedupeProducts,
    takeUntilKnown,
    noteIfUnpublished,
    UNAVAILABLE_TEXT,
  };
});
