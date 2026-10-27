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

  function limitedName(value, max) {
    const text = clean(value);
    if (!text || text.length > max) return "";
    if (/[\\/\u0000]/.test(text)) return "";
    return text;
  }

  function safeFileName(value) {
    const text = clean(value);
    if (!text || text.length > 180 || text === "." || text === "..") return "";
    if (/[\\/:*?"<>|\u0000]/.test(text)) return "";
    return text;
  }

  const DOWNLOAD_LABEL = /^(ダウンロード|無料ダウンロード|download|その他のdl方法|その他のダウンロード|ブラウザでダウンロード|ダウンロード商品)$/i;
  const FILE_EXT = /\.[A-Za-z0-9]{1,16}$/;
  const EXT_ONLY = /^\.[A-Za-z0-9]{1,16}$/;
  const VARIATION_NOISE = /^(ダウンロード商品|無料|有料|デジタル|ファイル|file|download)$/i;
  const NAME_BREAK = /[\s　/\\:*?"<>|\u0000()（）[\]【】]/;

  function downloadableIdFrom(value) {
    const match = String(value || "").match(/\/downloadables\/(\d{1,12})(?:[/?#]|$)/i);
    return match ? match[1] : "";
  }

  function hrefShape(value) {
    let text = String(value || "").trim();
    if (!text) return "";
    try {
      const url = new URL(text, "https://booth.pm");
      const keys = [...url.searchParams.keys()].sort();
      const query = keys.length
        ? `?${keys.map((key) => `${key.replace(/[0-9]+/g, "#")}=#`).join("&")}`
        : "";
      text = `${url.origin}${url.pathname}${query}`;
    } catch {
      text = text.split("#")[0];
    }
    return text.replace(/[0-9]+/g, "#").slice(0, 200);
  }

  function fileNameFromText(value) {
    const text = clean(value);
    if (!text || text.length > 180 || DOWNLOAD_LABEL.test(text) || EXT_ONLY.test(text)) return "";
    if (/\s\.[A-Za-z0-9]{1,16}$/.test(text)) return "";
    if (!FILE_EXT.test(text)) return "";
    return safeFileName(text);
  }

  function filenameTokens(value) {
    const text = String(value || "");
    const found = [];
    const pattern = /\.[A-Za-z0-9]{1,16}(?=$|[\s　)）\]])/g;
    let match;
    while ((match = pattern.exec(text)) !== null) {
      let start = match.index;
      while (start > 0 && !NAME_BREAK.test(text[start - 1])) start -= 1;
      while (start > 1 && text[start - 1] === " ") {
        let probe = start - 2;
        while (probe >= 0 && text[probe] !== " " && !NAME_BREAK.test(text[probe])) probe -= 1;
        if (probe === start - 2) break;
        start = probe + 1;
      }
      const named = fileNameFromText(text.slice(start, match.index + match[0].length));
      if (named && !found.includes(named)) found.push(named);
      if (match.index === pattern.lastIndex) pattern.lastIndex += 1;
    }
    return found;
  }

  function longestName(names) {
    let best = "";
    for (const name of names) {
      if (name && name.length > best.length) best = name;
    }
    return best;
  }

  function directText(node) {
    if (!node || !node.childNodes) return "";
    let text = "";
    for (const child of node.childNodes) {
      if (child.nodeType === 3) text += child.textContent || "";
    }
    return clean(text);
  }

  function fileNameFromLink(link) {
    if (!link || !link.getAttribute) return "";
    const attr = fileNameFromText(link.getAttribute("download"));
    if (attr) return attr;
    for (const key of ["title", "data-filename", "data-file-name", "aria-label"]) {
      const named = fileNameFromText(link.getAttribute(key));
      if (named) return named;
    }
    const own = fileNameFromText(directText(link));
    if (own) return own;
    try {
      const url = new URL(link.getAttribute("href") || link.href || "", "https://booth.pm");
      const base = decodeURIComponent(url.pathname.split("/").filter(Boolean).pop() || "");
      const named = fileNameFromText(base);
      if (named) return named;
    } catch {
      return "";
    }
    return "";
  }

  function visibleText(node) {
    if (!node) return "";
    if (downloadControlCount(node) > 0 || (node.querySelector && node.querySelector("a[href*='/items/']"))) {
      return directText(node);
    }
    return clean(node.textContent);
  }

  function joinedExtensionName(row) {
    if (!row || !row.querySelectorAll) return "";
    for (const node of row.querySelectorAll("*")) {
      const ext = directText(node);
      if (!EXT_ONLY.test(ext)) continue;
      const stem = safeFileName(visibleText(node.previousElementSibling));
      if (!stem || FILE_EXT.test(stem) || DOWNLOAD_LABEL.test(stem) || EXT_ONLY.test(stem)) continue;
      const named = safeFileName(`${stem}${ext}`);
      if (named && FILE_EXT.test(named)) return named;
    }
    return "";
  }

  function urlsOf(node) {
    const urls = [];
    if (!node || !node.getAttribute) return urls;
    for (const key of ["href", "data-href"]) {
      const value = node.getAttribute(key);
      if (value) urls.push(value);
    }
    const raw = node.getAttribute("data-dropdown-items");
    if (!raw) return urls;
    try {
      const items = JSON.parse(raw);
      if (!Array.isArray(items)) return urls;
      for (const item of items) {
        if (!item || typeof item !== "object") continue;
        for (const key of ["fallbackUrl", "deeplinkDownloadableUrl", "url", "href"]) {
          if (item[key]) urls.push(String(item[key]));
        }
      }
    } catch {
      // 壊れた JSON はリンクとして数えない。
    }
    return urls;
  }

  function idsOf(node) {
    const ids = [];
    for (const url of urlsOf(node)) {
      const id = downloadableIdFrom(url);
      if (id && !ids.includes(id)) ids.push(id);
    }
    return ids;
  }

  function isDownloadControl(node) {
    if (!node || !node.getAttribute) return false;
    if (node.closest) {
      const parentButton = node.closest("[data-dropdown-items], .js-download-button, .js-download-free-button");
      if (parentButton && parentButton !== node) return false;
    }
    if (node.hasAttribute("data-dropdown-items") || node.hasAttribute("download")) return true;
    if (node.classList && (node.classList.contains("js-download-button") || node.classList.contains("js-download-free-button"))) {
      return true;
    }
    const href = String(node.getAttribute("href") || node.getAttribute("data-href") || "");
    return /download/i.test(href);
  }

  function downloadControls(scope) {
    if (!scope || !scope.querySelectorAll) return [];
    return [...scope.querySelectorAll(
      "a[href], a[download], [data-href], [data-dropdown-items], .js-download-button, .js-download-free-button",
    )].filter(isDownloadControl);
  }

  function downloadControlCount(scope) {
    return downloadControls(scope).length + (isDownloadControl(scope) ? 1 : 0);
  }

  function downloadableIdsIn(scope) {
    const ids = new Set();
    const controls = downloadControls(scope);
    if (isDownloadControl(scope) && !controls.includes(scope)) controls.push(scope);
    for (const control of controls) {
      const found = idsOf(control);
      if (found.length === 0) ids.add(`anon:${ids.size}`);
      for (const id of found) ids.add(id);
    }
    return ids;
  }

  function fileSearchRoot(node) {
    const variation = node.closest && node.closest(".variation-item, [data-variation]");
    if (variation && downloadableIdsIn(variation).size <= 1) return variation;
    let current = node;
    while (current && current.parentElement) {
      const parent = current.parentElement;
      if (parent.matches && parent.matches("body, main, html")) break;
      if (downloadableIdsIn(parent).size > 1) break;
      if (parent.querySelector && parent.querySelector("a[href*='/items/']") && current !== node) break;
      current = parent;
    }
    return current || node;
  }

  function nearbyNodes(node) {
    const root = fileSearchRoot(node);
    if (root !== node) return [root];
    const nodes = [node];
    let prev = node.previousElementSibling;
    while (prev) {
      if (prev.matches && prev.matches(
        "a[href*='/items/'], .text-text-gray600, .item-card__shop-name, .item-card__shop-name-anchor",
      )) {
        break;
      }
      if (prev.querySelector && prev.querySelector("a[href*='/items/']")) break;
      if (downloadControlCount(prev) > 0) break;
      nodes.push(prev);
      prev = prev.previousElementSibling;
    }
    return nodes;
  }

  function attributeFileName(element) {
    if (!element || !element.getAttribute) return "";
    for (const key of ["title", "data-filename", "data-file-name", "aria-label", "download"]) {
      const named = fileNameFromText(element.getAttribute(key));
      if (named) return named;
    }
    return "";
  }

  function fileNameInContainer(root, control) {
    if (!root) return "";
    const texts = [];
    const attributes = [];
    const tokens = [];
    const elements = root.querySelectorAll ? [root, ...root.querySelectorAll("*")] : [root];
    for (const element of elements) {
      if (!element || element === control) continue;
      if (control && control.contains && control.contains(element)) continue;
      if (element.closest && element.closest("a[href*='/items/']")) continue;
      const fromAttribute = attributeFileName(element);
      if (fromAttribute) attributes.push(fromAttribute);
      if (isDownloadControl(element)) continue;
      if (downloadControlCount(element) > 0) continue;
      if (element.querySelector && element.querySelector("a[href*='/items/']")) continue;
      const text = clean(element.textContent);
      const whole = fileNameFromText(text);
      if (whole) texts.push(whole);
      else tokens.push(...filenameTokens(text));
    }
    const joined = joinedExtensionName(root);
    if (joined) texts.push(joined);
    return shortestName(texts) || longestName(attributes) || longestName(tokens);
  }

  function shortestName(names) {
    let best = "";
    for (const name of names) {
      if (name && (!best || name.length < best.length)) best = name;
    }
    return best;
  }

  function fileNameNear(node) {
    const fromControl = fileNameFromLink(node);
    if (fromControl) return fromControl;
    const root = fileSearchRoot(node);
    const named = fileNameInContainer(root, node);
    if (named) return named;
    const scopes = [];
    let prev = root.previousElementSibling;
    while (prev) {
      if (downloadableIdsIn(prev).size > 0) break;
      if (prev.querySelector && prev.querySelector("a[href*='/items/']")) break;
      if (prev.matches && prev.matches(
        ".text-text-gray600, .item-card__shop-name, .item-card__shop-name-anchor",
      )) {
        break;
      }
      scopes.push(prev);
      prev = prev.previousElementSibling;
    }
    for (const scope of scopes) {
      const found = fileNameInContainer(scope, node);
      if (found) return found;
    }
    return "";
  }

  function variationFromRow(node) {
    for (const row of nearbyNodes(node)) {
      if (!row || !row.querySelector) continue;
      const named = row.querySelector(".variation-name");
      const marked = limitedName((named && directText(named)) || (named && named.textContent) || row.getAttribute("data-variation") || "", 80);
      if (marked && !DOWNLOAD_LABEL.test(marked)) return marked;
      const labels = row.querySelectorAll ? row.querySelectorAll("div, span, h3, h4, strong, p") : [];
      for (const label of labels) {
        if (label.closest && label.closest("a[href*='downloadables'], a[download], .js-download-button, .js-download-free-button, .variation-price, .price")) {
          continue;
        }
        if (label.querySelector && label.querySelector("a[href*='/items/']")) continue;
        if (label.matches && label.matches(".text-text-gray600, .item-card__shop-name, .item-card__shop-name-anchor")) continue;
        const text = limitedName(directText(label), 40);
        if (!text || DOWNLOAD_LABEL.test(text) || VARIATION_NOISE.test(text)) continue;
        if (fileNameFromText(text) || EXT_ONLY.test(text) || filenameTokens(text).length > 0) continue;
        if (text.includes("…") || text.includes("...")) continue;
        if (/^[¥￥]/.test(text) || /^\(?\d+(?:\.\d+)?\s*(?:B|KB|MB|GB)\)?$/i.test(text)) continue;
        if (text === "R-18" || text === "R18") continue;
        return text;
      }
    }
    return "";
  }

  function sizeFromLink(link) {
    if (!link.hasAttribute("data-size")) return null;
    const size = Number(link.getAttribute("data-size"));
    if (!Number.isFinite(size) || size < 0) return null;
    return size;
  }

  function inRecommendation(node, skipCards) {
    return Boolean(skipCards && node.closest && node.closest(".item-card"));
  }

  function variationNodes(scope, skipCards) {
    if (!scope || !scope.querySelectorAll) return [];
    return [...scope.querySelectorAll("[data-variation], .variation-item")].filter((node) => {
      if (inRecommendation(node, skipCards)) return false;
      const parent = node.parentElement && node.parentElement.closest("[data-variation], .variation-item");
      return !parent;
    });
  }

  function pushVariation(variations, name, owned) {
    const cleaned = limitedName(name, 80);
    if (!cleaned) return;
    const existing = variations.find((row) => row.name === cleaned);
    if (existing) {
      if (owned) existing.owned = true;
      return;
    }
    variations.push({ name: cleaned, owned: Boolean(owned) });
  }

  function pushDownload(downloads, seen, fields) {
    const id = fields.downloadableId || "";
    const filename = safeFileName(fields.filename || id);
    if (!filename) return;
    const variation = limitedName(fields.variation, 80) || "";
    const key = id ? `id:${id}` : `file:${variation}\0${filename}`;
    const existing = id
      ? downloads.find((row) => row.downloadableId === id)
      : downloads.find((row) => !row.downloadableId && row.filename === filename && row.variation === variation);
    if (seen.has(key) || existing) {
      if (existing && fileNameFromText(filename) && !fileNameFromText(existing.filename)) existing.filename = filename;
      if (existing && !existing.variation && variation) existing.variation = variation;
      if (existing && existing.sizeBytes == null && fields.sizeBytes != null) existing.sizeBytes = fields.sizeBytes;
      return;
    }
    seen.add(key);
    const row = { filename, sizeBytes: fields.sizeBytes ?? null, variation };
    if (id) row.downloadableId = id;
    downloads.push(row);
  }

  function readVariations(scope, fromLibrary) {
    const variations = [];
    const downloads = [];
    const seen = new Set();
    const skipCards = !fromLibrary;
    for (const node of variationNodes(scope, skipCards)) {
      const named = node.querySelector && node.querySelector(".variation-name");
      const name = limitedName((named && named.textContent) || node.getAttribute("data-variation") || "", 80);
      const marked = node.getAttribute("data-owned");
      const controls = downloadControls(node);
      let owned = false;
      if (fromLibrary) owned = true;
      else if (marked === "true" || marked === "1") owned = true;
      else owned = controls.length > 0;
      if (name && !DOWNLOAD_LABEL.test(name)) pushVariation(variations, name, owned);
    }
    const controls = downloadControls(scope).filter((control) => !inRecommendation(control, skipCards));
    for (const control of controls) {
      const ids = idsOf(control);
      const filename = fileNameNear(control);
      const variation = variationFromRow(control);
      const sizeBytes = sizeFromLink(control);
      if (variation && (fromLibrary || filename || ids.length > 0)) pushVariation(variations, variation, true);
      if (ids.length === 0) {
        if (filename) pushDownload(downloads, seen, { filename, variation, sizeBytes, downloadableId: "" });
        continue;
      }
      for (const downloadableId of ids) {
        pushDownload(downloads, seen, {
          filename: filename || downloadableId,
          variation,
          sizeBytes,
          downloadableId,
        });
      }
    }
    if (fromLibrary && scope.querySelectorAll) {
      for (const node of scope.querySelectorAll(".variation-name")) {
        if (node.closest && node.closest("[data-variation], .variation-item")) continue;
        pushVariation(variations, node.textContent, true);
      }
    }
    return { variations, downloads };
  }

  function readFree(scope, skipCards) {
    if (!scope || !scope.querySelector) return null;
    const flagged = [...scope.querySelectorAll("[data-free='true']")].find((node) => !inRecommendation(node, skipCards));
    if (flagged) return true;
    const price = [...scope.querySelectorAll("[data-price], .price, .item-price, .variation-price")].find(
      (node) => !inRecommendation(node, skipCards),
    );
    if (!price) return null;
    const text = clean(price.textContent);
    if (!text) return null;
    if (text.includes("無料") || /^[¥￥]\s*0(?:\D|$)/.test(text)) return true;
    if (/[1-9]/.test(text)) return false;
    return null;
  }

  function readAdult(scope, skipCards) {
    if (!scope || !scope.querySelector) return null;
    const flagged = [...scope.querySelectorAll("[data-adult='true']")].find((node) => !inRecommendation(node, skipCards));
    if (flagged) return true;
    const badge = [...scope.querySelectorAll(".adult-badge, [data-rating]")].find(
      (node) => !inRecommendation(node, skipCards),
    );
    const text = clean(badge && badge.textContent);
    if (text === "R-18" || text === "R18") return true;
    return null;
  }

  function makeProduct(fields) {
    const product = {
      boothId: fields.boothId,
      title: fields.title,
      shop: fields.shop || "",
      descriptionSnippet: trimDescription(fields.description || ""),
      url: fields.url || `https://booth.pm/ja/items/${fields.boothId}`,
      thumbnailUrl: cardImage(fields.image),
      variations: fields.variations || [],
      downloads: fields.downloads || [],
    };
    if (fields.isFree === true || fields.isFree === false) product.isFree = fields.isFree;
    if (fields.isAdult === true || fields.isAdult === false) product.isAdult = fields.isAdult;
    if (fields.unpublished === true) product.unpublished = true;
    return product;
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
    const facts = readVariations(document, false);
    return makeProduct({
      boothId,
      title,
      shop: brandName(linked) || mainShop(document),
      description,
      url: absoluteUrl(href, "https://booth.pm"),
      image,
      variations: facts.variations,
      downloads: facts.downloads,
      isFree: readFree(document, true),
      isAdult: readAdult(document, true) === true ? true : null,
    });
  }

  function bytesFromFileSize(value) {
    if (typeof value === "number" && Number.isFinite(value) && value >= 0) return Math.round(value);
    return null;
  }

  function priceIsFree(price) {
    if (typeof price === "number") return price === 0;
    const text = clean(price);
    if (!text) return null;
    if (text.includes("無料")) return true;
    if (!/[¥￥]|JPY|円/.test(text)) return null;
    const digits = text.replace(/[^\d]/g, "");
    if (!digits) return null;
    return Number(digits) === 0;
  }

  function filesFromDownloadable(downloadable, variation) {
    const lists = [];
    if (Array.isArray(downloadable)) lists.push(downloadable);
    else if (downloadable && typeof downloadable === "object") {
      for (const key of ["no_musics", "noMusics", "musics", "files"]) {
        if (Array.isArray(downloadable[key])) lists.push(downloadable[key]);
      }
    }
    const files = [];
    for (const list of lists) {
      for (const file of list) {
        if (!file || typeof file !== "object") continue;
        const extension = clean(file.file_extension || file.fileExtension);
        const stem = clean(file.file_name || file.fileName);
        const suffix = extension ? (extension.startsWith(".") ? extension : `.${extension}`) : "";
        const composed = stem && suffix && !FILE_EXT.test(stem) ? `${stem}${suffix}` : stem;
        const filename = fileNameFromText(file.name) || fileNameFromText(composed);
        const id = downloadableIdFrom(file.url);
        if (!filename && !id) continue;
        files.push({
          filename: filename || id,
          downloadableId: id,
          sizeBytes: bytesFromFileSize(file.file_size ?? file.fileSize),
          variation,
        });
      }
    }
    return files;
  }

  function detailFromItemJson(data, boothId) {
    if (!data || typeof data !== "object") return null;
    const id = String(data.id || boothId || "");
    if (!validId(id)) return null;
    const shop = data.shop && typeof data.shop === "object" ? data.shop.name : "";
    const variations = [];
    const downloads = [];
    const rows = Array.isArray(data.variations) ? data.variations : [];
    for (const row of rows) {
      if (!row || typeof row !== "object") continue;
      const name = limitedName(row.name, 80);
      const status = String(row.status || "");
      const owned = status === "free_download" || status === "downloadable" || status === "owned";
      if (name && owned) pushVariation(variations, name, true);
      for (const file of filesFromDownloadable(row.downloadable, name || "")) downloads.push(file);
    }
    for (const file of filesFromDownloadable(data.downloadable, "")) downloads.push(file);
    let isFree = priceIsFree(data.price);
    if (isFree == null && rows.some((row) => row && row.status === "free_download")) isFree = true;
    let isAdult = null;
    if (data.is_adult === true || data.isAdult === true) isAdult = true;
    else if (data.is_adult === false || data.isAdult === false) isAdult = false;
    return makeProduct({
      boothId: id,
      title: clean(data.name) || `item ${id}`,
      shop: clean(shop),
      description: typeof data.description === "string" ? data.description : "",
      url: clean(data.url) || `https://booth.pm/ja/items/${id}`,
      image: "",
      variations,
      downloads,
      isFree,
      isAdult,
    });
  }

  function mergeDownload(downloads, file) {
    const id = file.downloadableId || "";
    const existing = downloads.find((row) =>
      (id && row.downloadableId === id)
      || (row.filename === file.filename && (row.variation || "") === (file.variation || "")),
    );
    if (!existing) {
      const row = {
        filename: file.filename,
        sizeBytes: file.sizeBytes ?? null,
        variation: file.variation || "",
      };
      if (id) row.downloadableId = id;
      downloads.push(row);
      return;
    }
    if (fileNameFromText(file.filename) && !fileNameFromText(existing.filename)) existing.filename = file.filename;
    if (!existing.downloadableId && id) existing.downloadableId = id;
    if (existing.sizeBytes == null && file.sizeBytes != null) existing.sizeBytes = file.sizeBytes;
    if (!existing.variation && file.variation) existing.variation = file.variation;
  }

  function mergeDetail(product, detail) {
    if (!product || !detail) return product;
    if ((detail.descriptionSnippet || "").length > (product.descriptionSnippet || "").length) {
      product.descriptionSnippet = detail.descriptionSnippet;
    }
    if (!product.shop && detail.shop) product.shop = detail.shop;
    if (!product.thumbnailUrl && detail.thumbnailUrl) product.thumbnailUrl = detail.thumbnailUrl;
    if (product.isFree == null && (detail.isFree === true || detail.isFree === false)) product.isFree = detail.isFree;
    if (product.isAdult == null && (detail.isAdult === true || detail.isAdult === false)) product.isAdult = detail.isAdult;
    product.variations = product.variations || [];
    for (const variation of detail.variations || []) {
      if (variation && variation.owned) pushVariation(product.variations, variation.name, true);
    }
    product.downloads = product.downloads || [];
    for (const download of detail.downloads || []) mergeDownload(product.downloads, download);
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

  const ITEM_ID_ATTRS = ["data-product-id", "data-item-id", "data-booth-id", "data-productid", "data-itemid"];

  function unpublishedBadge(scope) {
    if (!scope || !scope.querySelectorAll) return false;
    const nodes = scope.querySelectorAll("span, div, p, small, em, strong, b, i, label");
    for (const node of nodes) {
      if (node.children && node.children.length > 0) continue;
      if (clean(node.textContent) === "非公開") return true;
    }
    return false;
  }

  function attrItemId(node) {
    if (!node || !node.getAttribute) return "";
    for (const name of ITEM_ID_ATTRS) {
      const value = clean(node.getAttribute(name));
      if (validId(value)) return value;
    }
    return "";
  }

  function imageItemId(value) {
    const fromItems = idFromUrl(value);
    if (validId(fromItems)) return fromItems;
    let url;
    try {
      url = new URL(String(value || ""), "https://booth.pm");
    } catch {
      return "";
    }
    const host = url.hostname;
    if (!host.endsWith("pximg.net") && !/(^|\.)booth\.pm$/.test(host)) return "";
    const parts = url.pathname.split("/").filter(Boolean);
    const candidates = [];
    for (let index = 0; index < parts.length; index += 1) {
      const part = parts[index];
      if (!validId(part)) continue;
      const prev = parts[index - 1] || "";
      if (prev === "users" || prev === "c" || prev === "shops" || /^\d+x\d+$/.test(prev)) continue;
      candidates.push(part);
    }
    return candidates.length === 1 ? candidates[0] : "";
  }

  function soleCardId(scope, baseHref) {
    const ids = [];
    function add(id) {
      if (validId(id) && !ids.includes(id)) ids.push(id);
    }
    const nodes = scope && scope.querySelectorAll ? [scope, ...scope.querySelectorAll("*")] : [];
    for (const node of nodes) add(attrItemId(node));
    const anchors = scope && scope.querySelectorAll ? scope.querySelectorAll("a[href]") : [];
    for (const anchor of anchors) {
      add(idFromUrl(absoluteUrl(anchor.getAttribute("href") || anchor.href || "", baseHref)));
    }
    const images = scope && scope.querySelectorAll ? scope.querySelectorAll("img") : [];
    for (const image of images) {
      const src = image.getAttribute("src") || image.src || "";
      const srcset = image.getAttribute("srcset") || "";
      add(imageItemId(src));
      add(imageItemId(srcset));
    }
    return ids.length === 1 ? ids[0] : "";
  }

  function isFreeDownloadsPath(href) {
    try {
      const path = new URL(href, "https://accounts.booth.pm").pathname.replace(/\/+$/, "");
      return path === "/library/free_downloads";
    } catch {
      return false;
    }
  }

  function titledAnchor(scope, boothId, baseHref) {
    if (!scope || !scope.querySelectorAll) return "";
    for (const link of scope.querySelectorAll("a[href*='/items/']")) {
      const linkHref = absoluteUrl(link.getAttribute("href") || link.href || "", baseHref);
      if (idFromUrl(linkHref) !== boothId) continue;
      const titled = clean(link.getAttribute("title")) || clean(link.textContent);
      if (titled) return titled;
    }
    return "";
  }

  function productFromLibraryCard(scope, baseHref, preferredHref, imageNode) {
    if (!scope) return null;
    const unpublished = unpublishedBadge(scope);
    let href = preferredHref || "";
    let boothId = idFromUrl(href);
    if (!validId(boothId)) {
      if (!unpublished) return null;
      boothId = soleCardId(scope, baseHref);
      if (!validId(boothId)) return null;
      href = `https://booth.pm/ja/items/${boothId}`;
    }
    const image = imageNode || (scope.querySelector && scope.querySelector("img"));
    const title = libraryTitle(scope, boothId, baseHref)
      || clean(image && image.getAttribute("alt"))
      || titledAnchor(scope, boothId, baseHref);
    if (!title) return null;
    const shopNode = scope.querySelector && scope.querySelector(
      ".text-text-gray600, .item-card__shop-name-anchor, .item-card__shop-name",
    );
    const facts = readVariations(scope, true);
    return makeProduct({
      boothId,
      title: title.slice(0, 300),
      shop: clean(shopNode && shopNode.textContent).slice(0, 120),
      description: "",
      url: href,
      image: image ? image.getAttribute("src") || image.src || "" : "",
      variations: facts.variations,
      downloads: facts.downloads,
      isFree: readFree(scope, false),
      isAdult: readAdult(scope, false) === true ? true : null,
      unpublished: unpublished ? true : null,
    });
  }

  function cardFrom(anchor, root, baseHref) {
    const scope = root || anchor;
    const href = absoluteUrl(anchor.getAttribute("href") || anchor.href || "", baseHref);
    return productFromLibraryCard(scope, baseHref, href, null);
  }

  function libraryProduct(image, baseHref) {
    const card = cardRoot(image) || image.parentElement;
    const imageAnchor = image.closest && image.closest("a[href*='/items/']");
    const anchor = imageAnchor || (card && card.querySelector("a[href*='/items/']"));
    const href = anchor ? absoluteUrl(anchor.getAttribute("href") || anchor.href || "", baseHref) : "";
    return productFromLibraryCard(card, baseHref, href, image);
  }

  function pushUnpublishedCards(document, href, push) {
    const badges = document.querySelectorAll("span, div, p, small, em, strong, b, i, label");
    for (const node of badges) {
      if (node.children && node.children.length > 0) continue;
      if (clean(node.textContent) !== "非公開") continue;
      const card = (node.closest && node.closest(".bg-white, .item-card, li, article")) || node.parentElement;
      if (!card) continue;
      const image = card.querySelector && card.querySelector("img");
      if (image) {
        push(libraryProduct(image, href));
        continue;
      }
      const anchor = card.querySelector && card.querySelector("a[href*='/items/']");
      if (anchor) push(cardFrom(anchor, card, href));
      else push(productFromLibraryCard(card, href, "", null));
    }
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
    if (libraryPath) pushUnpublishedCards(document, href, push);
    if (isFreeDownloadsPath(href)) {
      for (const product of products) {
        if (product.isFree == null) product.isFree = true;
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
    // ライブラリの「非公開」は公開終了ではない。404 でもカテゴリ用の説明へ置き換えない。
    if (product.unpublished === true) return product;
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

  function diagnose(document, href) {
    const products = extractPage(document, href || "https://booth.pm/library");
    let variationCount = 0;
    let filenameCount = 0;
    let downloadCount = 0;
    for (const product of products) {
      variationCount += (product.variations || []).length;
      for (const download of product.downloads || []) {
        downloadCount += 1;
        if (fileNameFromText(download.filename)) filenameCount += 1;
      }
    }
    const selectors = [
      "img.l-library-item-thumbnail",
      "a[href*='/items/']",
      ".bg-white",
      ".variation-item",
      ".variations",
      ".variation-name",
      "[data-variation]",
      "a[href*='downloadables']",
      ".js-download-button",
      ".js-download-free-button",
      "[data-dropdown-items]",
      "[data-href*='downloadables']",
      "a[download]",
      ".variation-price",
    ];
    const selectorHits = {};
    for (const selector of selectors) {
      try {
        selectorHits[selector] = document.querySelectorAll(selector).length;
      } catch {
        selectorHits[selector] = 0;
      }
    }
    const hrefShapes = {};
    let downloadLinkCount = 0;
    const nodes = document.querySelectorAll
      ? document.querySelectorAll("a[href], [data-href], [data-dropdown-items]")
      : [];
    for (const node of nodes) {
      for (const url of urlsOf(node)) {
        if (!/download/i.test(url)) continue;
        const shape = hrefShape(url);
        if (!shape) continue;
        downloadLinkCount += 1;
        hrefShapes[shape] = (hrefShapes[shape] || 0) + 1;
      }
    }
    return {
      productCount: products.length,
      variationCount,
      downloadLinkCount,
      filenameCount,
      missingFilenameCount: downloadCount - filenameCount,
      selectorHits,
      hrefShapes,
    };
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
    diagnose,
    UNAVAILABLE_TEXT,
  };
});
