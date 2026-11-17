import { convertFileSrc } from "@tauri-apps/api/core";
import { isTauri } from "./api";

/**
 * 保存していない画像は BOOTH の画像サーバーからだけ読む。URL は読み込んだ商品情報に書かれた値なので、
 * 別のサーバーを指していても接続しない（`tauri.conf.json` の CSP の `img-src` と同じ範囲）。
 */
export function remoteImageUrl(url: string | null): string | null {
  if (!url) return null;
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:" || parsed.username || parsed.password || parsed.port) return null;
    const host = parsed.hostname;
    const allowed =
      host === "booth.pm" || host.endsWith(".booth.pm") || host === "pximg.net" || host.endsWith(".pximg.net");
    return allowed ? parsed.toString() : null;
  } catch {
    return null;
  }
}

export function mediaSrc(path: string | null, url: string | null): string | null {
  if (path && isTauri()) {
    return convertFileSrc(path);
  }
  // ブラウザ確認用の見本（Tauri の外）は、見本データの画像をそのまま出す。
  return isTauri() ? remoteImageUrl(url) : url;
}

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(0)} KB`;
  if (size < 1024 * 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)} MB`;
  return `${(size / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

export type FilePresence = "local" | "partial" | "remote" | "missing" | "unknown";

export const FILE_PRESENCE_LABEL: Record<FilePresence, string> = {
  local: "ローカル",
  partial: "一部あり",
  remote: "リモート",
  missing: "未ダウンロード",
  unknown: "ファイル情報なし",
};

export function filePresence(
  downloads: { state: "local" | "remote" | "missing" }[],
  localFileCount: number,
): FilePresence {
  if (downloads.length === 0 && localFileCount === 0) return "unknown";
  const local = downloads.filter((file) => file.state === "local").length;
  const remote = downloads.filter((file) => file.state === "remote").length;
  const missing = downloads.filter((file) => file.state === "missing").length;
  const hasLocal = local > 0 || localFileCount > 0;
  const hasNonLocal = remote > 0 || missing > 0;
  if (hasLocal && hasNonLocal) return "partial";
  if (hasLocal) return "local";
  if (remote > 0 && missing > 0) return "partial";
  if (remote > 0) return "remote";
  return "missing";
}
