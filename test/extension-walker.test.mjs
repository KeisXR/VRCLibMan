import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";
import { DOMParser } from "linkedom";

const require = createRequire(import.meta.url);
const { createWalker, LOGIN_NOTICE, DETAIL_UNREADABLE } = require("../extension/walker.js");
const realExtract = require("../extension/extract.js");
const policy = require("../extension/request-policy.js");
const shared = require("../extension/shared.js");

const LIBRARY = "https://accounts.booth.pm/library";
const GIFTS = "https://accounts.booth.pm/library/gifts";
const FREE = "https://accounts.booth.pm/library/free_downloads";
const product = (boothId, description = "説明".repeat(150)) => ({ boothId, title: `架空の服 ${boothId}`, url: `https://booth.pm/ja/items/${boothId}`, shop: "架空の工房", descriptionSnippet: description });

// BOOTH の応答の見本。pages[url] = { products, lastPage, status, landed, retryAfter }
function harness({ pages = {}, items = {}, orders = {}, app = {}, cancelAfter = Infinity } = {}) {
  const requests = [];
  const appCalls = [];
  const progress = [];
  const sleeps = [];
  const served = new Map();
  let clock = 0;
  let reads = 0;
  const response = (url, entry) => ({
    ok: (entry.status ?? 200) === 200,
    status: entry.status ?? 200,
    url: entry.landed ?? url,
    headers: { get: (name) => (name.toLowerCase() === "retry-after" ? entry.retryAfter ?? null : null) },
    text: async () => `<html><body data-url="${url}">${entry.lastPage ? `<a href="?page=${entry.lastPage}">last</a>` : ""}</body></html>`,
    json: async () => entry.json ?? {},
  });
  const extract = {
    ...realExtract,
    extractPage: (doc, href) => structuredClone(served.get(href)?.products || []),
  };
  const purchases = {
    orderList: (doc, href) => structuredClone(orders[href] || { orders: [], lastPage: 1 }),
    orderDetail: (doc, href) => structuredClone(orders[href] || null),
  };
  const walker = createWalker({
    fetch: async (url, options) => {
      requests.push({ url, credentials: options.credentials });
      reads += 1;
      const queue = pages[url] || items[url] || orders[url] || { status: 404 };
      const entry = Array.isArray(queue) ? queue.shift() : queue;
      served.set(entry.landed ?? url, entry);
      return response(url, entry);
    },
    DOMParser,
    extract,
    purchases,
    policy,
    shared,
    intervalMs: 1000,
    sleep: async (ms) => { sleeps.push(ms); clock += ms; },
    now: () => clock,
    isCancelled: () => reads >= cancelAfter,
    progress: (info) => progress.push(info),
    app: async (path, body) => {
      appCalls.push({ path, body });
      const reply = app[path];
      if (reply instanceof Error) throw reply;
      return typeof reply === "function" ? reply(body) : reply || {};
    },
  });
  return { walker, requests, appCalls, progress, sleeps };
}

test("a full library walk reads every section page by page with the login and marks the list complete", async () => {
  const h = harness({
    pages: {
      [`${LIBRARY}?page=1`]: { products: [product("9001"), product("9002", "短い")], lastPage: 2 },
      [`${LIBRARY}?page=2`]: { products: [product("9003")] },
      [`${GIFTS}?page=1`]: { products: [] },
      [`${FREE}?page=1`]: { products: [product("9001"), product("9004")] },
    },
    items: {
      "https://booth.pm/ja/items/9002": { status: 403 },
    },
  });
  const walked = await h.walker.walkLibrary();
  assert.equal(walked.complete, true);
  assert.equal(walked.error, undefined);
  assert.equal(walked.pages, 4);
  assert.deepEqual(walked.products.map((row) => row.boothId), ["9001", "9002", "9003", "9004"]);
  assert.deepEqual(h.requests.map((row) => row.url), [
    `${LIBRARY}?page=1`, `${LIBRARY}?page=2`, `${GIFTS}?page=1`, `${FREE}?page=1`,
  ]);
  assert.ok(h.requests.every((row) => row.credentials === "include"));
  assert.deepEqual(h.sleeps, [1000, 1000, 1000], "one request per interval, never in parallel");
  assert.equal(h.progress[0].line, "購入ライブラリ 1 ページ目");
  assert.ok(h.progress.some((row) => row.line === "購入ライブラリ 2 / 2 ページ" && row.total === 2));

  await h.walker.enrich(walked.products);
  const detailRequests = h.requests.slice(4);
  assert.deepEqual(detailRequests, [
    { url: "https://booth.pm/ja/items/9001.json", credentials: "omit" },
    { url: "https://booth.pm/ja/items/9002", credentials: "omit" },
    { url: "https://booth.pm/ja/items/9003.json", credentials: "omit" },
    { url: "https://booth.pm/ja/items/9004.json", credentials: "omit" },
  ], "a long description skips the page but still reads the category; a 403 page is not retried as JSON");
});

test("the category comes from the public JSON, because the page leaves its category and tag lists empty", async () => {
  const json = { id: 9001, name: "架空の服 9001", category: { id: 208, name: "3D衣装", parent: { name: "3Dモデル" } }, tags: [{ name: "VRChat" }], description: "導入" };
  const h = harness({
    items: {
      "https://booth.pm/ja/items/9001": { products: [] },
      "https://booth.pm/ja/items/9001.json": { json },
      "https://booth.pm/ja/items/9002.json": { json: { ...json, id: 9002, is_adult: true } },
    },
  });
  const short = { ...product("9001", "短い"), shop: "" };
  const known = { ...product("9003"), category: { name: "3D衣装" } };
  const adult = { ...product("9004"), isAdult: true };
  const adultByJson = product("9002");
  const products = [short, known, adult, adultByJson];
  await h.walker.enrich(products);
  assert.deepEqual(h.requests.map((row) => row.url), [
    "https://booth.pm/ja/items/9001",
    "https://booth.pm/ja/items/9001.json",
    "https://booth.pm/ja/items/9002.json",
  ], "a product that already has a category and an age-restricted product are not read again");
  assert.ok(h.requests.every((row) => row.credentials === "omit"));
  assert.deepEqual(short.category, { id: 208, name: "3D衣装", parent: { name: "3Dモデル" } });
  assert.deepEqual(short.tags, ["VRChat"]);
  assert.equal(adultByJson.category, undefined, "an age-restricted answer keeps no category");
  assert.equal(adultByJson.descriptionSnippet, "", "nor its description");
  assert.deepEqual(h.sleeps, [1000, 1000], "the JSON reads keep the same interval");
});

test("new purchases stop each list at the first imported product and are never a complete list", async () => {
  const h = harness({
    pages: {
      [`${LIBRARY}?page=1`]: { products: [product("9005"), product("9001"), product("9000")], lastPage: 9 },
      [`${GIFTS}?page=1`]: { products: [product("9006")] },
      [`${FREE}?page=1`]: { products: [product("9002")] },
    },
  });
  const walked = await h.walker.walkLibrary({ knownIds: new Set(["9001", "9002"]) });
  assert.deepEqual(walked.products.map((row) => row.boothId), ["9005", "9006"]);
  assert.equal(walked.stopped, true);
  assert.equal(walked.complete, false);
  assert.equal(h.requests.length, 3, "later pages of a stopped list are not read");
});

test("a redirect to the sign-in page stops the walk and keeps what was read", async () => {
  const h = harness({
    pages: {
      [`${LIBRARY}?page=1`]: { products: [product("9001")], lastPage: 3 },
      [`${LIBRARY}?page=2`]: { landed: "https://accounts.booth.pm/users/sign_in" },
    },
  });
  const walked = await h.walker.walkLibrary();
  assert.equal(walked.error, LOGIN_NOTICE);
  assert.equal(walked.complete, false);
  assert.deepEqual(walked.products.map((row) => row.boothId), ["9001"]);
  assert.equal(h.requests.length, 2);
});

test("429 waits for Retry-After once, and a second refusal stops with what was read", async () => {
  const h = harness({
    pages: {
      [`${LIBRARY}?page=1`]: [{ status: 429, retryAfter: "2" }, { products: [product("9001")], lastPage: 2 }],
      [`${LIBRARY}?page=2`]: [{ status: 503 }, { status: 429, retryAfter: "1" }],
    },
  });
  const walked = await h.walker.walkLibrary();
  assert.match(walked.error, /アクセス制限が続いている/);
  assert.deepEqual(walked.products.map((row) => row.boothId), ["9001"]);
  assert.ok(h.sleeps.reduce((sum, ms) => sum + ms, 0) >= 2000, "the requested wait is honoured");
  assert.ok(h.progress.some((row) => /2 秒待って一度だけ再試行/.test(row.line)));
  const long = harness({ pages: { [`${LIBRARY}?page=1`]: { status: 429, retryAfter: "120" } } });
  assert.match((await long.walker.walkLibrary()).error, /長い待機時間/);
  assert.equal(long.requests.length, 1);
});

test("cancel stops before the next request and returns the products already read", async () => {
  const h = harness({
    cancelAfter: 1,
    pages: {
      [`${LIBRARY}?page=1`]: { products: [product("9001")], lastPage: 5 },
    },
  });
  const walked = await h.walker.walkLibrary();
  assert.equal(walked.cancelled, true);
  assert.equal(walked.error, "");
  assert.deepEqual(walked.products.map((row) => row.boothId), ["9001"]);
  assert.equal(h.requests.length, 1);
});

const ORDERS = "https://accounts.booth.pm/orders";
const order = (orderId) => ({ orderId, state: "confirmed", orderedAt: "2026-11-02T09:30:20+09:00", title: "見本" });
const detail = (orderId) => ({ orderId, state: "confirmed", items: [{ boothId: "123456", name: "見本" }] });

test("the purchase history stops at a page with nothing new once it was read to the end, then reads missing details", async () => {
  const h = harness({
    orders: {
      [ORDERS]: { orders: [order("90000003"), order("90000002")], lastPage: 5 },
      [`${ORDERS}?page=2`]: { orders: [order("90000001")] },
      [`${ORDERS}/90000003`]: detail("90000003"),
    },
    app: {
      "/purchases/list": (body) => ({ added: body.orders.length === 2 ? 1 : 0, removed: 0, news: body.orders.length === 2 }),
      "/purchases/pending": { ids: ["90000003"] },
      "/purchases/detail": { removed: false },
      "/purchases/finish": { ok: true },
    },
  });
  const result = await h.walker.walkPurchases({ complete: true });
  assert.equal(result.notice, "");
  assert.equal(result.pages, 2);
  assert.equal(result.added, 1);
  assert.equal(result.details, 1);
  assert.equal(result.listComplete, true);
  assert.deepEqual(h.requests.map((row) => row.url), [ORDERS, `${ORDERS}?page=2`, `${ORDERS}/90000003`]);
  assert.ok(h.requests.every((row) => row.credentials === "include"));
  assert.deepEqual(h.appCalls.map((row) => row.path), ["/purchases/list", "/purchases/list", "/purchases/pending", "/purchases/detail", "/purchases/finish"]);
  assert.equal(h.appCalls[0].body.href, ORDERS);
  assert.equal(h.appCalls[3].body.href, `${ORDERS}/90000003`);
  assert.deepEqual(h.appCalls.at(-1).body, { listComplete: true });
  assert.ok(h.progress.some((row) => row.line === "注文の詳細 1 / 1"));
});

test("the first purchase history read goes to the last page even without new orders", async () => {
  const h = harness({
    orders: {
      [ORDERS]: { orders: [order("90000002")], lastPage: 2 },
      [`${ORDERS}?page=2`]: { orders: [order("90000001")] },
    },
    app: { "/purchases/list": { added: 0, news: false }, "/purchases/pending": { ids: [] } },
  });
  const result = await h.walker.walkPurchases({ complete: false });
  assert.equal(result.pages, 2);
  assert.equal(result.listComplete, true);
});

test("a login page, a mismatched detail or a stopped app ends the purchase history with a reason", async () => {
  const login = harness({ orders: { [ORDERS]: { landed: "https://accounts.booth.pm/users/sign_in" } } });
  const stopped = await login.walker.walkPurchases();
  assert.equal(stopped.notice, LOGIN_NOTICE);
  assert.equal(stopped.pages, 0);
  assert.equal(login.appCalls.length, 0, "nothing read, nothing finished");

  const mismatch = harness({
    orders: { [ORDERS]: { orders: [order("90000003")] }, [`${ORDERS}/90000003`]: detail("90000009") },
    app: { "/purchases/list": { added: 1, news: true }, "/purchases/pending": { ids: ["90000003"] } },
  });
  const unreadable = await mismatch.walker.walkPurchases();
  assert.equal(unreadable.notice, DETAIL_UNREADABLE);
  assert.equal(unreadable.listComplete, true);
  assert.deepEqual(mismatch.appCalls.at(-1), { path: "/purchases/finish", body: { listComplete: true } });

  const offline = harness({
    orders: { [ORDERS]: { orders: [order("90000003")], lastPage: 2 } },
    app: { "/purchases/list": new Error("アプリに接続できなくなったため、ここまでで止めました") },
  });
  assert.match((await offline.walker.walkPurchases()).notice, /アプリに接続できなくなった/);
});

test("the order of a checkout is read once and only when the page matches", async () => {
  const h = harness({ orders: { [`${ORDERS}/90000003`]: detail("90000003") } });
  assert.deepEqual(await h.walker.readOrderDetail("90000003"), { href: `${ORDERS}/90000003`, detail: detail("90000003") });
  assert.equal(await h.walker.readOrderDetail("../1"), null);
  assert.equal(h.requests.length, 1);
  assert.deepEqual(h.sleeps, []);
});
