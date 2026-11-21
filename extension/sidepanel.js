// サイドパネル。「ライブラリ」は開いているページのカードと、アバターとジャンルで並べた登録済みの商品。
// 「記録」は取得したファイル、アプリへの未送信、最近の取り込み。読み込みの進み具合はどちらにも出す。
const shared = globalThis.VrcLibManShared;
const companion = globalThis.VrcLibManCompanion;
const ui = globalThis.VrcLibManUi;
const cards = globalThis.VrcLibManPageCards;
const { h } = ui;
const cardSlot = document.querySelector("#cards");
const librarySlot = document.querySelector("#panel-library");
const logSlot = document.querySelector("#panel-log");
const linkChip = document.querySelector("#link-chip");
const viewButtons = { library: document.querySelector("#view-library"), log: document.querySelector("#view-log") };
document.querySelector("#open-settings").append(ui.icon("settings"));
document.querySelector("#open-settings").addEventListener("click", () => chrome.runtime.openOptionsPage());

const ROUTE_HINT = "設定の「ダウンロードを自動整理」をオンにすると、ここから取得できます。";
const LIBRARY_STALE_MS = 60 * 1000;

const state = {
  stored: { ...shared.DEFAULTS, pendingDeliveries: [], downloadIntents: [], downloadLog: [], importHistory: [], deliveryError: "", panelScope: "" },
  job: null,
  connection: undefined,
  progress: new Map(),
  note: "",
  view: "library",
  // 開いているページ。ポップアップと同じカード（pagecards.js）に渡す。
  tab: null,
  kind: "other",
  pageKey: "",
  products: null,
  pageStale: false,
  statuses: new Map(),
  detail: null,
  busy: false,
  message: "",
  tone: "",
};
let poller = null;
let pageGeneration = 0;
let libraryLoadedAt = 0;
let libraryTimer = null;

function send(message) {
  return chrome.runtime.sendMessage(message).catch((error) => ({ error: error.message || String(error) }));
}

function link() {
  return shared.connectionState(state.connection === undefined ? undefined : state.connection?.status ?? null, Boolean(state.stored.pairingToken));
}

function running() {
  return state.job?.state === "running";
}

function say(text, tone = "") {
  state.message = text || "";
  state.tone = tone;
  render();
}

// 一覧の商品を開く。BOOTH を見ているタブはそのタブで移り、ほかのページを見ているときは新しいタブで開く。
// Ctrl / ⌘ / Shift / 中クリックは、今のタブを動かさずに後ろのタブで開く。
function openItem(url, event) {
  if (!url) return;
  const aside = event.ctrlKey || event.metaKey || event.shiftKey || event.button === 1;
  if (!aside && state.tab?.id != null && shared.boothUrl(state.tab.url)) chrome.tabs.update(state.tab.id, { url });
  else chrome.tabs.create({ url, active: !aside });
}

const library = globalThis.VrcLibManLibrary.create({
  send,
  getToken: () => state.stored.pairingToken,
  openItem,
  onScope: (scope) => chrome.storage.local.set({ panelScope: scope }),
  getSavedScope: () => state.stored.panelScope,
});
librarySlot.append(library.node);

function refreshLibrary() {
  libraryLoadedAt = Date.now();
  return library.refresh();
}

// 取り込みやダウンロードの記録が変わったら、少し待ってから一覧と印を読み直す。
function libraryChanged() {
  clearTimeout(libraryTimer);
  libraryTimer = setTimeout(() => {
    refreshLibrary();
    refreshStatuses();
  }, 600);
}

async function startJob(kind) {
  if (state.busy) return;
  state.busy = true;
  say("");
  const result = await send({ type: "job-start", kind, tabId: state.tab?.id });
  state.busy = false;
  if (result?.error) say(result.error, "warn");
  else render();
}

const act = {
  startJob,
  send,
  say,
  openUrl: (url) => chrome.tabs.create({ url }),
  reloadPage: () => chrome.tabs.reload(state.tab.id),
  message: () => (state.message ? h("p", { class: `note ${state.tone}`, role: "status", text: state.message }) : null),
};

// ---- 開いているページ ----

// 見ている商品の購入の記録と、購入した対応版。金額は出さない。
function purchaseLines() {
  const detail = state.detail;
  const product = state.products?.[0];
  if (!detail || !product || detail.boothId !== String(product.boothId)) return null;
  const lines = [];
  const order = detail.orders?.[0];
  if (order) {
    const text = [
      shared.formatDate(order.orderedAt) || "日付なし",
      order.gift ? "ギフトとして贈った" : "",
      detail.orderCount > 1 ? `ほか ${detail.orderCount - 1} 件` : "",
    ].filter(Boolean).join("・");
    const url = shared.orderUrl(order.orderId);
    lines.push(h("div", { class: "row note num" },
      h("span", { class: "grow", text: `購入：${text}` }),
      url ? h("button", { class: "btn link", type: "button", text: "注文を開く", on: { click: () => act.openUrl(url) } }) : null,
    ));
  } else if (!detail.historyRead && state.statuses.get(String(product.boothId))?.registered) {
    lines.push(h("p", { class: "note", text: "購入履歴を読み込むと、ここに購入日が出ます。" }));
  }
  const edition = detail.edition;
  if (edition?.mode === "unknown") lines.push(h("p", { class: "note", text: "購入した対応版：未設定（アプリの「分類を編集」で選べます）" }));
  else if (edition?.avatars?.length) lines.push(h("p", { class: "note", text: `購入した対応版：${edition.avatars.join("、")}` }));
  return lines.length ? h("div", { class: "buy" }, lines) : null;
}

function pageCountLine() {
  const ids = [...new Set((state.products || []).map((product) => String(product.boothId)))];
  if (ids.length === 0 || state.statuses.size === 0) return null;
  const missing = ids.filter((id) => state.statuses.has(id) && !state.statuses.get(id).registered).length;
  return h("p", { class: "note num", text: `このページの商品 ${ids.length} 件・アプリに未登録 ${missing} 件` });
}

function pageCards() {
  const view = { ...state, busy: state.busy || running() };
  if (state.kind === "item") return [cards.itemCard(view, act, purchaseLines()), cards.filesCard(view, act, ROUTE_HINT)];
  if (state.kind === "library") return [cards.libraryCard(view, act, pageCountLine())];
  if (state.kind === "orders") return [cards.ordersCard(view, act)];
  return [];
}

async function readPage(tab, generation) {
  // 読み込みが終わった直後は、ページ側の準備が少し遅れることがある。
  for (let attempt = 0; attempt < 3; attempt += 1) {
    try {
      const response = await chrome.tabs.sendMessage(tab.id, { type: "extract-page" });
      return { products: Array.isArray(response?.products) ? response.products : [], stale: false };
    } catch {
      if (tab.status !== "complete") return null;
      await new Promise((resolve) => setTimeout(resolve, 400));
      if (generation !== pageGeneration) return null;
    }
  }
  // 拡張を入れる前・更新する前に開いたタブには、拡張のスクリプトが入っていない。
  return { products: [], stale: true };
}

// 今のウィンドウで見ているタブに合わせる。タブの切り替え、ページの移動、ウィンドウへのフォーカスで呼ぶ。
async function syncPage() {
  const generation = ++pageGeneration;
  const [found] = await chrome.tabs.query({ active: true, currentWindow: true }).catch(() => []);
  if (generation !== pageGeneration) return;
  // この画面をタブで開いているとき（サイドパネルの無いブラウザ）は、見ているページが無い。
  const tab = found && !String(found.url || "").startsWith(chrome.runtime.getURL("")) ? found : null;
  const key = tab ? `${tab.id}:${tab.url || ""}` : "";
  state.tab = tab;
  state.kind = shared.pageKind(tab?.url || "");
  if (key !== state.pageKey) {
    state.pageKey = key;
    state.products = null;
    state.pageStale = false;
    state.statuses = new Map();
    state.detail = null;
    state.message = "";
  }
  render();
  if (!["item", "library"].includes(state.kind)) {
    library.setPage({ productId: "", avatarIds: [] });
    return;
  }
  const page = await readPage(tab, generation);
  if (generation !== pageGeneration || !page) return;
  state.products = page.products;
  state.pageStale = page.stale;
  render();
  await refreshStatuses();
}

// 登録・所持・保存記録と対応アバター、購入の記録をアプリに尋ねる。答えが古いページのものなら捨てる。
async function refreshStatuses() {
  const generation = pageGeneration;
  if (!["item", "library"].includes(state.kind) || !state.products?.length || !link().reachable) return;
  const ids = [...new Set(state.products.map((product) => String(product.boothId)))].slice(0, 100);
  const result = await send({ type: "product-status", ids });
  if (generation !== pageGeneration) return;
  if (Array.isArray(result?.products)) state.statuses = new Map(result.products.map((row) => [row.boothId, row]));
  if (state.kind !== "item") {
    library.setPage({ productId: "", avatarIds: [] });
    render();
    return;
  }
  const id = ids[0];
  // 直接の対応を先に、共通素体からの対応を後に。
  const avatars = [...(state.statuses.get(id)?.avatars || [])].sort((a, b) => Number(a.viaBaseBody) - Number(b.viaBaseBody));
  library.setPage({ productId: id, avatarIds: avatars.map((avatar) => avatar.id) });
  render();
  const detail = await send({ type: "library-product", id });
  if (generation !== pageGeneration) return;
  state.detail = detail && !detail.error ? detail : null;
  render();
}

// ---- 読み込みの進み具合 ----

function jobSection() {
  const job = state.job;
  if (!job) return [];
  if (job.state === "running") {
    return [
      h("span", { class: "sec", text: "実行中" }),
      h("section", { class: "card" },
        h("span", { class: "title", text: job.label }),
        h("div", { class: "row note num" },
          h("span", { class: "grow", text: job.line }),
          job.count ? h("span", { text: `${job.count} 件` }) : null,
        ),
        ui.track(job.current, job.total, job.label),
        h("div", { class: "row" },
          h("p", { class: "note grow", text: "中止しても、読んだところまでは保存します。" }),
          h("button", { class: "btn danger small", type: "button", text: "中止", on: { click: () => send({ type: "job-cancel" }) } }),
        ),
      ),
    ];
  }
  // 成功した読み込みは「最近の取り込み」に出る。止まった・中止したものだけ理由を残す。
  if (job.state === "done" || !job.finishedAt || Date.now() - Date.parse(job.finishedAt) > 60 * 60 * 1000) return [];
  const label = job.state === "cancelled" ? "中止" : "途中で止まりました";
  return [
    h("span", { class: "sec", text: "最後の読み込み" }),
    h("section", { class: "card" },
      h("div", { class: "row" }, h("span", { class: "title grow", text: shared.HISTORY_LABELS[job.kind] || "読み込み" }), ui.chip(label, ui.jobTone(job.state))),
      h("p", { class: "note", role: "status", text: job.line }),
    ),
  ];
}

// ---- 記録 ----

function downloadRows() {
  const rows = [];
  for (const intent of state.stored.downloadIntents || []) {
    const item = state.progress.get(intent.downloadId);
    const total = Number(item?.totalBytes) > 0 ? Number(item.totalBytes) : 0;
    const received = Number(item?.bytesReceived) || 0;
    const detail = total
      ? `${shared.formatBytes(received)} / ${shared.formatBytes(total)}（${Math.round((received / total) * 100)}%）`
      : item?.state === "interrupted" ? "中断しました。ブラウザのダウンロード画面で再開できます" : "取得しています";
    rows.push(h("div", { class: "item" },
      h("div", { class: "file" },
        h("b", { text: intent.file.filename, title: intent.file.filename }),
        h("span", { class: "num", text: [intent.file.variation, detail].filter(Boolean).join("・") }),
        ui.track(received, total, `${intent.file.filename} の取得`),
      ),
    ));
  }
  for (const entry of (state.stored.downloadLog || []).slice(0, 10)) {
    const done = entry.state === "done";
    rows.push(h("div", { class: "item" },
      h("div", { class: "file" },
        h("b", { text: entry.filename, title: entry.filename }),
        h("span", { class: done ? "" : "warn", text: [entry.variation, entry.message].filter(Boolean).join("・") }),
      ),
      ui.chip(done ? "完了" : "送信待ち", done ? "good" : "warn"),
    ));
  }
  return rows;
}

function pendingRows() {
  const token = shared.normalizeToken(state.stored.pairingToken);
  return (state.stored.pendingDeliveries || []).map((entry) => {
    const name = entry.kind === "checkout"
      ? `購入完了ページ（商品 ${entry.checkout?.products?.length || 0} 件）`
      : entry.notice?.filename || "ダウンロードしたファイル";
    const reason = entry.token !== token
      ? "前のペアリングコードのときの記録です。そのコードに戻すまでアプリへ送りません"
      : state.stored.deliveryError || "アプリの起動を待っています";
    return h("div", { class: "item" }, h("div", { class: "file" }, h("b", { text: name, title: name }), h("span", { text: reason })));
  });
}

function historyRows() {
  return (state.stored.importHistory || []).slice(0, 8).map((entry) => {
    const label = shared.HISTORY_LABELS[entry.kind] || "取り込み";
    let counts;
    if (entry.kind === "purchases") counts = `${entry.pages || 0} ページ、新しい注文 ${entry.added || 0} 件、詳細 ${entry.details || 0} 件`;
    else if (entry.kind === "checkout") counts = `商品 ${entry.products || 0} 件`;
    else if (entry.saved) counts = `${entry.products || 0} 件を JSON に保存`;
    else counts = [entry.pages ? `${entry.pages} ページ` : "", `追加 ${entry.inserted ?? 0} 件、更新 ${entry.updated ?? 0} 件`].filter(Boolean).join("、");
    return h("div", { class: "item" },
      h("div", { class: "file" }, h("b", { text: label }), h("span", { class: "num", text: `${shared.formatTime(entry.at)}・${counts}` })),
      entry.complete ? ui.chip("完了", "good") : null,
    );
  });
}

function logParts() {
  const parts = [];
  const downloads = downloadRows();
  if (downloads.length) parts.push(h("span", { class: "sec", text: "ダウンロード" }), h("section", { class: "card" }, h("div", { class: "list" }, downloads)));
  const pending = pendingRows();
  if (pending.length) {
    parts.push(
      h("span", { class: "sec num", text: `アプリへ未送信 ${pending.length} 件` }),
      h("section", { class: "card" },
        h("div", { class: "list" }, pending),
        h("button", {
          class: "btn secondary block",
          type: "button",
          text: "アプリへ送り直す",
          on: {
            click: async () => {
              const result = await send({ type: "retry-deliveries" });
              state.note = result?.error || "";
              render();
            },
          },
        }),
        state.note ? h("p", { class: "note warn", role: "status", text: state.note }) : null,
      ),
    );
  }
  const history = historyRows();
  if (history.length) parts.push(h("span", { class: "sec", text: "最近の取り込み" }), h("section", { class: "card" }, h("div", { class: "list" }, history)));
  if (parts.length === 0) parts.push(h("p", { class: "note", text: "まだ記録はありません。商品情報を送ったりファイルを取得したりすると、ここに結果が出ます。" }));
  return parts;
}

// ---- 描画 ----

function setView(view) {
  state.view = view === "log" ? "log" : "library";
  render();
}

function render() {
  const status = link();
  linkChip.textContent = status.label;
  linkChip.className = status.tone ? `chip ${status.tone}` : "chip";
  // 気づいてほしい記録（取得中のファイルと未送信）の件数を「記録」に添える。
  const waiting = (state.stored.downloadIntents || []).length + (state.stored.pendingDeliveries || []).length;
  viewButtons.log.textContent = waiting ? `記録 ${waiting}` : "記録";
  for (const [name, button] of Object.entries(viewButtons)) {
    button.setAttribute("aria-selected", state.view === name ? "true" : "false");
    button.tabIndex = state.view === name ? 0 : -1;
  }
  const parts = [];
  if (!status.paired) {
    parts.push(h("section", { class: "card" },
      h("span", { class: "title", text: "アプリとつなぐ" }),
      h("p", { class: "note", text: status.message }),
      h("button", { class: "btn secondary block", type: "button", text: "設定を開く", on: { click: () => chrome.runtime.openOptionsPage() } }),
    ));
  }
  parts.push(...jobSection());
  if (state.view === "library" && status.paired) parts.push(...pageCards());
  cardSlot.replaceChildren(...parts.filter(Boolean));
  librarySlot.hidden = state.view !== "library" || !status.paired;
  logSlot.hidden = state.view !== "log";
  if (state.view === "log") logSlot.replaceChildren(...logParts());
  schedulePolling();
}

async function pollDownloads() {
  const ids = (state.stored.downloadIntents || []).map((row) => row.downloadId).filter((id) => id != null);
  const next = new Map();
  for (const id of ids) {
    const [item] = await chrome.downloads.search({ id }).catch(() => []);
    if (item) next.set(id, item);
  }
  state.progress = next;
  render();
}

function schedulePolling() {
  // 取得中のファイルの進み具合は「記録」を出しているあいだだけ読む。
  const active = state.view === "log" && (state.stored.downloadIntents || []).some((row) => row.downloadId != null);
  if (active && !poller) poller = setInterval(pollDownloads, 1000);
  if (!active && poller) {
    clearInterval(poller);
    poller = null;
  }
}

async function refreshConnection() {
  state.connection = await send({ type: "connection" });
  if (state.connection?.error) state.connection = { status: null };
  const reachable = Boolean(link().reachable);
  if (reachable) libraryLoadedAt = Date.now();
  library.setReachable(reachable);
  render();
  await refreshStatuses();
}

for (const [name, button] of Object.entries(viewButtons)) {
  button.addEventListener("click", () => setView(name));
  button.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    const other = name === "library" ? "log" : "library";
    setView(other);
    viewButtons[other].focus();
  });
}

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "session") {
    if (changes.job) state.job = changes.job.newValue || null;
    // ポップアップの「未送信 N 件」から開いたときは「記録」を出す。
    if (changes.panelView?.newValue === "log") {
      state.view = "log";
      chrome.storage.session.remove("panelView").catch(() => {});
    }
  }
  if (area === "local") {
    for (const [key, change] of Object.entries(changes)) state.stored[key] = change.newValue;
    if (changes.followPageAvatar) library.setFollow(changes.followPageAvatar.newValue !== false);
    if (changes.pairingToken) refreshConnection();
    if (changes.importHistory || changes.downloadLog) libraryChanged();
  }
  render();
});

chrome.tabs.onActivated.addListener(() => syncPage());
chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (!change.url && change.status !== "complete") return;
  if (tabId === state.tab?.id || tab?.active) syncPage();
});
window.addEventListener("focus", () => {
  syncPage();
  refreshConnection().then(() => {
    if (link().reachable && Date.now() - libraryLoadedAt > LIBRARY_STALE_MS) refreshLibrary();
  });
});

async function init() {
  state.stored = await chrome.storage.local.get(state.stored);
  const session = await chrome.storage.session.get({ job: null, panelView: "" });
  state.job = session.job;
  if (session.panelView === "log") {
    state.view = "log";
    chrome.storage.session.remove("panelView").catch(() => {});
  }
  library.setFollow(state.stored.followPageAvatar !== false);
  render();
  await Promise.all([refreshConnection(), syncPage()]);
  pollDownloads();
}

globalThis.sidePanelUnderTest = { state, render, library, syncPage, refreshConnection };
init();
