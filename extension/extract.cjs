(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) {
    module.exports = api;
  }
  root.VrcLibManExtract = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const PREFERRED = /対応|素体|衣装|髪|ギミック|アバター|モデル|同梱/;

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
      if (slash >= 0) url.pathname = `/c/360x360${rest.slice(slash)}`;
    } else {
      url.pathname = `/c/360x360${url.pathname}`;
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
    const description = blocks.length > 0 ? blocks.join("\n\n") : clean(linked && linked.description);
    const og = document.querySelector('meta[property="og:image"]');
    const image = imageUrl(linked && linked.image) || (og && og.getAttribute("content")) || "";
    return makeProduct({
      boothId,
      title,
      shop: brandName(linked),
      description,
      url: absoluteUrl(href, "https://booth.pm"),
      image,
    });
  }

  function cardFrom(anchor, root, baseHref) {
    const href = absoluteUrl(anchor.getAttribute("href") || anchor.href || "", baseHref);
    const boothId = idFromUrl(href);
    if (!validId(boothId)) return null;
    const title = clean(anchor.textContent) || clean(anchor.getAttribute("title"));
    if (!title) return null;
    const shopNode = root && root.querySelector(
      ".text-text-gray600, .item-card__shop-name-anchor, .item-card__shop-name",
    );
    const imageNode = root && root.querySelector("img");
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

  function extractLibrary(document, href) {
    const products = [];
    const seen = new Set();
    function push(product) {
      if (!product || seen.has(product.boothId)) return;
      seen.add(product.boothId);
      products.push(product);
    }
    for (const image of document.querySelectorAll("img.l-library-item-thumbnail")) {
      const anchor = image.closest && image.closest("a[href*='/items/']");
      if (!anchor) continue;
      const card = (image.closest && image.closest(".bg-white, .item-card, li, article")) || anchor.parentElement;
      push(cardFrom(anchor, card, href));
    }
    let libraryPath = false;
    try {
      libraryPath = new URL(href).pathname.includes("/library");
    } catch {
      libraryPath = false;
    }
    if (libraryPath) {
      for (const card of document.querySelectorAll(".item-card")) {
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

  return { extractPage, extractItem, trimDescription, cardImage };
});
