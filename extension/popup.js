// ポップアップ。接続の状態と、開いているページでできる主な操作だけを出す。
// 長い読み込みはサービスワーカーと offscreen 文書が受け持つので、閉じても続く。
const shared = globalThis.VrcLibManShared;
const companion = globalThis.VrcLibManCompanion;
const ui = globalThis.VrcLibManUi;
const cards = globalThis.VrcLibManPageCards;
const { h } = ui;

const main = document.querySelector("#main");
const linkChip = document.querySelector("#link-chip");
const routeInput = document.querySelector("#route-downloads");
const pendingButton = document.querySelector("#pending");
const pendingCount = document.querySelector("#pending-count");
document.querySelector("#open-panel").append(ui.icon("panel"));
document.querySelector("#open-settings").append(ui.icon("settings"));

const state = {
  tab: null,
  kind: "other",
  products: null,
  pageStale: false,
  statuses: new Map(),
  connection: undefined,
  stored: { ...shared.DEFAULTS, pendingDeliveries: [], importHistory: [], extensionUpdate: null },
  job: null,
  message: "",
  tone: "",
  busy: false,
};

function send(message) {
  return chrome.runtime.sendMessage(message).catch((error) => ({ error: error.message || String(error) }));
}

function hasToken() {
  return Boolean(state.stored.pairingToken);
}

function link() {
  return shared.connectionState(state.connection === undefined ? undefined : state.connection?.status ?? null, hasToken());
}

function running() {
  return state.job?.state === "running";
}

function say(text, tone = "") {
  state.message = text || "";
  state.tone = tone;
  render();
}

async function loadStored() {
  state.stored = await chrome.storage.local.get({ ...shared.DEFAULTS, pendingDeliveries: [], importHistory: [], extensionUpdate: null });
  state.job = (await chrome.storage.session.get({ job: null })).job;
}

async function refreshConnection() {
  state.connection = await send({ type: "connection" });
  if (state.connection?.error) state.connection = { status: null };
  render();
  await refreshStatuses();
}

async function readPage() {
  if (!state.tab?.id || !["item", "library"].includes(state.kind)) return;
  try {
    const response = await chrome.tabs.sendMessage(state.tab.id, { type: "extract-page" });
    state.products = Array.isArray(response?.products) ? response.products : [];
    state.pageStale = false;
  } catch {
    // 拡張を入れる前・更新する前に開いたタブには、拡張のスクリプトが入っていない。
    state.products = [];
    state.pageStale = true;
  }
  render();
  await refreshStatuses();
}

async function refreshStatuses() {
  if (state.kind !== "item" || !state.products?.length || !link().reachable) return;
  const ids = [...new Set(state.products.map((product) => String(product.boothId)))].slice(0, 100);
  const result = await send({ type: "product-status", ids });
  if (Array.isArray(result?.products)) {
    state.statuses = new Map(result.products.map((row) => [row.boothId, row]));
    render();
  }
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

function openUrl(url) {
  chrome.tabs.create({ url });
  window.close();
}

// view が "log" のときは、サイドパネルの「記録」（ダウンロード・未送信・最近の取り込み）を出して開く。
async function openPanel(view) {
  // 待たずに書く。サイドパネルは本人の操作の直後にしか開けない。
  if (view === "log") chrome.storage.session.set({ panelView: "log" }).catch(() => {});
  try {
    if (chrome.sidePanel?.open && state.tab?.windowId != null) {
      await chrome.sidePanel.open({ windowId: state.tab.windowId });
      window.close();
      return;
    }
  } catch {
    // サイドパネルを開けないブラウザでは、同じ画面をタブで開く。
  }
  openUrl(chrome.runtime.getURL("sidepanel.html"));
}

function recentLine() {
  const job = state.job;
  if (!job || job.state === "running" || !job.finishedAt) return null;
  if (Date.now() - Date.parse(job.finishedAt) > 15 * 60 * 1000) return null;
  return h("p", { class: `note ${job.state === "done" ? "" : "warn"}`, role: "status", text: job.line });
}

function messageLine() {
  if (!state.message) return recentLine();
  return h("p", { class: `note ${state.tone}`, role: "status", text: state.message });
}

function pairingCard() {
  const status = link();
  const input = h("input", {
    class: "field num",
    type: "text",
    value: state.stored.pairingToken ? shared.displayToken(state.stored.pairingToken) : "",
    placeholder: "XXXX-XXXX",
    autocomplete: "off",
    spellcheck: false,
    "aria-label": "ペアリングコード",
  });
  const save = async () => {
    await chrome.storage.local.set({ pairingToken: shared.displayToken(input.value) });
    state.connection = undefined;
    say("");
    await refreshConnection();
  };
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") save();
  });
  return h("section", { class: "card" },
    h("span", { class: "title", text: "アプリとつなぐ" }),
    h("p", { class: `note ${hasToken() ? "warn" : ""}`, text: status.message }),
    input,
    h("button", { class: "btn primary block", type: "button", text: "接続する", on: { click: save } }),
    messageLine(),
  );
}

function jobCard() {
  const job = state.job;
  const counts = job.total > 0 ? `${job.current} / ${job.total}` : "";
  return h("section", { class: "card" },
    h("span", { class: "sec", text: "実行中" }),
    h("span", { class: "title", text: job.label }),
    h("div", { class: "row note num" }, h("span", { class: "grow", text: job.line }), job.count ? h("span", { text: `${job.count} 件` }) : null),
    ui.track(job.current, job.total, `${job.label} ${counts}`),
    h("p", { class: "note", text: "ポップアップを閉じても続きます。" }),
    h("div", { class: "row" },
      h("button", { class: "btn secondary grow", type: "button", text: "サイドパネルで見る", on: { click: () => openPanel() } }),
      h("button", { class: "btn danger", type: "button", text: "中止", on: { click: () => send({ type: "job-cancel" }) } }),
    ),
  );
}

function reloadPage() {
  chrome.tabs.reload(state.tab.id);
  window.close();
}

// カードは pagecards.js（サイドパネルと共通）。ここは操作の渡し方だけを決める。
const act = { startJob, send, say, openUrl, reloadPage, message: messageLine };
const ROUTE_HINT = "下の「ダウンロードを自動整理」をオンにすると、ここから取得できます。";

function jsonButton() {
  if (!["item", "library"].includes(state.kind) || !state.products?.length) return null;
  return h("button", {
    class: "btn block",
    type: "button",
    text: "このページを JSON として保存",
    on: {
      click: async () => {
        const result = await send({ type: "save-page-json", tabId: state.tab.id });
        say(result?.ok ? `商品 ${result.count} 件を JSON として保存しました。アプリのインポート画面へドロップしてください` : result?.error, result?.ok ? "good" : "warn");
      },
    },
  });
}

// アプリが用意したフォルダに新しい版があるとき。読み込み直しても変わらないときは、読み込み元の変え方を案内する。
function updateCard() {
  const update = state.stored.extensionUpdate;
  if (!update?.version) return null;
  if (update.stuck) {
    return h("section", { class: "card" },
      h("span", { class: "title", text: `拡張機能の更新（v${update.version}）があります` }),
      h("p", { class: "note warn", text: "この拡張機能は、アプリが用意したフォルダとは別の場所から入れられているため、自動では更新できません。アプリの「設定 → BOOTH → 拡張機能をつなぐ」に出るフォルダを、ブラウザの拡張機能の画面から入れ直してください。" }),
    );
  }
  return h("section", { class: "card" },
    h("span", { class: "title", text: `拡張機能を v${update.version} に更新できます` }),
    h("p", { class: "note", text: "更新すると、このポップアップはいったん閉じます。" }),
    h("button", {
      class: "btn secondary block",
      type: "button",
      text: "更新する",
      on: {
        click: async () => {
          const result = await send({ type: "reload-extension" });
          if (result?.error) say(result.error, "warn");
        },
      },
    }),
  );
}

function render() {
  const status = link();
  linkChip.textContent = status.label;
  linkChip.className = status.tone ? `chip ${status.tone}` : "chip";
  routeInput.checked = state.stored.routeDownloads === true;
  const pending = (state.stored.pendingDeliveries || []).length;
  pendingButton.hidden = pending === 0;
  pendingCount.textContent = String(pending);
  document.querySelector("#footer").hidden = !status.paired;
  document.querySelector("#open-panel").hidden = !status.paired;
  const parts = [];
  if (!status.paired) {
    parts.push(pairingCard(), jsonButton());
  } else {
    parts.push(updateCard());
    if (running()) parts.push(jobCard());
    else if (state.kind === "item") parts.push(cards.itemCard(state, act));
    else if (state.kind === "library") parts.push(cards.libraryCard(state, act));
    else if (state.kind === "orders") parts.push(cards.ordersCard(state, act));
    else parts.push(cards.otherCard(state, act));
    if (state.kind === "item") parts.push(cards.filesCard(state, act, ROUTE_HINT));
  }
  main.replaceChildren(...parts.filter(Boolean));
}

routeInput.addEventListener("change", async () => {
  const result = await send({ type: "set-route", enabled: routeInput.checked });
  if (result?.error) say(result.error, "warn");
  else if (result?.note) say(result.note);
  else say(result?.enabled ? "ダウンロードしたファイルを自動整理します" : "自動整理をオフにしました");
});
pendingButton.addEventListener("click", () => openPanel("log"));
document.querySelector("#open-panel").addEventListener("click", () => openPanel());
document.querySelector("#open-settings").addEventListener("click", () => {
  chrome.runtime.openOptionsPage();
  window.close();
});

chrome.storage.onChanged.addListener(async (changes, area) => {
  if (area === "session" && changes.job) state.job = changes.job.newValue || null;
  if (area === "local") {
    for (const [key, change] of Object.entries(changes)) state.stored[key] = change.newValue;
  }
  render();
});

async function init() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  state.tab = tab || null;
  state.kind = shared.pageKind(tab?.url || "");
  await loadStored();
  render();
  await Promise.all([refreshConnection(), readPage()]);
}

globalThis.popupUnderTest = { state, render, refreshConnection, startJob, init };
init();
