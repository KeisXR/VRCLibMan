// サービスワーカーには DOM が無いので、BOOTH のページを読む長い処理はこの見えない文書で動かす。
// ポップアップを閉じても続く。進み具合と結果はサービスワーカーへ送り、保存と送信はあちらが受け持つ。
const shared = globalThis.VrcLibManShared;
let active = null;

function report(message) {
  return chrome.runtime.sendMessage({ source: "offscreen", ...message }).catch(() => {});
}

// offscreen 文書では chrome.runtime.getManifest を使えないので、拡張の版はジョブと一緒に受け取る。
function appRequester(token, version) {
  return async (path, body) => {
    let response;
    try {
      // GET には Origin が付かずアプリが断るので、読み取りも POST で送る（background.js の appRequest と同じ）。
      response = await fetch(`${shared.LOCAL_BASE}${path}`, {
        method: "POST",
        credentials: "omit",
        headers: shared.localHeaders(token, String(version || ""), body !== undefined),
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      throw new Error("アプリに接続できなくなったため、ここまでで止めました");
    }
    const data = await response.json().catch(() => ({}));
    if (!response.ok) throw new Error(data.error || `アプリが受け付けませんでした（エラー ${response.status}）`);
    return data;
  };
}

function walkerFor(job, state) {
  return globalThis.VrcLibManWalker.createWalker({
    fetch: (url, options) => fetch(url, options),
    DOMParser,
    extract: globalThis.VrcLibManExtract,
    purchases: globalThis.VrcLibManPurchases,
    policy: globalThis.VrcLibManRequestPolicy,
    shared,
    intervalMs: job.intervalMs,
    isCancelled: () => state.cancelled,
    progress: (info) => report({ type: "walk-progress", id: job.id, ...info }),
    app: job.token ? appRequester(job.token, job.version) : undefined,
  });
}

async function run(job) {
  const state = { id: job.id, cancelled: false };
  active = state;
  const walker = walkerFor(job, state);
  try {
    if (job.kind === "purchases") {
      const result = await walker.walkPurchases({ complete: job.complete === true });
      await report({ type: "walk-result", id: job.id, kind: job.kind, purchases: result });
      return;
    }
    let products = Array.isArray(job.products) ? job.products : [];
    let pages = 0;
    let complete = false;
    let stopped = false;
    let error = "";
    let wasCancelled = false;
    if (job.kind === "library" || job.kind === "library-new") {
      const known = job.kind === "library-new" ? new Set((job.knownIds || []).map(String)) : null;
      const walked = await walker.walkLibrary({ knownIds: known });
      products = walked.products;
      pages = walked.pages;
      complete = walked.complete;
      stopped = walked.stopped;
      error = walked.error || "";
      wasCancelled = Boolean(walked.cancelled);
    }
    if (products.length > 0 && !wasCancelled) {
      try {
        await walker.enrich(products);
      } catch (enrichError) {
        if (enrichError.cancelled) wasCancelled = true;
        else error = error || enrichError.message;
      }
    }
    await report({ type: "walk-result", id: job.id, kind: job.kind, products, pages, complete: complete && !error && !wasCancelled, stopped, error, cancelled: wasCancelled });
  } catch (error) {
    await report({ type: "walk-result", id: job.id, kind: job.kind, products: [], error: error.message || String(error) });
  } finally {
    if (active === state) active = null;
  }
}

chrome.runtime.onMessage.addListener((message, sender, respond) => {
  if (message?.target !== "offscreen" || sender.id !== chrome.runtime.id) return undefined;
  if (message.type === "walk") {
    if (active) {
      respond({ error: "ほかの読み込みを実行中です" });
      return undefined;
    }
    run(message.job);
    respond({ ok: true });
  } else if (message.type === "walk-cancel") {
    if (active && active.id === message.id) active.cancelled = true;
    respond({ ok: Boolean(active) });
  } else if (message.type === "status") {
    respond({ active: active ? active.id : null });
  } else if (message.type === "order-detail") {
    const state = { cancelled: false };
    walkerFor({ id: "order", intervalMs: message.intervalMs }, state)
      .readOrderDetail(message.orderId)
      .then((value) => respond({ value }), (error) => respond({ error: error.message }));
    return true;
  } else if (message.type === "blob-url") {
    const url = URL.createObjectURL(new Blob([message.json], { type: "application/json" }));
    setTimeout(() => URL.revokeObjectURL(url), 5 * 60 * 1000);
    respond({ url });
  }
  return undefined;
});
