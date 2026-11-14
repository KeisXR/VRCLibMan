// 本人が開いた購入完了ページ・購入履歴・注文詳細のページだけを読み、サービスワーカーへ渡す。
// ページの移動はしない。購入完了ページは購入完了の見出しと注文番号が一致したときの専用データだけを読む。
(function () {
  const href = location.href;
  let url;
  try {
    url = new URL(href);
  } catch {
    return;
  }
  if (url.protocol !== "https:" || url.port || url.username || url.password) return;
  const send = (message) => chrome.runtime.sendMessage(message).catch(() => {});
  const path = url.pathname.replace(/\/+$/, "");
  if (url.hostname === "checkout.booth.pm") {
    const orderId = /^\/orders\/(\d{4,12})$/.exec(url.pathname)?.[1];
    const auto = globalThis.VrcLibManAuto;
    if (!orderId || !auto) return;
    const products = auto.checkout(document, href);
    if (products.length) send({ type: "checkout", href, orderId, products });
    return;
  }
  const purchases = globalThis.VrcLibManPurchases;
  if (url.hostname !== "accounts.booth.pm" || !purchases) return;
  if (path === "/orders") {
    const listed = purchases.orderList(document, href);
    if (listed.orders.length) send({ type: "orders-page", kind: "list", href, orders: listed.orders });
  } else if (/^\/orders\/\d{4,12}$/.test(path)) {
    const detail = purchases.orderDetail(document, href);
    if (detail) send({ type: "orders-page", kind: "detail", href, detail });
  }
})();
