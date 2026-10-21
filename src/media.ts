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

export function confidenceLabel(value: number | null): string {
  if (value == null) return "未抽出";
  return `${Math.round(value * 100)}%`;
}
