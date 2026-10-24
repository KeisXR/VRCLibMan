import { convertFileSrc } from "@tauri-apps/api/core";
import { isTauri } from "./api";

export function mediaSrc(path: string | null, url: string | null): string | null {
  if (path && isTauri()) {
    return convertFileSrc(path);
  }
  return url;
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
  unknown: "不明",
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

export function confidenceLabel(value: number | null): string {
  if (value == null) return "未抽出";
  return `${Math.round(value * 100)}%`;
}
