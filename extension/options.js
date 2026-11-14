// 設定ページ。変更はその場で chrome.storage.local に保存する。自動整理はアプリ側の設定とそろえる。
const shared = globalThis.VrcLibManShared;
const policy = globalThis.VrcLibManRequestPolicy;
const $ = (selector) => document.querySelector(selector);
const statusLine = $("#status");
const tokenInput = $("#token");
const intervalInput = $("#booth-interval");

$("#version").textContent = `拡張 ${chrome.runtime.getManifest().version}`;

function send(message) {
  return chrome.runtime.sendMessage(message).catch((error) => ({ error: error.message || String(error) }));
}

function say(text, tone = "") {
  statusLine.textContent = text || "";
  statusLine.className = tone ? `note ${tone}` : "note";
}

function showConnection(result, stored) {
  const status = shared.connectionState(result === undefined ? undefined : result?.status ?? null, Boolean(stored.pairingToken));
  $("#link-chip").textContent = status.label;
  $("#link-chip").className = status.tone ? `chip ${status.tone}` : "chip";
  const seen = stored.lastConnection?.at && stored.lastConnection.status >= 200 && stored.lastConnection.status < 300
    ? `最後の通信：${shared.formatTime(stored.lastConnection.at)}`
    : "";
  $("#link-note").textContent = [status.message, seen].filter(Boolean).join(" ");
}

async function load() {
  const stored = await chrome.storage.local.get({ ...shared.DEFAULTS, lastConnection: null });
  tokenInput.value = stored.pairingToken ? shared.displayToken(stored.pairingToken) : "";
  intervalInput.value = String(policy.normalizeIntervalMs(stored.boothIntervalMs));
  $("#auto-purchase").checked = stored.autoPurchase !== false;
  $("#route-downloads").checked = stored.routeDownloads === true;
  $("#show-badges").checked = stored.showBadges !== false;
  return stored;
}

async function checkConnection() {
  showConnection(undefined, await load());
  const result = await send({ type: "connection" });
  const stored = await load();
  showConnection(result?.error ? { status: null } : result, stored);
}

$("#save-token").addEventListener("click", async () => {
  await chrome.storage.local.set({ pairingToken: shared.displayToken(tokenInput.value) });
  say("");
  await checkConnection();
});
tokenInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") $("#save-token").click();
});

intervalInput.addEventListener("change", async () => {
  const next = policy.normalizeIntervalMs(intervalInput.value);
  const stored = await chrome.storage.local.get({ boothIntervalMs: policy.DEFAULT_INTERVAL_MS });
  const old = policy.normalizeIntervalMs(stored.boothIntervalMs);
  if (next === 250 && old !== 250 &&
      !window.confirm("0.25秒間隔はBOOTHへのアクセス制限やアカウントへの影響が起きる可能性のある高リスク設定です。規約上の安全性は保証されません。この設定を使用しますか？")) {
    intervalInput.value = String(old);
    return;
  }
  await chrome.storage.local.set({ boothIntervalMs: next });
  intervalInput.value = String(next);
  say(`BOOTH へのアクセス間隔を ${next / 1000} 秒にしました`);
});

$("#auto-purchase").addEventListener("change", async (event) => {
  await chrome.storage.local.set({ autoPurchase: event.target.checked });
  say(event.target.checked ? "購入完了ページを自動で登録します" : "購入完了ページの自動登録をオフにしました");
});

$("#show-badges").addEventListener("change", async (event) => {
  await chrome.storage.local.set({ showBadges: event.target.checked });
  say("開いている BOOTH のページは、再読み込みすると反映されます");
});

$("#route-downloads").addEventListener("change", async (event) => {
  const result = await send({ type: "set-route", enabled: event.target.checked });
  event.target.checked = result?.enabled === true;
  if (result?.error) say(result.error, "warn");
  else say(result?.note || (result?.enabled ? "ダウンロードしたファイルを自動整理します" : "自動整理をオフにしました"));
});

async function lastBoothTab() {
  const tabs = await chrome.tabs.query({ url: ["https://booth.pm/*", "https://*.booth.pm/*"] });
  return tabs.sort((a, b) => (b.lastAccessed || 0) - (a.lastAccessed || 0))[0] || null;
}

$("#diagnose").addEventListener("click", async () => {
  try {
    const tab = await lastBoothTab();
    if (!tab) throw new Error("BOOTH のページを開いてから、もう一度押してください");
    await chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["extract.js"] });
    const [result] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => globalThis.VrcLibManExtract.diagnose(document, location.href),
    });
    const diagnosis = { extension: chrome.runtime.getManifest().version, ...(result?.result ?? {}) };
    const text = JSON.stringify(diagnosis, null, 2);
    const box = $("#diagnosis");
    box.hidden = false;
    box.value = text;
    try {
      await navigator.clipboard.writeText(text);
      say("診断情報をコピーしました。商品名やファイル名は含めていません");
    } catch {
      say("診断情報を下の欄に出しました。商品名やファイル名は含めていません");
    }
  } catch (error) {
    say(error.message || String(error), "warn");
  }
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  if (changes.routeDownloads) $("#route-downloads").checked = changes.routeDownloads.newValue === true;
});

checkConnection();
