const status = document.querySelector("#status");
const linkStatus = document.querySelector("#link-status");
const button = document.querySelector("#export");
const allButton = document.querySelector("#export-all");
const newButton = document.querySelector("#export-new");
const saveTokenButton = document.querySelector("#save-token");
const openLibraryButton = document.querySelector("#open-library");
const tokenInput = document.querySelector("#token");
const routeInput = document.querySelector("#route-downloads");
const api = () => globalThis.VrcLibManExtract;
const LOCAL_PORT = 17321;
const LOCAL_BASE = `http://127.0.0.1:${LOCAL_PORT}`;
const LIBRARY_URL = "https://accounts.booth.pm/library";
const WAKE_URL = "vrclibman://import";
const WAKE_WAIT_MS = 8000;

function setStatus(text) {
  status.textContent = text;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function retryAfterMs(response) {
  const seconds = Number(response.headers.get("retry-after"));
  if (Number.isFinite(seconds) && seconds > 0) return Math.min(seconds * 1000, 20000);
  return 2000;
}

async function fetchWithBackoff(url, options) {
  const first = await fetch(url, options);
  if (first.status !== 429) return first;
  const waitMs = retryAfterMs(first);
  setStatus(`サーバーが混んでいるので ${Math.ceil(waitMs / 1000)} 秒待って再試行します`);
  await sleep(waitMs);
  const second = await fetch(url, options);
  if (second.status === 429) {
    throw new Error("サーバーが混んでいるため、ここまでで止めました");
  }
  return second;
}

async function currentTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id || !tab.url || !/^https:\/\/([^/]+\.)?booth\.pm\//.test(tab.url)) {
    throw new Error("Booth の商品ページかライブラリを開いてから実行してください");
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

async function readLibraryPages(startUrl, knownIds) {
  const firstUrl = api().libraryPageUrl(startUrl, 1);
  setStatus("ライブラリ 1 ページ目を読んでいます");
  const firstResponse = await fetchWithBackoff(firstUrl, { credentials: "include" });
  if (!firstResponse.ok) {
    throw new Error(`ライブラリを読めませんでした (${firstResponse.status})`);
  }
  const first = parseHtml(await firstResponse.text(), firstUrl);
  let taken = api().takeUntilKnown(first.products, knownIds);
  let products = api().dedupeProducts(taken.products);
  let stopped = taken.stopped;
  const lastPage = api().libraryLastPage(first.doc, firstUrl);
  try {
    for (let page = 2; page <= lastPage && !stopped; page += 1) {
      await sleep(250);
      setStatus(`ライブラリ ${page}/${lastPage} ページを読んでいます（${products.length} 件）`);
      const pageUrl = api().libraryPageUrl(startUrl, page);
      const response = await fetchWithBackoff(pageUrl, { credentials: "include" });
      if (!response.ok) {
        throw new Error(`ライブラリ ${page} ページ目を読めませんでした (${response.status})`);
      }
      const parsed = parseHtml(await response.text(), pageUrl);
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
  return { products, lastPage, stopped };
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
    await sleep(250);
  }
}

function setBusy(busy) {
  button.disabled = busy;
  allButton.disabled = busy;
  newButton.disabled = busy;
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
      headers: { Authorization: `Bearer ${token}` },
    });
    return response.ok;
  } catch {
    return false;
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
  if (!token) {
    linkStatus.textContent = "未ペアリング。アプリの設定に出るコードを貼ってください";
    return;
  }
  const reachable = await probe(token);
  linkStatus.textContent = reachable
    ? "ペアリング済み。アプリは起動しています"
    : "ペアリング済み。アプリは起動していません";
  if (!reachable) return;
  try {
    const response = await fetch(`${LOCAL_BASE}/route-downloads`, {
      method: "GET",
      credentials: "omit",
      headers: { Authorization: `Bearer ${token}` },
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
      headers: { Authorization: `Bearer ${token}` },
    });
  } catch {
    throw unreachableError();
  }
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(data.error || `取り込み済みの確認ができませんでした (${response.status})`);
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
  throw new Error("アプリを起動できませんでした。更新モードは、取り込み済みかをアプリに確認します");
}

async function postImport(token, payload) {
  let response;
  try {
    response = await fetch(`${LOCAL_BASE}/import`, {
      method: "POST",
      credentials: "omit",
      headers: {
        Authorization: `Bearer ${token}`,
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
  return "アプリを起動できませんでした。JSONを保存したので、アプリの取り込みへドロップしてください";
}

function sentText(report, products) {
  const parts = [
    `${products.length} 件を送りました。新規 ${report.inserted ?? 0}、更新 ${report.updated ?? 0}、変更なし ${report.unchanged ?? 0}`,
  ];
  if (report.skipped) parts.push(`スキップ ${report.skipped}`);
  if (Array.isArray(report.warnings) && report.warnings.length > 0) parts.push(report.warnings[0]);
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
        setStatus("更新モードにはペアリングコードが必要です。アプリの設定のコードを貼ってください");
        return;
      }
      knownIds = await knownIdsWithWake(token);
    }
    const collected = await readLibraryPages(tab.url, knownIds);
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
    const stopNote = updateMode && collected.stopped ? "。取り込み済みの商品があったので、そこまでで止めました" : "";
    setStatus(`${await deliver(products, source)}（${collected.lastPage} ページ分）${stopNote}`);
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

allButton.addEventListener("click", () => sendLibrary(false));
newButton.addEventListener("click", () => sendLibrary(true));

saveTokenButton.addEventListener("click", async () => {
  await chrome.storage.local.set({ pairingToken: tokenInput.value.trim() });
  await refreshLink();
});

openLibraryButton.addEventListener("click", () => {
  chrome.tabs.create({ url: LIBRARY_URL });
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
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ enabled }),
    });
    if (!response.ok) {
      setStatus("振り分け設定をアプリへ保存できませんでした");
    }
  } catch {
    setStatus("アプリが起動していないので、拡張側だけ覚えました");
  }
});

refreshLink();
