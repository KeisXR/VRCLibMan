// サイドパネル。読み込みの進み具合、取得したファイル、アプリへの未送信通知、最近の取り込みを出し続ける。
const shared = globalThis.VrcLibManShared;
const ui = globalThis.VrcLibManUi;
const { h } = ui;
const main = document.querySelector("#main");
const linkChip = document.querySelector("#link-chip");
document.querySelector("#open-settings").append(ui.icon("settings"));
document.querySelector("#open-settings").addEventListener("click", () => chrome.runtime.openOptionsPage());

const state = {
  stored: { ...shared.DEFAULTS, pendingDeliveries: [], downloadIntents: [], downloadLog: [], importHistory: [], deliveryError: "" },
  job: null,
  connection: undefined,
  progress: new Map(),
  note: "",
};
let poller = null;

function send(message) {
  return chrome.runtime.sendMessage(message).catch((error) => ({ error: error.message || String(error) }));
}

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

function render() {
  const status = shared.connectionState(state.connection === undefined ? undefined : state.connection?.status ?? null, Boolean(state.stored.pairingToken));
  linkChip.textContent = status.label;
  linkChip.className = status.tone ? `chip ${status.tone}` : "chip";
  const parts = [];
  if (!status.paired) {
    parts.push(h("section", { class: "card" },
      h("span", { class: "title", text: "アプリとつなぐ" }),
      h("p", { class: "note", text: status.message }),
      h("button", { class: "btn secondary block", type: "button", text: "設定を開く", on: { click: () => chrome.runtime.openOptionsPage() } }),
    ));
  }
  parts.push(...jobSection());
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
  if (parts.length === 0) parts.push(h("p", { class: "note", text: "まだ記録はありません。ポップアップから商品情報を送ると、ここに進み具合と結果が出ます。" }));
  main.replaceChildren(...parts);
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
  const active = (state.stored.downloadIntents || []).some((row) => row.downloadId != null);
  if (active && !poller) poller = setInterval(pollDownloads, 1000);
  if (!active && poller) {
    clearInterval(poller);
    poller = null;
  }
}

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "session" && changes.job) state.job = changes.job.newValue || null;
  if (area === "local") for (const [key, change] of Object.entries(changes)) state.stored[key] = change.newValue;
  render();
});

async function init() {
  state.stored = await chrome.storage.local.get(state.stored);
  state.job = (await chrome.storage.session.get({ job: null })).job;
  render();
  state.connection = await send({ type: "connection" });
  if (state.connection?.error) state.connection = { status: null };
  render();
  pollDownloads();
}

globalThis.sidePanelUnderTest = { state, render };
init();
