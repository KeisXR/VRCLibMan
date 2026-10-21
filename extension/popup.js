const status = document.querySelector("#status");
const button = document.querySelector("#export");
const allButton = document.querySelector("#export-all");
const api = () => globalThis.VrcLibManExtract;

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

async function readLibraryPages(startUrl) {
  const firstUrl = api().libraryPageUrl(startUrl, 1);
  setStatus("ライブラリ 1 ページ目を読んでいます");
  const firstResponse = await fetchWithBackoff(firstUrl, { credentials: "include" });
  if (!firstResponse.ok) {
    throw new Error(`ライブラリを読めませんでした (${firstResponse.status})`);
  }
  const first = parseHtml(await firstResponse.text(), firstUrl);
  let products = api().dedupeProducts(first.products);
  const lastPage = api().libraryLastPage(first.doc, firstUrl);
  try {
    for (let page = 2; page <= lastPage; page += 1) {
      setStatus(`ライブラリ ${page}/${lastPage} ページを読んでいます（${products.length} 件）`);
      const pageUrl = api().libraryPageUrl(startUrl, page);
      const response = await fetchWithBackoff(pageUrl, { credentials: "include" });
      if (!response.ok) {
        throw new Error(`ライブラリ ${page} ページ目を読めませんでした (${response.status})`);
      }
      const parsed = parseHtml(await response.text(), pageUrl);
      if (parsed.products.length === 0) break;
      products = api().dedupeProducts(products.concat(parsed.products));
    }
  } catch (error) {
    error.partialProducts = products;
    error.libraryPages = lastPage;
    throw error;
  }
  return { products, lastPage };
}

function needsDetail(product) {
  return (product.descriptionSnippet || "").length < 200 || !product.shop;
}

async function enrich(products) {
  const targets = products.filter(needsDetail);
  for (let index = 0; index < targets.length; index += 1) {
    const product = targets[index];
    setStatus(`説明文を補完しています ${index + 1}/${targets.length} 件`);
    try {
      const response = await fetchWithBackoff(product.url, { credentials: "omit" });
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
        if (response.ok) {
          const detail = api().detailFromItemJson(await response.json(), product.boothId);
          api().mergeDetail(product, detail);
        }
      } catch (error) {
        if (error instanceof Error && error.message.includes("止めました")) throw error;
      }
    }
    await sleep(250);
  }
}

function setBusy(busy) {
  button.disabled = busy;
  allButton.disabled = busy;
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

function finishMessage(products, scope) {
  const missing = products.filter((product) => !(product.descriptionSnippet || "").trim() || !product.shop).length;
  const base = `${products.length} 件を書き出しました（${scope}）`;
  return missing > 0 ? `${base}。ショップまたは説明が空の商品が ${missing} 件あります` : base;
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
    await download(products, "current-page");
    setStatus(finishMessage(products, "このページのみ"));
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error));
  } finally {
    setBusy(false);
  }
});

allButton.addEventListener("click", async () => {
  setBusy(true);
  let products = [];
  try {
    const tab = await currentTab();
    if (!api().isLibraryUrl(tab.url)) {
      setStatus("ライブラリのページを開いてから実行してください");
      return;
    }
    const collected = await readLibraryPages(tab.url);
    products = collected.products;
    if (products.length === 0) {
      setStatus("ライブラリから商品を見つけられませんでした");
      return;
    }
    try {
      await enrich(products);
    } catch (error) {
      await download(products, "library");
      const reason = error instanceof Error ? error.message : String(error);
      setStatus(`${products.length} 件まで書き出しました。${reason}`);
      return;
    }
    await download(products, "library");
    setStatus(finishMessage(products, `${collected.lastPage} ページ分`));
  } catch (error) {
    const partial = error instanceof Error && Array.isArray(error.partialProducts) ? error.partialProducts : products;
    const reason = error instanceof Error ? error.message : String(error);
    if (partial.length > 0) {
      try {
        await download(partial, "library");
      } catch {
        setStatus(reason);
        return;
      }
      setStatus(`${partial.length} 件まで書き出しました。${reason}`);
      return;
    }
    setStatus(reason);
  } finally {
    setBusy(false);
  }
});
