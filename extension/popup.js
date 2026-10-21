const status = document.querySelector("#status");
const button = document.querySelector("#export");

function setStatus(text) {
  status.textContent = text;
}

async function readTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id || !tab.url || !/^https:\/\/([^/]+\.)?booth\.pm\//.test(tab.url)) {
    throw new Error("Booth の商品ページかライブラリを開いてから実行してください");
  }
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

function needsDetail(product) {
  return (product.descriptionSnippet || "").length < 200 || !product.shop;
}

async function enrich(products) {
  const targets = products.filter(needsDetail).slice(0, 40);
  for (let index = 0; index < targets.length; index += 1) {
    const product = targets[index];
    setStatus(`説明文を補完しています ${index + 1}/${targets.length}`);
    try {
      const response = await fetch(product.url, { credentials: "omit" });
      if (response.ok) {
        const html = await response.text();
        const doc = new DOMParser().parseFromString(html, "text/html");
        const [item] = globalThis.VrcLibManExtract.extractPage(doc, product.url);
        globalThis.VrcLibManExtract.mergeDetail(product, item);
      }
    } catch {
      // 公開ページを取れない商品は、見えている情報のまま残す。
    }
    if (needsDetail(product)) {
      try {
        const response = await fetch(`https://booth.pm/ja/items/${product.boothId}.json`, {
          credentials: "omit",
          headers: { Accept: "application/json" },
        });
        if (response.ok) {
          const data = await response.json();
          const detail = globalThis.VrcLibManExtract.detailFromItemJson(data, product.boothId);
          globalThis.VrcLibManExtract.mergeDetail(product, detail);
        }
      } catch {
        // JSON も取れない商品は、見えている情報のまま残す。
      }
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
}

button.addEventListener("click", async () => {
  button.disabled = true;
  try {
    setStatus("ページを読んでいます");
    const products = await readTab();
    if (products.length === 0) {
      setStatus("このページから商品を見つけられませんでした");
      return;
    }
    await enrich(products);
    const payload = {
      format: "vrclibman.booth-export",
      version: 1,
      exportedAt: new Date().toISOString(),
      source: "current-page",
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
    const missing = products.filter((product) => !(product.descriptionSnippet || "").trim() || !product.shop).length;
    setStatus(
      missing > 0
        ? `${products.length} 件を書き出しました（このページのみ）。ショップまたは説明が空の商品が ${missing} 件あります`
        : `${products.length} 件を書き出しました（このページのみ）`,
    );
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error));
  } finally {
    button.disabled = false;
  }
});
