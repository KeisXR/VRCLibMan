// VRCLibMan のサービスワーカー。設定・状態の保存、アプリへの送信、ファイル取得の追跡を受け持つ。
// BOOTH のページを読む長い処理は offscreen 文書（offscreen.js）で動かし、ここでは結果だけを扱う。
// Cookie やパスワードは読み出さず、アプリにもページにも渡さない。ペアリングコードはページへのメッセージに載せない。
importScripts("shared.js", "download-route.js", "companion.js");

const shared = globalThis.VrcLibManShared;
const companion = globalThis.VrcLibManCompanion;
const route = globalThis.VrcLibManRoute;
const OFFSCREEN = "offscreen.html";
const EXTENSION_PAGES = ["popup.html", "sidepanel.html", "options.html"];
const SOURCES = { page: "current-page", library: "library", "library-new": "library-update" };

// Serialize storage mutations across events; persist everything needed after worker shutdown.
let serial = Promise.resolve();
function exclusive(run) {
  const next = serial.then(run, run);
  serial = next.catch(() => {});
  return next;
}
function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
function version() {
  return chrome.runtime.getManifest().version;
}
function normalized(token) {
  return shared.normalizeToken(token);
}
function bare(url) {
  return String(url || "").split("#")[0].split("?")[0];
}
async function settings() {
  return chrome.storage.local.get({
    ...shared.DEFAULTS,
    pendingDeliveries: [],
    downloadIntents: [],
    downloadLog: [],
    importHistory: [],
    checkoutSeen: [],
  });
}

// ---- アプリとの通信 ----

let lastSeenWrite = 0;
function noteSeen(status) {
  const now = Date.now();
  if (now - lastSeenWrite < 30000) return;
  lastSeenWrite = now;
  chrome.storage.local.set({ lastConnection: { at: new Date(now).toISOString(), status } }).catch(() => {});
}

// Chrome は、ホスト権限で送る拡張の GET に Origin を付けない。アプリは Origin で拡張を見分けるので、
// 読み取りも POST で送る（本文なし）。
async function appRequest(path, { token, body, timeoutMs } = {}) {
  let response;
  try {
    response = await fetch(`${shared.LOCAL_BASE}${path}`, {
      method: "POST",
      credentials: "omit",
      ...(timeoutMs ? { signal: AbortSignal.timeout(timeoutMs) } : {}),
      headers: shared.localHeaders(token, version(), body !== undefined),
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (error) {
    const failure = new Error("アプリに接続できません。VRCLibMan を起動してください");
    // A timeout may mean the app received the request; only a refused connection is safe to retry.
    failure.unreachable = !["TimeoutError", "AbortError"].includes(error?.name);
    throw failure;
  }
  if (response.ok) noteSeen(response.status);
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    const failure = new Error(data.error || `アプリへ送れませんでした（エラー ${response.status}）`);
    failure.status = response.status;
    throw failure;
  }
  return data;
}

// The last /health body. The app reports there which extension version it placed in its folder.
let lastHealth = null;
async function probe(token, timeoutMs = 1500) {
  try {
    const response = await fetch(`${shared.LOCAL_BASE}/health`, {
      method: "POST",
      credentials: "omit",
      signal: AbortSignal.timeout(timeoutMs),
      headers: shared.localHeaders(token, version(), false),
    });
    if (response.ok) {
      noteSeen(response.status);
      lastHealth = await response.json().catch(() => ({}));
    }
    return response.status;
  } catch {
    return null;
  }
}

// ---- アプリが用意したフォルダの新しい版へ読み込み直す ----

const VERSION_CHECK_MS = 10 * 60 * 1000;

function compareVersions(left, right) {
  const a = String(left).split(".").map(Number);
  const b = String(right).split(".").map(Number);
  for (let index = 0; index < 4; index += 1) {
    const diff = (a[index] || 0) - (b[index] || 0);
    if (diff) return diff;
  }
  return 0;
}

// An unpacked extension reloads from its own folder. If that folder is not the app's, the version stays
// the same after one reload; then we stop and tell the user where to load it from, instead of looping.
async function applyUpdate(latest, { reload }) {
  const own = version();
  if (typeof latest !== "string" || !/^\d+(\.\d+){0,3}$/.test(latest) || compareVersions(latest, own) <= 0) {
    await chrome.storage.local.set({ extensionUpdate: null });
    return { current: true };
  }
  const { reloadedFor } = await chrome.storage.local.get({ reloadedFor: "" });
  if (reloadedFor === latest) {
    await chrome.storage.local.set({ extensionUpdate: { version: latest, stuck: true } });
    return { stuck: true };
  }
  const job = await readJob();
  if (!reload || job?.state === "running") {
    await chrome.storage.local.set({ extensionUpdate: { version: latest, stuck: false } });
    return { pending: true };
  }
  // Only an unpacked (developer mode) extension reads its folder again on reload.
  const self = chrome.management?.getSelf ? await chrome.management.getSelf().catch(() => null) : null;
  if (self && self.installType !== "development") {
    await chrome.storage.local.set({ extensionUpdate: { version: latest, stuck: true } });
    return { stuck: true };
  }
  await chrome.storage.local.set({ reloadedFor: latest, extensionUpdate: { version: latest, stuck: false } });
  chrome.runtime.reload();
  return { reloading: true };
}

async function checkForUpdate({ force = false } = {}) {
  const stored = await chrome.storage.local.get({ pairingToken: "", lastVersionCheck: 0 });
  if (!stored.pairingToken) return null;
  if (!force && Date.now() - stored.lastVersionCheck < VERSION_CHECK_MS) return null;
  await chrome.storage.local.set({ lastVersionCheck: Date.now() });
  if (!reachable(await probe(stored.pairingToken))) return null;
  return applyUpdate(lastHealth?.extension, { reload: true });
}

function reachable(status) {
  return status !== null && status >= 200 && status < 300;
}

function connectError(status) {
  if (status === null) return new Error("アプリに接続できません。VRCLibMan を起動してもう一度お試しください");
  return new Error(shared.connectionState(status, true).message || `アプリとの接続を確認してください（エラー ${status}）`);
}

// 起動しているアプリにはそのまま送る。届かないときだけ vrclibman://import でアプリを起こし、
// 読み取りの /health だけを待つ。送信そのものは繰り返さない。
async function ensureApp(token, { active = false, keepOnFail = false } = {}) {
  let status = await probe(token);
  if (status !== null) return status;
  let wake = null;
  try {
    wake = await chrome.tabs.create({ url: shared.WAKE_URL, active });
  } catch {
    // The scheme may be unregistered; the health checks below still decide.
  }
  const started = Date.now();
  while (status === null && Date.now() - started < shared.WAKE_WAIT_MS) {
    await sleep(500);
    status = await probe(token, 1000);
  }
  // Keep a visible launch tab after a failure so the browser's launch prompt can still be accepted.
  if (wake?.id != null && (status !== null || !keepOnFail)) chrome.tabs.remove(wake.id).catch(() => {});
  return status;
}

// ---- offscreen 文書 ----

let creating = null;
async function hasOffscreen() {
  if (!chrome.runtime.getContexts) return false;
  const contexts = await chrome.runtime.getContexts({
    contextTypes: ["OFFSCREEN_DOCUMENT"],
    documentUrls: [chrome.runtime.getURL(OFFSCREEN)],
  });
  return contexts.length > 0;
}
async function ensureOffscreen() {
  if (await hasOffscreen()) return;
  if (!creating) {
    creating = chrome.offscreen
      .createDocument({
        url: OFFSCREEN,
        reasons: ["DOM_PARSER", "BLOBS"],
        justification: "BOOTH のページを読み取り、保存用の JSON を作るため",
      })
      .catch((error) => {
        if (!/single offscreen/i.test(error?.message || "")) throw error;
      })
      .finally(() => {
        creating = null;
      });
  }
  await creating;
}
async function toOffscreen(message) {
  for (let attempt = 0; ; attempt += 1) {
    try {
      return await chrome.runtime.sendMessage({ target: "offscreen", ...message });
    } catch (error) {
      if (attempt >= 4) throw error;
      await sleep(100);
    }
  }
}
async function offscreenRunning(id) {
  if (!(await hasOffscreen())) return false;
  const answer = await toOffscreen({ type: "status" }).catch(() => null);
  return Boolean(answer?.active) && (!id || answer.active === id);
}
function closeOffscreenLater() {
  setTimeout(async () => {
    const job = await readJob();
    if (job?.state === "running") return;
    if (await hasOffscreen()) chrome.offscreen.closeDocument().catch(() => {});
  }, 5 * 60 * 1000);
}

// ---- 読み込みジョブ（進み具合は storage.session に置き、画面はその変化を見る） ----

async function readJob() {
  return (await chrome.storage.session.get({ job: null })).job;
}
async function writeJob(job) {
  await chrome.storage.session.set({ job });
}
// Progress and results arrive as separate messages; one queue keeps a late progress note from reviving a finished job.
let jobSerial = Promise.resolve();
function jobLock(run) {
  const next = jobSerial.then(run, run);
  jobSerial = next.catch(() => {});
  return next;
}
function updateJob(id, change) {
  return jobLock(async () => {
    const job = await readJob();
    if (!job || job.id !== id) return null;
    const next = change(job);
    if (next) await writeJob(next);
    return next;
  });
}
function finishJob(id, patch) {
  return updateJob(id, (job) => ({ ...job, ...patch, finishedAt: new Date().toISOString() }));
}
async function addHistory(entry) {
  const { importHistory } = await chrome.storage.local.get({ importHistory: [] });
  await chrome.storage.local.set({
    importHistory: [{ at: new Date().toISOString(), ...entry }, ...importHistory].slice(0, 20),
  });
}

async function readTabProducts(tabId) {
  try {
    const response = await chrome.tabs.sendMessage(tabId, { type: "extract-page" });
    if (response?.error) throw new Error(response.error);
    if (Array.isArray(response?.products)) return response.products;
  } catch {
    await chrome.scripting.executeScript({ target: { tabId }, files: ["extract.js"] });
    const [result] = await chrome.scripting.executeScript({
      target: { tabId },
      func: () => globalThis.VrcLibManExtract.extractPage(document, location.href),
    });
    return Array.isArray(result?.result) ? result.result : [];
  }
  return [];
}

function dedupe(products) {
  const seen = new Set();
  return (products || []).filter((product) => {
    const id = String(product?.boothId || "");
    if (!id || seen.has(id)) return false;
    seen.add(id);
    return true;
  });
}

async function startJob(message) {
  const kind = message.kind;
  if (!shared.JOB_LABELS[kind]) throw new Error("この操作には対応していません");
  const current = await readJob();
  if (current?.state === "running" && (await offscreenRunning(current.id))) {
    throw new Error("ほかの読み込みを実行中です。終わるか中止してから、もう一度押してください");
  }
  const stored = await settings();
  const job = { id: crypto.randomUUID(), kind, intervalMs: stored.boothIntervalMs, version: version() };
  const view = {
    id: job.id,
    kind,
    label: shared.JOB_LABELS[kind],
    state: "running",
    line: "準備しています",
    current: 0,
    total: 0,
    count: 0,
    startedAt: new Date().toISOString(),
  };
  await jobLock(() => writeJob(view));
  try {
    if (kind === "page") {
      const tab = await chrome.tabs.get(message.tabId);
      if (!companion.boothUrl(tab.url)) throw new Error("BOOTH の商品ページかライブラリを開いてから実行してください");
      job.products = dedupe(await readTabProducts(tab.id));
      if (job.products.length === 0) throw new Error("このページから商品を見つけられませんでした");
    } else if (kind === "library-new" || kind === "purchases") {
      if (!stored.pairingToken) throw new Error("この操作にはアプリとの接続が必要です。設定でペアリングコードを保存してください");
      const status = await ensureApp(stored.pairingToken);
      if (!reachable(status)) throw connectError(status);
      if (kind === "library-new") {
        const data = await appRequest("/known-ids", { token: stored.pairingToken, timeoutMs: 8000 });
        job.knownIds = (Array.isArray(data.ids) ? data.ids : []).map(String);
      } else {
        const state = await appRequest("/purchases/state", { token: stored.pairingToken, timeoutMs: 8000 });
        job.complete = state.complete === true;
        job.token = stored.pairingToken;
      }
    }
    const latest = await readJob();
    if (!latest || latest.id !== job.id || latest.state !== "running") return { ok: true, id: job.id };
    await ensureOffscreen();
    const answer = await toOffscreen({ type: "walk", job });
    if (answer?.error) throw new Error(answer.error);
    return { ok: true, id: job.id };
  } catch (error) {
    await finishJob(job.id, { state: "error", line: error.message });
    throw error;
  }
}

async function cancelJob() {
  const job = await readJob();
  if (!job || job.state !== "running") return { ok: true };
  if (await hasOffscreen()) {
    const answer = await toOffscreen({ type: "walk-cancel", id: job.id }).catch(() => null);
    if (answer?.ok) {
      await updateJob(job.id, (current) => current.state === "running" ? { ...current, line: "中止しています。読んだところまでは残します" } : null);
      return { ok: true };
    }
  }
  await finishJob(job.id, { state: "cancelled", line: "中止しました" });
  return { ok: true };
}

// The worker can restart while nothing runs in the offscreen document; never show a dead job as running.
async function recoverJob() {
  const job = await readJob();
  if (job?.state !== "running") return;
  if (Date.now() - Date.parse(job.startedAt) < 20000) return;
  if (await offscreenRunning(job.id)) return;
  await finishJob(job.id, { state: "error", line: "読み込みが中断しました。もう一度実行してください" });
}

async function onProgress(message) {
  await updateJob(message.id, (job) => job.state !== "running" ? null : {
    ...job,
    line: message.line ?? job.line,
    current: message.current ?? job.current,
    total: message.total ?? job.total,
    count: message.count ?? job.count,
  });
}

function sentText(report, count) {
  const parts = [`商品情報を ${count} 件送りました。追加 ${report.inserted ?? 0} 件、更新 ${report.updated ?? 0} 件`];
  if (report.skipped) parts.push(`読み込めなかった商品 ${report.skipped} 件`);
  if (Array.isArray(report.warnings)) parts.push(...report.warnings.slice(0, 3));
  return parts.join("。");
}

async function saveJson(payload) {
  await ensureOffscreen();
  const answer = await toOffscreen({ type: "blob-url", json: JSON.stringify(payload, null, 2) });
  if (!answer?.url) throw new Error("JSON を保存できませんでした");
  const stamp = new Date().toISOString().slice(0, 16).replace(/[-:]/g, "").replace("T", "-");
  await chrome.downloads.download({ url: answer.url, filename: `vrclibman-booth-${stamp}.json`, saveAs: true });
  closeOffscreenLater();
}

// 商品情報をアプリへ送る。届かなければアプリを起こして 1 回だけ送り直し、それでも届かなければ JSON を保存する。
async function deliver(products, source, { complete = false } = {}) {
  const stored = await settings();
  const payload = {
    format: "vrclibman.booth-export",
    version: 1,
    exportedAt: new Date().toISOString(),
    source,
    ...(complete ? { complete: true } : {}),
    products,
  };
  if (!stored.pairingToken) {
    await saveJson(payload);
    return { text: "ペアリングコードが未入力なので、JSON を保存しました", saved: true };
  }
  const send = async () => {
    const report = await appRequest("/import", { token: stored.pairingToken, body: payload });
    return { text: sentText(report, products.length), report };
  };
  try {
    return await send();
  } catch (error) {
    if (!error.unreachable) throw error;
  }
  if (reachable(await ensureApp(stored.pairingToken))) {
    try {
      return await send();
    } catch (error) {
      if (!error.unreachable) throw error;
    }
  }
  await saveJson(payload);
  return { text: "アプリに接続できませんでした。JSON を保存したので、アプリのインポート画面へドロップしてください", saved: true };
}

async function onResult(message) {
  const job = await readJob();
  if (!job || job.id !== message.id) return;
  closeOffscreenLater();
  if (message.kind === "purchases") {
    const result = message.purchases || {};
    const base = `購入履歴 ${result.pages || 0} ページ、新しい注文 ${result.added || 0} 件、詳細 ${result.details || 0} 件`;
    const read = Boolean(result.pages || result.details);
    const line = result.cancelled
      ? (read ? `中止しました。${base}` : "中止しました")
      : result.notice
        ? (read ? `${base}。${result.notice}` : result.notice)
        : base;
    if (result.pages || result.details) {
      await addHistory({ kind: "purchases", pages: result.pages || 0, added: result.added || 0, details: result.details || 0, note: result.notice || "" });
    }
    await finishJob(job.id, { state: result.cancelled ? "cancelled" : result.notice ? "error" : "done", line });
    return;
  }
  const products = Array.isArray(message.products) ? message.products : [];
  if (products.length === 0) {
    const empty = job.kind === "page"
      ? "このページから商品を見つけられませんでした"
      : job.kind === "library-new" && message.stopped
        ? "新しい購入はありません"
        : "ライブラリから商品を見つけられませんでした";
    const line = message.cancelled ? "中止しました" : message.error || empty;
    if (!message.cancelled && !message.error) await addHistory({ kind: job.kind, products: 0, pages: message.pages || 0 });
    await finishJob(job.id, { state: message.cancelled ? "cancelled" : message.error ? "error" : "done", line });
    return;
  }
  await updateJob(job.id, (current) => current.state === "running" ? { ...current, line: "アプリへ送っています" } : null);
  try {
    const sent = await deliver(products, SOURCES[job.kind] || "current-page", { complete: message.complete === true });
    const notes = [];
    if (job.kind !== "page" && message.pages) notes.push(`${message.pages} ページを確認`);
    if (job.kind === "library-new" && message.stopped) notes.push("前に取り込んだ商品まで来たので、そこで止めました");
    if (message.cancelled) notes.push("中止したので、読んだところまでを送りました");
    if (message.error) notes.push(message.error);
    const report = sent.report || {};
    await addHistory({
      kind: job.kind,
      products: products.length,
      inserted: report.inserted ?? 0,
      updated: report.updated ?? 0,
      unchanged: report.unchanged ?? 0,
      pages: message.pages || 0,
      complete: message.complete === true && !sent.saved,
      saved: Boolean(sent.saved),
      note: message.error || "",
    });
    await finishJob(job.id, {
      state: message.error ? "error" : message.cancelled ? "cancelled" : "done",
      line: [sent.text, ...notes].join("。"),
    });
  } catch (error) {
    await finishJob(job.id, { state: "error", line: error.message });
  }
}

// ---- ページからの登録（ダウンロードリンク、「LibMan に登録」） ----

async function importSelected(tabId, pick) {
  const tab = await chrome.tabs.get(tabId);
  if (!companion.boothUrl(tab.url)) throw new Error("BOOTHのページを開いてください");
  const stored = await settings();
  if (!stored.pairingToken) throw new Error("拡張の設定でペアリングコードを保存してください");
  const response = await chrome.tabs.sendMessage(tabId, { type: "extract-page" });
  const product = pick(response?.products);
  if (!product) throw new Error("どの商品のファイルか確かめられません。ページを再読み込みしてください");
  // The protocol carries no product data or credentials; the authenticated HTTP request does.
  const status = await ensureApp(stored.pairingToken, { active: true, keepOnFail: true });
  if (!reachable(status)) throw connectError(status);
  // Never replay an import after an ambiguous response. Retry only read-only health checks.
  await appRequest("/import-download", { token: stored.pairingToken, body: product, timeoutMs: 8000 });
  return { ok: true };
}

// ---- 購入完了ページと購入履歴のページ ----

function cleanCheckout(message, sender) {
  const url = companion.boothUrl(message?.href);
  const orderId = String(message?.orderId || "");
  if (!url || url.hostname !== "checkout.booth.pm" || url.pathname !== `/orders/${orderId}` || !/^\d{4,12}$/.test(orderId)) return null;
  if (bare(sender.url) !== bare(message.href)) return null;
  const products = (Array.isArray(message.products) ? message.products : [])
    .filter((product) => /^\d{4,12}$/.test(String(product?.boothId || "")) && String(product?.title || "").trim())
    .slice(0, 50)
    .map((product) => ({
      boothId: String(product.boothId),
      title: String(product.title).slice(0, 1000),
      shop: String(product.shop || "").slice(0, 300),
    }));
  return products.length ? { href: url.href, orderId, products } : null;
}

async function queueCheckout(message, sender) {
  const checkout = cleanCheckout(message, sender);
  if (!checkout) return { ok: false };
  const stored = await settings();
  if (!stored.autoPurchase || !stored.pairingToken) return { ok: true, skipped: true };
  if (stored.checkoutSeen.includes(checkout.orderId)) return { ok: true, skipped: true };
  const id = `checkout-${checkout.orderId}`;
  await chrome.storage.local.set({
    checkoutSeen: [checkout.orderId, ...stored.checkoutSeen].slice(0, 200),
    pendingDeliveries: [...stored.pendingDeliveries.filter((row) => row.id !== id), {
      id,
      kind: "checkout",
      token: normalized(stored.pairingToken),
      checkout,
    }],
  });
  return { ok: true };
}

async function readCheckoutOrder(orderId, token, intervalMs) {
  await ensureOffscreen();
  const answer = await toOffscreen({ type: "order-detail", orderId, intervalMs });
  closeOffscreenLater();
  if (answer?.value) await appRequest("/purchases/detail", { token, body: answer.value, timeoutMs: 8000 });
}

async function ordersPage(message, sender) {
  const stored = await settings();
  if (!stored.autoPurchase || !stored.pairingToken) return { ok: true };
  if (bare(sender.url) !== bare(message.href)) return { ok: false };
  const job = await readJob();
  if (job?.state === "running" && job.kind === "purchases") return { ok: true };
  const body = message.kind === "detail"
    ? { href: message.href, detail: message.detail }
    : { href: message.href, orders: Array.isArray(message.orders) ? message.orders : [] };
  const path = message.kind === "detail" ? "/purchases/detail" : "/purchases/list";
  // Only a page the user opened, while the app is running. Nothing is queued or retried.
  await appRequest(path, { token: stored.pairingToken, body, timeoutMs: 8000 }).catch(() => {});
  return { ok: true };
}

// ---- 取得の追跡と、アプリへの未送信通知 ----

async function markLog(notificationId, state, message) {
  if (!notificationId) return;
  const { downloadLog } = await chrome.storage.local.get({ downloadLog: [] });
  if (!downloadLog.some((row) => row.id === notificationId)) return;
  await chrome.storage.local.set({
    downloadLog: downloadLog.map((row) => row.id === notificationId ? { ...row, state, message, at: new Date().toISOString() } : row),
  });
}

async function flush() {
  const stored = await settings();
  if (!stored.pairingToken) return;
  let failure = "";
  for (const entry of stored.pendingDeliveries) {
    if (entry.token !== normalized(stored.pairingToken)) continue;
    try {
      if (entry.kind === "checkout") {
        const result = await appRequest("/purchases/checkout", { token: stored.pairingToken, body: entry.checkout, timeoutMs: 8000 });
        await addHistory({ kind: "checkout", products: entry.checkout.products.length, inserted: result.registered || 0 });
        if (result.needsDetail) {
          readCheckoutOrder(entry.checkout.orderId, stored.pairingToken, stored.boothIntervalMs).catch(() => {});
        }
      } else {
        if (entry.product) {
          await appRequest("/import", { token: stored.pairingToken, body: { format: "vrclibman.booth-export", version: 1, products: [entry.product] }, timeoutMs: 8000 });
        }
        await appRequest("/download-finished", { token: stored.pairingToken, body: entry.notice, timeoutMs: 8000 });
        await markLog(entry.notice?.notificationId, "done", "ライブラリに移しました");
      }
      const current = await settings();
      await chrome.storage.local.set({ pendingDeliveries: current.pendingDeliveries.filter((row) => row.id !== entry.id) });
    } catch (error) {
      failure ||= error.message;
      if (entry.kind !== "checkout") await markLog(entry.notice?.notificationId, "pending", error.message);
    }
  }
  await chrome.storage.local.set({ deliveryError: failure });
}

async function queueNotice(notice, intent, downloadId) {
  const stored = await settings();
  const profile = await chrome.storage.local.get({ deliveryProfile: "" });
  const namespace = profile.deliveryProfile || crypto.randomUUID();
  if (!profile.deliveryProfile) await chrome.storage.local.set({ deliveryProfile: namespace });
  const id = `${namespace}-${downloadId}`;
  if (!stored.pendingDeliveries.some((row) => row.id === id)) {
    await chrome.storage.local.set({
      pendingDeliveries: [...stored.pendingDeliveries, {
        id,
        token: intent?.token || normalized(stored.pairingToken),
        product: intent?.product,
        notice: { ...notice, variation: intent?.file.variation || "", ...(intent ? { downloadableId: intent.file.downloadableId } : {}), notificationId: id },
      }],
    });
  }
  if (!stored.downloadLog.some((row) => row.id === id)) {
    await chrome.storage.local.set({
      downloadLog: [{
        id,
        filename: notice.filename,
        variation: intent?.file.variation || "",
        boothId: notice.boothId,
        state: "pending",
        message: "アプリへ知らせています",
        at: new Date().toISOString(),
      }, ...stored.downloadLog].slice(0, 30),
    });
  }
  // Remove an intent only once its completed notice is durable.
  if (intent) await chrome.storage.local.set({ downloadIntents: stored.downloadIntents.filter((row) => row.id !== intent.id) });
}

chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  let settled = false;
  const finish = (decision) => {
    if (!settled) {
      settled = true;
      decision ? suggest(decision) : suggest();
    }
  };
  settings().then((stored) => {
    const id = companion.downloadId(item.url);
    const intents = stored.downloadIntents.filter((row) => row.file.downloadableId === id && (row.downloadId == null || row.downloadId === item.id));
    if (intents.length === 1) {
      const intent = intents[0];
      finish({ filename: `VRCLibMan/${intent.product.boothId}/${intent.file.filename}`, conflictAction: "uniquify" });
      exclusive(async () => {
        const current = await settings();
        await chrome.storage.local.set({ downloadIntents: current.downloadIntents.map((row) => row.id === intent.id ? { ...row, downloadId: item.id } : row) });
      }).catch(() => {});
    } else finish(route.decideFilename(item, stored.routeDownloads));
  }).catch(() => finish(null));
  return true;
});

async function handleDownload(item) {
  if (item?.state === "interrupted") {
    const stored = await settings();
    if (stored.downloadIntents.some((row) => row.downloadId === item.id)) {
      await chrome.storage.local.set({
        downloadIntents: item.canResume ? stored.downloadIntents : stored.downloadIntents.filter((row) => row.downloadId !== item.id),
        deliveryError: item.canResume ? "取得が中断しました。ブラウザのダウンロード画面で再開してください。" : "取得が中止されました。必要ならファイルを選び直してください。",
      });
    }
    return;
  }
  if (!item || item.state !== "complete") return;
  const stored = await settings();
  const intent = stored.downloadIntents.find((row) => row.downloadId === item.id);
  let notice = route.completionNotice(item);
  if (!notice || (!intent && (!stored.routeDownloads || !stored.pairingToken))) return;
  // Staged product ID must agree with the exact clicked file's product.
  if (intent && intent.product.boothId !== notice.boothId) return;
  if (/^(text\/html|application\/xhtml\+xml)(?:;|$)/i.test(item.mime || "")) {
    await chrome.storage.local.set({
      deliveryError: "ファイルの代わりにログイン画面などが届きました。BOOTH にログインしているか確認してください。",
      downloadIntents: stored.downloadIntents.filter((row) => row.id !== intent?.id),
    });
    return;
  }
  if (!intent) {
    const session = await chrome.storage.session.get({ pageNotes: [] });
    const note = session.pageNotes.find((row) => row.boothId === notice.boothId);
    if (note) notice = { ...notice, title: note.title || "", ...(note.isFree ? { isFree: true } : {}), ...(note.isAdult ? { isAdult: true } : {}) };
  }
  await queueNotice({ ...notice, startedAt: item.startTime || intent?.startedAt || "" }, intent, item.id);
  await flush();
}

chrome.downloads.onChanged.addListener((delta) => {
  if (["complete", "interrupted"].includes(delta.state?.current)) {
    exclusive(async () => {
      const [item] = await chrome.downloads.search({ id: delta.id });
      await handleDownload(item);
    }).catch(() => {});
  }
});

async function reconcile() {
  const stored = await settings();
  for (const intent of stored.downloadIntents) {
    const items = intent.downloadId == null
      ? await chrome.downloads.search({ url: intent.url, startedAfter: intent.startedAt, limit: 10 })
      : await chrome.downloads.search({ id: intent.downloadId });
    // Never infer a mapping when several downloads match a recovered intent.
    if (items.length === 1) {
      if (intent.downloadId == null) {
        const current = await settings();
        await chrome.storage.local.set({ downloadIntents: current.downloadIntents.map((row) => row.id === intent.id ? { ...row, downloadId: items[0].id } : row) });
      }
      await handleDownload(items[0]);
    }
  }
  await flush();
}

function eligibleFile(selected) {
  return selected && /^\d{4,12}$/.test(selected.product.boothId)
    && route.safeDownloadName(selected.file.filename) === selected.file.filename
    && companion.isAsset(selected.file.filename);
}

// Reachable only from an explicit file button: our popup, or a trusted click on our button in the page.
async function startDownload(tabId, downloadableId) {
  const tab = await chrome.tabs.get(tabId);
  if (!companion.boothUrl(tab.url)) throw new Error("BOOTHの商品ページかライブラリを開いてください");
  const response = await chrome.tabs.sendMessage(tab.id, { type: "extract-page" });
  const selected = companion.choose(response?.products, downloadableId);
  if (!selected) throw new Error("どのファイルか確かめられません。ページを再読み込みしてください");
  if (!eligibleFile(selected)) throw new Error("このファイルはここからは取得できません");
  const { product, file } = selected;
  const stored = await settings();
  if (!stored.pairingToken || !stored.routeDownloads) throw new Error("アプリとの接続と「ダウンロードを自動整理」を設定してください");
  if (stored.downloadIntents.some((row) => row.file.downloadableId === file.downloadableId)) {
    throw new Error("このファイルはすでに取得待ちです。ブラウザのダウンロード一覧を確認してください");
  }
  const url = `https://booth.pm/downloadables/${file.downloadableId}`;
  const intent = { id: crypto.randomUUID(), token: normalized(stored.pairingToken), product, file, url, startedAt: new Date().toISOString() };
  await chrome.storage.local.set({ downloadIntents: [...stored.downloadIntents, intent] });
  let id;
  try {
    id = await chrome.downloads.download({ url, filename: `VRCLibMan/${product.boothId}/${file.filename}`, conflictAction: "uniquify" });
  } catch {
    await chrome.storage.local.set({ downloadIntents: stored.downloadIntents });
    throw new Error("取得を開始できません。BOOTHへのログイン状態を確認してください");
  }
  const current = await settings();
  await chrome.storage.local.set({ downloadIntents: current.downloadIntents.map((row) => row.id === intent.id ? { ...row, downloadId: id } : row) });
  return { ok: true };
}

async function rememberDownload(tabId, downloadableId) {
  const stored = await settings();
  if (!stored.routeDownloads || !stored.pairingToken) return { ok: true };
  const result = await chrome.tabs.sendMessage(tabId, { type: "extract-page" });
  const selected = companion.choose(result?.products, downloadableId);
  if (!eligibleFile(selected)) return { ok: true };
  if (stored.downloadIntents.some((row) => row.file.downloadableId === downloadableId)) return { ok: true };
  const intent = { id: crypto.randomUUID(), token: normalized(stored.pairingToken), ...selected, url: `https://booth.pm/downloadables/${downloadableId}`, startedAt: new Date().toISOString() };
  await chrome.storage.local.set({ downloadIntents: [...stored.downloadIntents, intent] });
  return { ok: true };
}

async function rememberNotes(notes) {
  const clean = (Array.isArray(notes) ? notes : [])
    .filter((note) => /^\d{4,12}$/.test(String(note?.boothId || "")))
    .slice(0, 40)
    .map((note) => ({
      boothId: String(note.boothId),
      isFree: note.isFree === true,
      isAdult: note.isAdult === true,
      title: String(note.title || "").slice(0, 300),
    }));
  if (clean.length === 0) return;
  const { pageNotes } = await chrome.storage.session.get({ pageNotes: [] });
  const map = new Map(pageNotes.map((row) => [row.boothId, row]));
  for (const note of clean) {
    map.delete(note.boothId);
    map.set(note.boothId, note);
  }
  await chrome.storage.session.set({ pageNotes: [...map.values()].slice(-40) });
}

// ---- 設定画面とポップアップ ----

async function connection() {
  const stored = await settings();
  recoverJob().catch(() => {});
  if (!stored.pairingToken) return { hasToken: false, status: null, routeDownloads: stored.routeDownloads };
  const status = await probe(stored.pairingToken);
  let routeDownloads = stored.routeDownloads;
  if (reachable(status)) {
    try {
      const data = await appRequest("/route-downloads/state", { token: stored.pairingToken, timeoutMs: 4000 });
      routeDownloads = data.enabled === true;
      await chrome.storage.local.set({ routeDownloads });
    } catch {
      // アプリの設定が読めないときは、拡張に保存した値のままにする。
    }
  }
  await chrome.storage.local.set({ lastConnection: { at: new Date().toISOString(), status } });
  if (reachable(status)) await applyUpdate(lastHealth?.extension, { reload: false });
  return { hasToken: true, status, routeDownloads };
}

async function setRoute(enabled) {
  await chrome.storage.local.set({ routeDownloads: enabled });
  const stored = await settings();
  if (!stored.pairingToken) return { ok: true, enabled };
  try {
    const data = await appRequest("/route-downloads", { token: stored.pairingToken, body: { enabled }, timeoutMs: 4000 });
    return { ok: true, enabled: data.enabled === true ? true : enabled };
  } catch (error) {
    if (error.unreachable || error.status === undefined) {
      return { ok: true, enabled, note: "アプリに接続できないため、拡張機能側に設定を保存しました" };
    }
    await chrome.storage.local.set({ routeDownloads: false });
    return { ok: false, enabled: false, error: error.message || "自動整理の設定をアプリへ保存できませんでした" };
  }
}

async function savePageJson(tabId) {
  const tab = await chrome.tabs.get(tabId);
  if (!companion.boothUrl(tab.url)) throw new Error("BOOTH の商品ページかライブラリを開いてから実行してください");
  const products = dedupe(await readTabProducts(tab.id));
  if (products.length === 0) throw new Error("このページから商品を見つけられませんでした");
  await saveJson({ format: "vrclibman.booth-export", version: 1, exportedAt: new Date().toISOString(), source: "current-page", products });
  return { ok: true, count: products.length };
}

async function productStatus(ids) {
  const stored = await settings();
  if (!stored.pairingToken) throw new Error("アプリとの接続を設定してください");
  return appRequest("/product-status", { token: stored.pairingToken, body: { ids }, timeoutMs: 8000 });
}

async function pageState() {
  const stored = await settings();
  return {
    paired: Boolean(stored.pairingToken),
    routeDownloads: stored.routeDownloads === true,
    showBadges: stored.showBadges !== false,
    autoPurchase: stored.autoPurchase !== false,
  };
}

// ---- メッセージの受け口。送り元ごとに許す操作を分ける ----

function fromExtensionPage(sender) {
  return sender.id === chrome.runtime.id && EXTENSION_PAGES.some((page) => bare(sender.url) === chrome.runtime.getURL(page));
}
function fromOffscreen(sender) {
  return sender.id === chrome.runtime.id && bare(sender.url) === chrome.runtime.getURL(OFFSCREEN);
}
function fromBooth(sender) {
  return sender.id === chrome.runtime.id && sender.tab?.id != null && Boolean(companion.boothUrl(sender.url));
}

function answer(promise, respond, fallback) {
  promise.then(respond, (error) => respond({ error: fallback || error?.message || String(error) }));
  return true;
}

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (!message || message.target === "offscreen") return undefined;
  if (fromOffscreen(sender)) {
    if (message.type === "walk-progress") return answer(onProgress(message), respond);
    if (message.type === "walk-result") return answer(onResult(message), respond);
    return undefined;
  }
  if (fromBooth(sender)) {
    const tabId = sender.tab.id;
    switch (message.type) {
      case "page-notes":
        return answer(exclusive(() => rememberNotes(message.notes)), respond);
      case "page-state":
        return answer(pageState(), respond);
      case "product-status":
        // Tokens and app responses never pass through page messages.
        return answer(exclusive(() => productStatus(message.ids)), respond, "アプリに接続できません");
      case "remember-download":
        return answer(exclusive(() => rememberDownload(tabId, message.downloadableId)), respond, "どのファイルを取得したかを記録できません");
      case "import-download":
        return answer(importSelected(tabId, (products) => companion.choose(products, message.downloadableId)?.product), respond);
      case "import-product":
        return answer(importSelected(tabId, (products) => companion.productById(products, String(message.boothId || ""))), respond);
      case "start-download":
        return answer(exclusive(() => startDownload(tabId, message.downloadableId)), respond);
      case "checkout":
        return answer(exclusive(async () => {
          const result = await queueCheckout(message, sender);
          await flush();
          return result;
        }), respond);
      case "orders-page":
        return answer(ordersPage(message, sender), respond);
      default:
        return undefined;
    }
  }
  if (fromExtensionPage(sender)) {
    switch (message.type) {
      case "connection":
        return answer(connection(), respond);
      case "job-start":
        return answer(startJob(message), respond);
      case "job-cancel":
        return answer(cancelJob(), respond);
      case "start-download":
        return answer(exclusive(() => startDownload(message.tabId, message.downloadableId)), respond);
      case "retry-deliveries":
        return answer(exclusive(reconcile), respond, "送り直せませんでした");
      case "set-route":
        return answer(setRoute(message.enabled === true), respond);
      case "save-page-json":
        return answer(savePageJson(message.tabId), respond);
      case "reload-extension":
        return answer((async () => {
          const job = await readJob();
          if (job?.state === "running") throw new Error("読み込みが終わってから、もう一度押してください");
          const stored = await chrome.storage.local.get({ extensionUpdate: null });
          if (!stored.extensionUpdate?.version) return { ok: true };
          return applyUpdate(stored.extensionUpdate.version, { reload: true });
        })(), respond);
      case "product-status":
        return answer(exclusive(() => productStatus(message.ids)), respond, "アプリに接続できません");
      default:
        return undefined;
    }
  }
  return undefined;
});

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name !== "retry-deliveries") return;
  // Finish pending deliveries first, so a reload never cuts one off midway.
  exclusive(reconcile).catch(() => {}).finally(() => checkForUpdate().catch(() => {}));
});
function initialize() {
  chrome.alarms.create("retry-deliveries", { periodInMinutes: 1 });
  exclusive(reconcile).catch(() => {}).finally(() => checkForUpdate({ force: true }).catch(() => {}));
  recoverJob().catch(() => {});
}
chrome.runtime.onStartup.addListener(initialize);
chrome.runtime.onInstalled.addListener(initialize);
