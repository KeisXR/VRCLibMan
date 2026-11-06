/** Only these reads may be replayed by Tauri's existing postMessage fallback. */
const RECOVERABLE_READS = new Set([
  "get_settings", "list_knowledge", "integration_status", "catalog_info",
  "plugin:app|version", "browser_state", "browser_panel_snapshot",
]);

export const IPC_TRANSPORT_TIMEOUT_MS = 5000;
export const READ_TIMEOUT_MS = 10000;

function recoverableCommand(input: RequestInfo | URL, init?: RequestInit): string | null {
  if (typeof input !== "string" || init?.method !== "POST") return null;
  let url: URL;
  try { url = new URL(input); } catch { return null; }
  if (!["http:", "https:"].includes(url.protocol) || url.host !== "ipc.localhost" || url.username || url.password) return null;
  let command: string;
  try { command = decodeURIComponent(url.pathname.slice(1)); } catch { return null; }
  if (RECOVERABLE_READS.has(command)) return command;
  // Setting the measured rectangle is idempotent. Never replay navigation, downloads,
  // saves, imports, or arbitrary browser actions after an uncertain response.
  if (command === "browser_action" && typeof init.body === "string") {
    try {
      if (JSON.parse(init.body)?.action?.kind === "panelArea") return command;
    } catch { /* Not a panel report. */ }
  }
  return null;
}

/**
 * WebView2 can leave custom-protocol fetches pending. Reject a bounded, replay-safe
 * request so Tauri selects its built-in postMessage transport, including the invoke key.
 * No external fetches or mutation requests are changed, and no payloads are logged.
 */
export function installIpcRecovery(target: Window, timeoutMs = IPC_TRANSPORT_TIMEOUT_MS): () => void {
  if (!("__TAURI_INTERNALS__" in target) || typeof target.fetch !== "function") return () => {};
  const original = target.fetch;
  const wrapped: typeof fetch = (input, init) => {
    const command = recoverableCommand(input, init);
    if (!command || init?.signal) return original.call(target, input, init);
    const controller = new AbortController();
    return new Promise<Response>((resolve, reject) => {
      const timer = setTimeout(() => {
        console.warn(`[VRCLibMan] IPC transport timeout: ${command}`);
        controller.abort();
        reject(new Error("アプリとの通信が応答しないため、接続方法を切り替えます"));
      }, timeoutMs);
      Promise.resolve().then(() => original.call(target, input, { ...init, signal: controller.signal }))
        .then(resolve, reject).finally(() => clearTimeout(timer));
    });
  };
  target.fetch = wrapped;
  return () => { if (target.fetch === wrapped) target.fetch = original; };
}

/** Bounds UI reads only; timing out must never automatically repeat a mutation. */
export function readWithTimeout<T>(read: () => Promise<T>, label: string, timeoutMs = READ_TIMEOUT_MS): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`${label}の応答がありません。もう一度読み込んでください。`)), timeoutMs);
    Promise.resolve().then(read).then(resolve, reject).finally(() => clearTimeout(timer));
  });
}
