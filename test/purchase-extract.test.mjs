import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { parseHTML } from "linkedom";

const require = createRequire(import.meta.url);
const { orderList, orderDetail, parseOrderedAt, parseYen, orderState } = require("../src-tauri/src/purchase_extract.cjs");

// 実際のページの形だけをまねた見本。商品名、ショップ名、金額は架空のもの。
function listCard(id, state, label, title, at) {
  return `<a class="nav-reverse" href="/orders/${id}"><div class="sheet"><div class="relative flex">
    <div class="pl-8"><img src="https://booth.pximg.net/c/72x72_a2_g5/x/i/1000/a.jpg"></div>
    <div class="flex-1 pl-16"><div class="badge mx-0 align-top order-state ${state}">${label}</div>
    <div class="u-tpg-caption1 text-[#505c6b]">${title}</div>
    <div class="u-tpg-caption2 text-[#505c6b]">注文日時: ${at}</div></div></div>
    <button class="exclude-btn">除外する</button></div></a>`;
}

function listPage(cards, last = 3) {
  const pager = Array.from({ length: last }, (_, index) => `<li><a class="nav-item" href="/orders?page=${index + 1}">${index + 1}</a></li>`).join("");
  return parseHTML(`<html><body><div class="manage-page-body"><div class="container static-3of4 l-orders-index">
    ${cards}<ul>${pager}<li><a class="nav-item last-page" href="/orders?page=${last}">»</a></li></ul>
    <a href="/orders/55555555">リンクだけ</a></div></div></body></html>`).document;
}

function itemSheet(id, name, price, boost, extra = "") {
  return `<div class="sheet sheet--p400"><div class="flex"><div class="u-mr-500"><a href="https://sample.booth.pm/items/${id}"><img src="https://booth.pximg.net/c/72x72/x.jpg"></a></div>
    <div class="flex-[1]"><div class="desktop:flex"><div class="desktop:flex-1">
      <div class="text-14"><b><a class="nav" href="https://sample.booth.pm/items/${id}">${name}</a></b></div>
      <div class="u-tpg-caption1">¥ ${price}</div>
      <div class="u-tpg-caption1"><span class="particulars-heading">BOOST<i class="icon-boost"></i></span>¥ ${boost}</div>${extra}
    </div><div class="flex gap-8"><a href="https://twitter.com/intent/tweet?text=x">ポストする</a></div></div></div></div>
    <div class="list"><div class="legacy-list-item"><div class="legacy-list-item__center"><div><b>gift_wrapping_set.zip</b></div>
      <div class="js-download-button" data-href="https://booth.pm/downloadables/1234567" data-test="downloadable"><button>ダウンロード</button></div></div></div></div></div>`;
}

function detailPage(id, { state = "completed", label = "発送完了", at = "2026/09/13 08:26:39", total = "2,300", shops = null, extra = "", shown = id } = {}) {
  const groups = shops ?? [["見本ショップ", itemSheet("1234567", "見本の衣装 (フルセット)", "2,000", "300")]];
  const body = groups.map(([shop, sheets]) => `<div class="l-order-detail-by-shop"><div class="l-order-detail-sheet-group-header"><b><a class="nav u-tpg-title3" title="${shop}" href="https://sample.booth.pm/">${shop}</a></b></div>
    <div class="sheet-group"><div class="sheet sheet--p400"><b class="u-tpg-title3">ダウンロード商品</b><a href="https://sample.booth.pm/conversations/new">ショップにメッセージを送る</a></div>${sheets}</div></div>`).join("");
  return parseHTML(`<html><body><div class="manage-page-body"><div class="container static-width"><div class="sheet">
    <div><span class="mx-0 badge order-state ${state}">${label}</span></div>
    <div class="u-mt-500"><div class="l-row text-14">
      <div class="l-col-pc-3">注文日時</div><div class="l-col-pc-9">${at}</div>
      <div class="l-col-pc-3">注文番号</div><div class="l-col-pc-9">${shown}</div>
      <div class="l-col-pc-3">お支払方法</div><div class="l-col-pc-9">見本の支払方法</div>
      <div class="l-col-pc-3">お支払金額</div><div class="l-col-pc-9">¥ ${total}</div>
    </div></div></div>${extra}${body}
    <div class="l-order-detail-by-shop"><form action="https://accounts.booth.pm/orders/${id}/order_qualified_invoices"><input type="submit" value="発行できません"></form></div>
    <div class="step-navs"><a class="nav" href="/orders">戻る</a></div></div></div></body></html>`).document;
}

test("the order list keeps order links with a state badge and reads the date to the second", () => {
  const document = listPage(
    listCard("90000001", "completed", "発送完了", "見本の衣装 (フルセット)", "2026/11/02 09:30:20") +
      listCard("90000002", "cancelled", "キャンセル", "見本の髪", "2026/10/14 11:09:50"),
    7,
  );
  const result = orderList(document, "https://accounts.booth.pm/orders?page=2");
  assert.equal(result.lastPage, 7);
  assert.deepEqual(result.orders, [
    { orderId: "90000001", state: "confirmed", orderedAt: "2026-11-02T09:30:20+09:00", title: "見本の衣装 (フルセット)" },
    { orderId: "90000002", state: "cancelled", orderedAt: "2026-10-14T11:09:50+09:00", title: "見本の髪" },
  ]);
});

test("the order list reads nothing outside the order history", () => {
  const document = listPage(listCard("90000001", "completed", "発送完了", "見本", "2026/11/02 09:30:20"));
  for (const href of ["https://accounts.booth.pm/library", "https://booth.pm/orders", "https://accounts.booth.pm/orders/90000001", "http://accounts.booth.pm/orders"]) {
    assert.deepEqual(orderList(document, href).orders, [], href);
  }
});

test("an order detail gives the date, total, items, price and BOOST, and not the payment method", () => {
  const detail = orderDetail(detailPage("87614554"), "https://accounts.booth.pm/orders/87614554");
  assert.deepEqual(detail, {
    orderId: "87614554",
    state: "confirmed",
    orderedAt: "2026-09-13T08:26:39+09:00",
    totalYen: 2300,
    gift: false,
    items: [{ boothId: "1234567", name: "見本の衣装 (フルセット)", shop: "見本ショップ", priceYen: 2000, boostYen: 300, quantity: 1 }],
  });
  assert.ok(!JSON.stringify(detail).includes("見本の支払方法"));
  assert.ok(!JSON.stringify(detail).includes("downloadables"));
});

test("items from two shops keep their own shop, price and quantity", () => {
  const document = detailPage("87614555", {
    total: "1,700",
    shops: [
      ["一つ目のショップ", itemSheet("2000001", "見本のアクセサリ", "500", "0") + itemSheet("2000002", "見本の小物", "200", "0", '<div class="u-tpg-caption1">数量: 2</div>')],
      ["二つ目のショップ", itemSheet("2000003", "見本のテクスチャ", "800", "0")],
    ],
  });
  const detail = orderDetail(document, "https://accounts.booth.pm/orders/87614555");
  assert.deepEqual(
    detail.items.map((item) => [item.boothId, item.shop, item.priceYen, item.quantity]),
    [
      ["2000001", "一つ目のショップ", 500, 1],
      ["2000002", "一つ目のショップ", 200, 2],
      ["2000003", "二つ目のショップ", 800, 1],
    ],
  );
});

test("a cancelled or unpaid order is not a confirmed purchase", () => {
  const cancelled = orderDetail(detailPage("80000001", { state: "cancelled", label: "キャンセル" }), "https://accounts.booth.pm/orders/80000001");
  assert.equal(cancelled.state, "cancelled");
  const unpaid = orderDetail(detailPage("80000002", { state: "wait_payment", label: "支払い待ち" }), "https://accounts.booth.pm/orders/80000002");
  assert.equal(unpaid.state, "pending");
  const shipping = orderDetail(detailPage("80000003", { state: "paid", label: "発送待ち" }), "https://accounts.booth.pm/orders/80000003");
  assert.equal(shipping.state, "confirmed");
});

test("a detail page for another order, or outside the order history, is not read", () => {
  assert.equal(orderDetail(detailPage("80000001", { shown: "80000009" }), "https://accounts.booth.pm/orders/80000001"), null);
  assert.equal(orderDetail(detailPage("80000001"), "https://accounts.booth.pm/orders"), null);
  assert.equal(orderDetail(detailPage("80000001"), "https://booth.pm/orders/80000001"), null);
  assert.equal(orderDetail(parseHTML("<html><body><p>ログイン</p></body></html>").document, "https://accounts.booth.pm/orders/80000001"), null);
});

test("a gift shown only on the detail page is detected without returning the gift URL", () => {
  const gift = `<div class="sheet"><div class="l-row"><div>ギフト用URL</div><div><input value="https://booth.pm/gifts/secret-token-123"></div></div><div>未受け取り</div></div>`;
  const detail = orderDetail(detailPage("80000004", { extra: gift }), "https://accounts.booth.pm/orders/80000004");
  assert.equal(detail.gift, true);
  assert.ok(!JSON.stringify(detail).includes("secret-token-123"));
  const icon = `<div><pixiv-icon name="24/Gift"></pixiv-icon></div>`;
  assert.equal(orderDetail(detailPage("80000005", { extra: icon }), "https://accounts.booth.pm/orders/80000005").gift, true);
  const badge = `<div><span class="badge order-gift"></span></div>`;
  assert.equal(orderDetail(detailPage("80000006", { extra: badge }), "https://accounts.booth.pm/orders/80000006").gift, true);
});

test("the word gift in a product, shop or file name is not a gift order", () => {
  const document = detailPage("80000007", {
    shops: [["ギフトショップ", itemSheet("3000001", "ギフトボックス (gift)", "500", "0")]],
  });
  assert.equal(orderDetail(document, "https://accounts.booth.pm/orders/80000007").gift, false);
});

test("dates, yen and states are parsed strictly", () => {
  assert.equal(parseOrderedAt("注文日時: 2026/1/2 3:04"), "2026-01-02T03:04:00+09:00");
  assert.equal(parseOrderedAt("2026/13/02 09:30:20"), null);
  assert.equal(parseOrderedAt("なし"), null);
  assert.equal(parseYen("¥ 12,345"), 12345);
  assert.equal(parseYen("1,000円"), 1000);
  assert.equal(parseYen("無料"), null);
  assert.equal(orderState(null), "unknown");
});
