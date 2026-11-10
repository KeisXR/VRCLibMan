// BOOTH の購入履歴（accounts.booth.pm/orders）と、注文 1 件の詳細ページだけを読む。
// 返すのは注文番号、状態、日時、金額、商品 ID・名前・ショップ、ギフトかどうかの真偽値だけ。
// 支払方法、住所、氏名、電話番号、ギフト用 URL、ダウンロード URL は読まない。
(function (root) {
  const ORIGIN = "https://accounts.booth.pm";
  const ORDER_ID = /^\d{4,12}$/;
  const MAX_PAGE = 200;
  const MAX_NAME = 1000;

  function clean(text) {
    return String(text == null ? "" : text).replace(/\s+/g, " ").trim();
  }

  function parseUrl(href, base) {
    try {
      return new URL(href, base);
    } catch (_) {
      return null;
    }
  }

  function ordersUrl(href) {
    const url = parseUrl(href);
    if (!url || url.origin !== ORIGIN || url.username || url.password) return null;
    return url;
  }

  function parseYen(text) {
    const match = clean(text).match(/[¥￥]\s*([\d,]+)/) || clean(text).match(/([\d,]+)\s*円/);
    if (!match) return null;
    const value = Number(match[1].replace(/,/g, ""));
    return Number.isSafeInteger(value) && value >= 0 ? value : null;
  }

  function two(value) {
    return String(value).padStart(2, "0");
  }

  // BOOTH は日本時間で表示する。表示どおりの壁時計を +09:00 付きで残す。
  function parseOrderedAt(text) {
    const match = clean(text).match(/(\d{4})[\/.-](\d{1,2})[\/.-](\d{1,2})\s+(\d{1,2}):(\d{2})(?::(\d{2}))?/);
    if (!match) return null;
    const [, year, month, day, hour, minute, second] = match;
    const values = [Number(month), Number(day), Number(hour), Number(minute), Number(second || 0)];
    if (values[0] < 1 || values[0] > 12 || values[1] < 1 || values[1] > 31 || values[2] > 23 || values[3] > 59 || values[4] > 59) {
      return null;
    }
    return `${year}-${two(month)}-${two(day)}T${two(hour)}:${two(minute)}:${two(second || 0)}+09:00`;
  }

  // 状態の印。キャンセルと支払い待ちは確定した購入にしない。
  function orderState(node) {
    if (!node) return "unknown";
    const cls = String(node.getAttribute ? node.getAttribute("class") || "" : "").toLowerCase();
    const text = clean(node.textContent);
    if (/cancel/.test(cls) || /キャンセル|cancel/i.test(text)) return "cancelled";
    if (/unpaid|wait_?payment|payment_?wait|pending_?payment/.test(cls) || /(支払|入金)い?待ち|未払い|unpaid|awaiting payment/i.test(text)) {
      return "pending";
    }
    return "confirmed";
  }

  function pageNumber(url) {
    const page = Number(url.searchParams.get("page") || "1");
    return Number.isInteger(page) && page >= 1 && page <= MAX_PAGE ? page : 1;
  }

  function lastPage(document, href) {
    let last = 1;
    for (const link of document.querySelectorAll("a[href]")) {
      const url = parseUrl(link.getAttribute("href") || "", href);
      if (!url || url.origin !== ORIGIN || url.pathname.replace(/\/+$/, "") !== "/orders") continue;
      last = Math.max(last, pageNumber(url));
    }
    return last;
  }

  function orderIdFromPath(pathname) {
    const match = pathname.match(/^\/orders\/(\d{4,12})\/?$/);
    return match ? match[1] : null;
  }

  function orderedAtIn(scope) {
    for (const node of scope.querySelectorAll("div, span, p, time, dd")) {
      const text = clean(node.textContent);
      if (/注文日時|Order date/i.test(text) && node.children.length === 0) {
        const value = parseOrderedAt(text);
        if (value) return value;
      }
    }
    for (const node of scope.querySelectorAll("time, .u-tpg-caption2")) {
      const value = parseOrderedAt(node.getAttribute("datetime") || node.textContent);
      if (value) return value;
    }
    return null;
  }

  /** 購入履歴の一覧 1 ページ。注文ごとのリンクと状態の印があるものだけを読む。 */
  function orderList(document, href) {
    const page = ordersUrl(href);
    if (!page || page.pathname.replace(/\/+$/, "") !== "/orders") return { orders: [], lastPage: 1 };
    const orders = [];
    const seen = new Set();
    for (const link of document.querySelectorAll("a[href]")) {
      const url = parseUrl(link.getAttribute("href") || "", href);
      if (!url || url.origin !== ORIGIN) continue;
      const orderId = orderIdFromPath(url.pathname);
      if (!orderId || seen.has(orderId)) continue;
      const badge = link.querySelector(".order-state");
      if (!badge) continue;
      seen.add(orderId);
      const titleNode = link.querySelector(".u-tpg-caption1");
      const title = clean(titleNode ? titleNode.textContent : "").slice(0, MAX_NAME);
      orders.push({ orderId, state: orderState(badge), orderedAt: orderedAtIn(link), title });
    }
    return { orders, lastPage: lastPage(document, href) };
  }

  function fieldValue(scope, labels) {
    for (const node of scope.querySelectorAll("div, dt, th, span")) {
      if (node.children.length !== 0) continue;
      if (!labels.includes(clean(node.textContent))) continue;
      const value = node.nextElementSibling;
      if (value) return clean(value.textContent);
    }
    return "";
  }

  function itemIdFromUrl(url) {
    if (url.protocol !== "https:" || url.username || url.password || url.port) return null;
    if (!(url.hostname === "booth.pm" || url.hostname.endsWith(".booth.pm"))) return null;
    const match = url.pathname.match(/^(?:\/[a-z]{2}(?:-[a-z]{2,4})?)?\/items\/(\d{4,12})\/?$/i);
    return match ? match[1] : null;
  }

  function quantityIn(text) {
    const match = clean(text).match(/(?:数量|個数|Quantity)\s*[:：]?\s*(\d{1,4})/i) || clean(text).match(/[×x]\s*(\d{1,4})\s*(?:点|個)?$/);
    if (!match) return null;
    const value = Number(match[1]);
    return value >= 1 ? value : null;
  }

  // 商品名の外に金額の行がある段か。商品名そのものに「¥」や「円」が入ることがあるので、名前の側は見ない。
  function priceBesideName(node, link) {
    return [...node.children].some((child) => !child.contains(link) && /[¥￥]|円/.test(child.textContent || ""));
  }

  // 商品名のリンクから数段だけ上がり、金額の書かれた入れ物を探す。別の商品のリンクを含む段では止める。
  function itemBox(link, boothId, href) {
    let node = link.parentElement;
    for (let depth = 0; node && depth < 5; depth += 1) {
      const others = [...node.querySelectorAll("a[href]")].some((other) => {
        const url = parseUrl(other.getAttribute("href") || "", href);
        const id = url && itemIdFromUrl(url);
        return id && id !== boothId;
      });
      if (others) return null;
      if (priceBesideName(node, link)) return node;
      node = node.parentElement;
    }
    return null;
  }

  function priceLines(box, link) {
    const result = { priceYen: null, boostYen: null, quantity: null };
    if (!box) return result;
    for (const child of box.children) {
      if (child.contains(link)) continue;
      const text = clean(child.textContent);
      const quantity = quantityIn(text);
      if (quantity && result.quantity == null) result.quantity = quantity;
      const yen = parseYen(text);
      if (yen == null) continue;
      if (/BOOST/i.test(text)) {
        if (result.boostYen == null) result.boostYen = yen;
      } else if (result.priceYen == null) {
        result.priceYen = yen;
      }
    }
    return result;
  }

  function shopOf(link) {
    const group = link.closest(".l-order-detail-by-shop");
    const header = group && group.querySelector(".l-order-detail-sheet-group-header");
    if (!header) return "";
    const named = header.querySelector("a[title]");
    return clean(named ? named.getAttribute("title") || named.textContent : header.textContent).slice(0, MAX_NAME);
  }

  function orderItems(scope, href) {
    const items = [];
    const seen = new Set();
    for (const link of scope.querySelectorAll("a[href]")) {
      const name = clean(link.textContent);
      if (!name) continue;
      const url = parseUrl(link.getAttribute("href") || "", href);
      const boothId = url && itemIdFromUrl(url);
      if (!boothId) continue;
      const key = `${boothId}\u0000${name}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const prices = priceLines(itemBox(link, boothId, href), link);
      items.push({
        boothId,
        name: name.slice(0, MAX_NAME),
        shop: shopOf(link),
        priceYen: prices.priceYen,
        boostYen: prices.boostYen,
        quantity: prices.quantity || 1,
      });
    }
    return items;
  }

  // 商品名、ショップ名、ファイル名とダウンロードの並びにある文字は、ギフトの印として見ない。
  function giftSkip(node) {
    if (!node.closest) return false;
    if (node.closest("a[href*='/items/'], .l-order-detail-sheet-group-header, script, style, noscript")) return true;
    const row = node.closest(".legacy-list-item");
    return Boolean(row && row.querySelector(".js-download-button, [data-test='downloadable']"));
  }

  /**
   * 贈ったギフトの注文か。ギフトの印は詳細ページにしか出ないことがあるので、詳細でだけ判定する。
   * 商品名、ショップ名、ファイル名の中の「ギフト」は見ない。ギフト用 URL の中身は返さない。
   */
  function giftMarked(scope) {
    const nodes = [scope, ...scope.querySelectorAll("*")];
    for (const node of nodes) {
      if (giftSkip(node)) continue;
      const cls = String(node.getAttribute ? node.getAttribute("class") || "" : "");
      if (/(^|[\s_-])gift/i.test(cls)) return true;
      for (const name of ["title", "aria-label", "alt", "name", "data-test", "data-tracking"]) {
        const value = node.getAttribute ? node.getAttribute(name) || "" : "";
        if (/ギフト|gift/i.test(value)) return true;
      }
      const target = node.getAttribute ? node.getAttribute("href") || "" : "";
      if (/gift/i.test(target)) return true;
      for (const child of node.childNodes || []) {
        if (child.nodeType === 3 && /ギフト|gift/i.test(child.textContent || "")) return true;
      }
    }
    return false;
  }

  /** 注文 1 件の詳細ページ。URL の注文番号とページの注文番号が一致するときだけ返す。 */
  function orderDetail(document, href) {
    const page = ordersUrl(href);
    const orderId = page && orderIdFromPath(page.pathname);
    if (!orderId) return null;
    const scope = document.querySelector(".manage-page-body") || document.body;
    if (!scope) return null;
    const shown = fieldValue(scope, ["注文番号", "Order number", "Order No."]).replace(/\D/g, "");
    if (shown && shown !== orderId) return null;
    const orderedAt = parseOrderedAt(fieldValue(scope, ["注文日時", "Order date"]));
    const items = orderItems(scope, href);
    if (!orderedAt && items.length === 0) return null;
    return {
      orderId,
      state: orderState(scope.querySelector(".order-state")),
      orderedAt,
      totalYen: parseYen(fieldValue(scope, ["お支払金額", "お支払い金額", "支払金額", "Payment amount", "Total"])),
      gift: giftMarked(scope),
      items,
    };
  }

  const api = {
    orderList,
    orderDetail,
    parseOrderedAt,
    parseYen,
    orderState,
    lastPage,
    isOrderId: (id) => ORDER_ID.test(String(id || "")),
  };
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.VrcLibManPurchases = api;
})(globalThis);
