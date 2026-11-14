// Read only the confirmed checkout's own item records, never recommendation links.
(function (root) {
  function checkout(document, href) {
    const url = new URL(href);
    if (url.protocol !== "https:" || url.hostname !== "checkout.booth.pm" || url.port || url.username || url.password || !/^\/orders\/\d+$/.test(url.pathname)) return [];
    if (document.querySelector('meta[name="xrcloud:event"]')?.getAttribute("content") !== "purchased") return [];
    if (document.querySelector(".page-header__title")?.textContent.trim() !== "ご注文が完了しました") return [];
    const order = document.querySelector('[data-tracking="order"][data-order-id]');
    if (order?.getAttribute("data-order-id") !== url.pathname.split("/").pop()) return [];
    const products = new Map();
    for (const node of document.querySelectorAll('[data-tracking="item"][data-product-id]')) {
      const id = node.getAttribute("data-product-id") || "";
      const title = (node.getAttribute("data-product-name") || "").trim();
      if (!/^\d{4,12}$/.test(id) || !title || title.length > 1000) continue;
      products.set(id, { boothId: id, title, shop: node.getAttribute("data-product-brand") || "", url: `https://booth.pm/ja/items/${id}` });
    }
    return [...products.values()];
  }
  function chooseDownload(products, id) {
    if (!/^\d{4,12}$/.test(id || "")) return null;
    const matches = products.flatMap(product => (product.downloads || [])
      .filter(file => file.downloadableId === id)
      .map(file => ({ product, variation: file.variation || "", filename: file.filename, downloadableId: id })));
    return matches.length === 1 ? matches[0] : null;
  }
  const api = { checkout, chooseDownload };
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.VrcLibManAuto = api;
})(globalThis);
