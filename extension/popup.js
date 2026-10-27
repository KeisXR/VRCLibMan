const status = document.querySelector("#status");
document.querySelector("#extension-version").textContent = `拡張機能 v${chrome.runtime.getManifest().version}`;
const linkStatus = document.querySelector("#link-status");
const connectionSettings = document.querySelector("#connection-settings");
const resultDetails = document.querySelector("#result-details");
const resultBreakdown = document.querySelector("#result-breakdown");
const button = document.querySelector("#export");
const allButton = document.querySelector("#export-all");
const newButton = document.querySelector("#export-new");
const saveTokenButton = document.querySelector("#save-token");
const openLibraryButton = document.querySelector("#open-library");
const diagnoseButton = document.querySelector("#diagnose");
const diagnosisBox = document.querySelector("#diagnosis");
const tokenInput = document.querySelector("#token");
const routeInput = document.querySelector("#route-downloads");
const intervalInput = document.querySelector("#booth-interval");
const api = () => globalThis.VrcLibManExtract;
const LOCAL_PORT = 17321;
const LOCAL_BASE = `http://127.0.0.1:${LOCAL_PORT}`;
const LIBRARY_URL = "https://accounts.booth.pm/library";
const LIBRARY_SECTIONS = [
  { url: "https://accounts.booth.pm/library", label: "ライブラリ" },
  { url: "https://accounts.booth.pm/library/gifts", label: "ギフト" },
  { url: "https://accounts.booth.pm/library/free_downloads", label: "無料ダウンロード" },
];
const WAKE_URL = "vrclibman://import";
const WAKE_WAIT_MS = 8000;

function setStatus(text) {
  status.textContent = text;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

const requestPolicy = globalThis.VrcLibManRequestPolicy;
let paceBoothRequest = requestPolicy.createPacer();

// Each user-started import captures the selected pace.
function resetPacer() {
  paceBoothRequest = requestPolicy.createPacer(requestPolicy.normalizeIntervalMs(intervalInput.value));
}

async function fetchWithBackoff(url, options) {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    await paceBoothRequest();
    const response = await fetch(url, options);
    if (!requestPolicy.isThrottled(response.status)) return response;
    if (attempt === 1) {
      throw new Error("BOOTH のアクセス制限が続いているため、ここまでで止めました");
    }
    const requested = requestPolicy.retryAfterMs(response.headers.get("retry-after"));
    const waitMs = requested ?? requestPolicy.DEFAULT_RETRY_MS;
    if (waitMs > requestPolicy.MAX_RETRY_MS) {
      throw new Error("BOOTH から長い待機時間を指定されたため、再試行せずここまでで止めました");
    }
    setStatus(`BOOTH のアクセス制限を受けたため ${Math.ceil(waitMs / 1000)} 秒待って一度だけ再試行します`);
    await sleep(waitMs);
  }
  throw new Error("BOOTH から応答が得られず、ここまでで止めました");
}

async function currentTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id || !tab.url || !/^https:\/\/([^/]+\.)?booth\.pm\//.test(tab.url)) {
    throw new Error("BOOTHの商品ページかライブラリを開いてから実行してください");
  }
  return tab;
}

async function readTab() {
  const tab = await currentTab();
  try {
    const response = await chrome.tabs.sendMessage(tab.id, { type: "extract-page" });
    if (response?.error) throw new Error(response.error);
    if (Array.isArray(response?.products)) return response.products;
  } catch {
    await chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["extract.js"] });
    const [result] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => globalThis.VrcLibManExtract.extractPage(document, location.href),
    });
    return Array.isArray(result?.result) ? result.result : [];
  }
  return [];
}

function parseHtml(html, href) {
  const doc = new DOMParser().parseFromString(api().quietHtml(html), "text/html");
  return { doc, products: api().extractPage(doc, href) };
}

async function readLibraryPages(startUrl, knownIds, label) {
  const sectionLabel = label || "ライブラリ";
  const firstUrl = api().libraryPageUrl(startUrl, 1);
  setStatus(`${sectionLabel} 1 ページ目を読んでいます`);
  const firstResponse = await fetchWithBackoff(firstUrl, { credentials: "include" });
  if (!firstResponse.ok) {
    throw new Error(`${sectionLabel}を読めませんでした (${firstResponse.status})`);
  }
  const first = parseHtml(await firstResponse.text(), firstUrl);
  let taken = api().takeUntilKnown(first.products, knownIds);
  let products = api().dedupeProducts(taken.products);
  let stopped = taken.stopped;
  let pagesRead = 1;
  const lastPage = api().libraryLastPage(first.doc, firstUrl);
  try {
    for (let page = 2; page <= lastPage && !stopped; page += 1) {
      setStatus(`${sectionLabel} ${page}/${lastPage} ページを読んでいます（${products.length} 件）`);
      const pageUrl = api().libraryPageUrl(startUrl, page);
      const response = await fetchWithBackoff(pageUrl, { credentials: "include" });
      if (!response.ok) {
        throw new Error(`${sectionLabel} ${page} ページ目を読めませんでした (${response.status})`);
      }
      const parsed = parseHtml(await response.text(), pageUrl);
      pagesRead = page;
      if (parsed.products.length === 0) break;
      taken = api().takeUntilKnown(parsed.products, knownIds);
      products = api().dedupeProducts(products.concat(taken.products));
      stopped = taken.stopped;
    }
  } catch (error) {
    error.partialProducts = products;
    error.libraryPages = lastPage;
    throw error;
  }
  return { products, pagesRead, stopped };
}

async function readAllLibrarySections(knownIds) {
  let products = [];
  let pagesRead = 0;
  let stopped = false;
  for (const section of LIBRARY_SECTIONS) {
    try {
      const collected = await readLibraryPages(section.url, knownIds, section.label);
      products = api().dedupeProducts(products.concat(collected.products));
      pagesRead += collected.pagesRead;
      stopped = stopped || collected.stopped;
    } catch (error) {
      const partial = error instanceof Error && Array.isArray(error.partialProducts) ? error.partialProducts : [];
      products = api().dedupeProducts(products.concat(partial));
      if (error instanceof Error) error.partialProducts = products;
      throw error;
    }
  }
  return { products, pagesRead, stopped };
}

function needsDetail(product) {
  return (product.descriptionSnippet || "").length < 200 || !product.shop;
}

async function enrich(products) {
  const targets = products.filter(needsDetail);
  for (let index = 0; index < targets.length; index += 1) {
    const product = targets[index];
    setStatus(`説明文を補完しています ${index + 1}/${targets.length} 件`);
    let htmlStatus = 0;
    let jsonStatus = 0;
    try {
      const response = await fetchWithBackoff(product.url, { credentials: "omit" });
      htmlStatus = response.status;
      if (response.ok) {
        const { products: parsed } = parseHtml(await response.text(), product.url);
        api().mergeDetail(product, parsed[0]);
      }
    } catch (error) {
      if (error instanceof Error && error.message.includes("止めました")) throw error;
    }
    // Do not fall back to another endpoint after explicit access denial.
    // In particular, JSON must not be used to bypass an age/login gate.
    if (!requestPolicy.canUseJsonFallback(htmlStatus)) continue;
    if (needsDetail(product)) {
      try {
        const response = await fetchWithBackoff(`https://booth.pm/ja/items/${product.boothId}.json`, {
          credentials: "omit",
          headers: { Accept: "application/json" },
        });
        jsonStatus = response.status;
        if (response.ok) {
          const detail = api().detailFromItemJson(await response.json(), product.boothId);
          api().mergeDetail(product, detail);
        }
      } catch (error) {
        if (error instanceof Error && error.message.includes("止めました")) throw error;
      }
    }
    api().noteIfUnpublished(product, htmlStatus, jsonStatus);
  }
}

function setBusy(busy) {
  if (busy) {
    resultDetails.hidden = true;
    resultBreakdown.textContent = "";
  }
  button.disabled = busy;
  allButton.disabled = busy;
  newButton.disabled = busy;
  intervalInput.disabled = busy;
  saveTokenButton.disabled = busy;
}

function isUnreachable(error) {
  return Boolean(error && error.unreachable);
}

function unreachableError() {
  const error = new TypeError("アプリに届きません");
  error.unreachable = true;
  return error;
}

async function savedToken() {
  const stored = await chrome.storage.local.get("pairingToken");
  return String(stored.pairingToken || "").trim();
}

async function probe(token) {
  try {
    const response = await fetch(`${LOCAL_BASE}/health`, {
      method: "GET",
      credentials: "omit",
      headers: { Authorization: `Bearer ${token}`, "X-VRCLibMan-Extension-Version": chrome.runtime.getManifest().version },
    });
    return response.status;
  } catch {
    return null;
  }
}

async function storedRoute() {
  const stored = await chrome.storage.local.get({ routeDownloads: false });
  return stored.routeDownloads === true;
}

async function refreshLink() {
  const token = await savedToken();
  tokenInput.value = token;
  routeInput.checked = await storedRoute();
  const storedInterval = await chrome.storage.local.get({ boothIntervalMs: requestPolicy.DEFAULT_INTERVAL_MS });
  intervalInput.value = String(requestPolicy.normalizeIntervalMs(storedInterval.boothIntervalMs));
  if (!token) {
    linkStatus.textContent = "未ペアリング。アプリの設定に出るコードを貼ってください";
    connectionSettings.open = true;
    return;
  }
  const status = await probe(token);
  const reachable = status !== null && status >= 200 && status < 300;
  linkStatus.textContent = reachable
    ? "アプリに接続しています"
    : status === 401
      ? "コードが一致しません。アプリの設定からコピーし直してください"
      : status === 403
        ? "この拡張機能は接続を許可されていません。アプリでコードを作り直してください"
        : status === 429
          ? "接続の試行が続いています。少し待ってから確認してください"
          : "アプリに接続できません。アプリの起動とコードを確認してください";
  connectionSettings.open = !reachable;
  if (!reachable) return;
  try {
    const response = await fetch(`${LOCAL_BASE}/route-downloads`, {
      method: "GET",
      credentials: "omit",
      headers: { Authorization: `Bearer ${token}`, "X-VRCLibMan-Extension-Version": chrome.runtime.getManifest().version },
    });
    if (!response.ok) return;
    const data = await response.json();
    const enabled = data.enabled === true;
    routeInput.checked = enabled;
    await chrome.storage.local.set({ routeDownloads: enabled });
  } catch {
    // アプリの設定が読めないときは、拡張に保存した値のままにする。
  }
}

async function fetchKnownIds(token) {
  let response;
  try {
    response = await fetch(`${LOCAL_BASE}/known-ids`, {
      method: "GET",
      credentials: "omit",
      headers: { Authorization: `Bearer ${token}`, "X-VRCLibMan-Extension-Version": chrome.runtime.getManifest().version },
    });
  } catch {
    throw unreachableError();
  }
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(data.error || `インポート済みの確認ができませんでした (${response.status})`);
  }
  return new Set((Array.isArray(data.ids) ? data.ids : []).map((id) => String(id)));
}

async function knownIdsWithWake(token) {
  try {
    return await fetchKnownIds(token);
  } catch (error) {
    if (!isUnreachable(error)) throw error;
  }
  setStatus("アプリを起動しています");
  await wakeApp();
  const started = Date.now();
  while (Date.now() - started < WAKE_WAIT_MS) {
    await sleep(400);
    try {
      return await fetchKnownIds(token);
    } catch (error) {
      if (!isUnreachable(error)) throw error;
    }
  }
  throw new Error("新しい購入だけを送るには、アプリとの接続が必要です。アプリを起動してから再実行してください");
}

async function postImport(token, payload) {
  let response;
  try {
    response = await fetch(`${LOCAL_BASE}/import`, {
      method: "POST",
      credentials: "omit",
      headers: {
        Authorization: `Bearer ${token}`, "X-VRCLibMan-Extension-Version": chrome.runtime.getManifest().version,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    });
  } catch {
    throw unreachableError();
  }
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(data.error || `アプリが受け付けませんでした (${response.status})`);
  }
  return data;
}

async function wakeApp() {
  try {
    const tab = await chrome.tabs.create({ url: WAKE_URL, active: false });
    setTimeout(() => {
      if (tab?.id != null) chrome.tabs.remove(tab.id).catch(() => {});
    }, 1200);
  } catch {
    // プロトコルを開けないときは、再試行のあと案内する。
  }
}

async function deliver(products, source) {
  const token = await savedToken();
  const payload = {
    format: "vrclibman.booth-export",
    version: 1,
    exportedAt: new Date().toISOString(),
    source,
    products,
  };
  if (!token) {
    await download(products, source);
    return "ペアリングコードが未入力なので、JSONを保存しました";
  }
  try {
    return sentText(await postImport(token, payload), products);
  } catch (error) {
    if (!isUnreachable(error)) throw error;
  }
  setStatus("アプリを起動しています");
  await wakeApp();
  const started = Date.now();
  while (Date.now() - started < WAKE_WAIT_MS) {
    await sleep(400);
    try {
      return sentText(await postImport(token, payload), products);
    } catch (error) {
      if (!isUnreachable(error)) throw error;
    }
  }
  await download(products, source);
  return "アプリに接続できませんでした。JSONを保存したので、アプリのインポート画面へドロップしてください";
}

function sentText(report, products) {
  const parts = [
    `商品情報を ${products.length} 件送りました。追加 ${report.inserted ?? 0} 件、更新 ${report.updated ?? 0} 件`,
  ];
  const details = [`変更なし ${report.unchanged ?? 0} 件`];
  if (report.skipped) parts.push(`読み込めなかった商品 ${report.skipped} 件`);
  if (typeof report.variations === "number" || typeof report.downloads === "number") {
    details.push(`バリエーション ${report.variations ?? 0} 件、ファイル情報 ${report.downloads ?? 0} 件`);
  }
  if (report.unnamedDownloads) details.push(`ファイル名未取得 ${report.unnamedDownloads} 件`);
  resultBreakdown.textContent = details.join("。");
  resultDetails.hidden = false;
  if (Array.isArray(report.warnings)) parts.push(...report.warnings);
  return parts.join("。");
}

async function download(products, source) {
  const payload = {
    format: "vrclibman.booth-export",
    version: 1,
    exportedAt: new Date().toISOString(),
    source,
    products,
  };
  const blob = new Blob([JSON.stringify(payload, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const stamp = new Date().toISOString().slice(0, 16).replace(/[-:]/g, "").replace("T", "-");
  await chrome.downloads.download({
    url,
    filename: `vrclibman-booth-${stamp}.json`,
    saveAs: true,
  });
}

button.addEventListener("click", async () => {
  resetPacer();
  setBusy(true);
  try {
    setStatus("ページを読んでいます");
    const products = api().dedupeProducts(await readTab());
    if (products.length === 0) {
      setStatus("このページから商品を見つけられませんでした");
      return;
    }
    await enrich(products);
    setStatus(await deliver(products, "current-page"));
    await refreshLink();
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error));
  } finally {
    setBusy(false);
  }
});

async function sendLibrary(updateMode) {
  resetPacer();
  setBusy(true);
  let products = [];
  try {
    const tab = await currentTab();
    if (!api().isLibraryUrl(tab.url)) {
      setStatus("ライブラリのページを開いてから実行してください");
      return;
    }
    let knownIds = null;
    if (updateMode) {
      const token = await savedToken();
      if (!token) {
        setStatus("新しい購入だけを送るにはペアリングが必要です。アプリの設定のコードを貼ってください");
        return;
      }
      knownIds = await knownIdsWithWake(token);
    }
    const collected = await readAllLibrarySections(knownIds);
    products = collected.products;
    if (products.length === 0) {
      setStatus(updateMode && collected.stopped ? "新しい購入はありません" : "ライブラリから商品を見つけられませんでした");
      return;
    }
    const source = updateMode ? "library-update" : "library";
    try {
      await enrich(products);
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      setStatus(`${await deliver(products, source)}。${reason}`);
      await refreshLink();
      return;
    }
    const stopNote = updateMode && collected.stopped ? "。インポート済みの商品があったので、そこまでで止めました" : "";
    setStatus(`${await deliver(products, source)}（${collected.pagesRead} ページを確認）${stopNote}`);
    await refreshLink();
  } catch (error) {
    const partial = error instanceof Error && Array.isArray(error.partialProducts) ? error.partialProducts : products;
    const reason = error instanceof Error ? error.message : String(error);
    const source = updateMode ? "library-update" : "library";
    if (partial.length > 0) {
      try {
        setStatus(`${await deliver(partial, source)}。${reason}`);
        await refreshLink();
      } catch (sendError) {
        setStatus(sendError instanceof Error ? sendError.message : reason);
      }
      return;
    }
    setStatus(reason);
  } finally {
    setBusy(false);
  }
}

intervalInput.addEventListener("change", async () => {
  const next = requestPolicy.normalizeIntervalMs(intervalInput.value);
  const stored = await chrome.storage.local.get({ boothIntervalMs: requestPolicy.DEFAULT_INTERVAL_MS });
  const old = requestPolicy.normalizeIntervalMs(stored.boothIntervalMs);
  if (next === 250 && old !== 250 &&
      !window.confirm("0.25秒間隔はBOOTHへのアクセス制限やアカウントへの影響が起きる可能性のある高リスク設定です。規約上の安全性は保証されません。この設定を使用しますか？")) {
    intervalInput.value = String(old);
    return;
  }
  await chrome.storage.local.set({ boothIntervalMs: next });
  intervalInput.value = String(next);
  setStatus(`BOOTHへのアクセス間隔を ${next / 1000} 秒に設定しました`);
});

allButton.addEventListener("click", () => sendLibrary(false));
newButton.addEventListener("click", () => sendLibrary(true));

saveTokenButton.addEventListener("click", async () => {
  await chrome.storage.local.set({ pairingToken: tokenInput.value.trim() });
  await refreshLink();
});

openLibraryButton.addEventListener("click", () => {
  chrome.tabs.create({ url: LIBRARY_URL });
});

diagnoseButton.addEventListener("click", async () => {
  try {
    const tab = await currentTab();
    await chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["extract.js"] });
    const [result] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => globalThis.VrcLibManExtract.diagnose(document, location.href),
    });
    const text = JSON.stringify(result?.result ?? {}, null, 2);
    diagnosisBox.hidden = false;
    diagnosisBox.value = text;
    try {
      await navigator.clipboard.writeText(text);
      setStatus("診断情報をコピーしました。商品名やファイル名は含めていません");
    } catch {
      setStatus("診断情報を下の欄に出しました。商品名やファイル名は含めていません");
    }
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error));
  }
});

routeInput.addEventListener("change", async () => {
  const enabled = routeInput.checked === true;
  await chrome.storage.local.set({ routeDownloads: enabled });
  const token = await savedToken();
  if (!token) return;
  try {
    const response = await fetch(`${LOCAL_BASE}/route-downloads`, {
      method: "POST",
      credentials: "omit",
      headers: {
        Authorization: `Bearer ${token}`, "X-VRCLibMan-Extension-Version": chrome.runtime.getManifest().version,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ enabled }),
    });
    if (!response.ok) {
      const data = await response.json().catch(() => ({}));
      routeInput.checked = false;
      await chrome.storage.local.set({ routeDownloads: false });
      setStatus(data.error || "自動整理の設定をアプリへ保存できませんでした");
    }
  } catch {
    setStatus("アプリに接続できないため、拡張機能側に設定を保存しました");
  }
});

refreshLink();
