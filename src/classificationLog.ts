import { CATEGORY_LABEL } from "./types";
import type { ClassificationLink, UnresolvedNameCount } from "./types";

const KIND_LABEL: Record<string, string> = {
  import: "BOOTH 取り込み",
  reclassify: "再分類",
  catalog: "対応表の再適用",
  edit: "手動編集後の再判定",
  startup: "起動時の整理",
  booth_detail: "商品情報の更新",
  undo: "元に戻す",
};

const CLASSIFIER_LABEL: Record<string, string> = {
  catalog: "対応表",
  apple: "Apple",
  ollama: "Ollama",
  cloud: "クラウド",
};

const TARGET_LABEL: Record<string, string> = {
  unclassified: "未分類",
  stale: "分類規則の更新分",
  all: "すべて",
};

/** 知らない値は、古い版が書いた記録でも読めるよう、そのまま出す。 */
export function runKindLabel(kind: string): string {
  return KIND_LABEL[kind] ?? kind;
}

export function classifierLabel(classifier: string): string {
  return CLASSIFIER_LABEL[classifier] ?? classifier;
}

export function runNoteLabel(kind: string, note: string | null): string {
  if (!note) return "";
  return kind === "reclassify" ? (TARGET_LABEL[note] ?? note) : note;
}

export function categoryName(category: string | null): string {
  return CATEGORY_LABEL[category ?? "uncategorized"] ?? category ?? "未分類";
}

export function linkNames(links: readonly ClassificationLink[]): string {
  return links.length === 0 ? "なし" : links.map((link) => link.name).join("、");
}

export function formatLogTime(iso: string): string {
  const time = new Date(iso);
  if (Number.isNaN(time.getTime())) return iso;
  return time.toLocaleString("ja-JP", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function confidenceText(confidence: number | null): string {
  return confidence == null ? "" : confidence.toFixed(2);
}

/** GitHub の Issue に貼るための一覧。 */
export function unresolvedListText(names: readonly UnresolvedNameCount[]): string {
  return names.map((item) => `- ${item.name}（${item.productCount} 件）`).join("\n");
}

export function classificationLogHref(boothId?: string): string {
  return boothId ? `#/classification-log/${encodeURIComponent(boothId)}` : "#/classification-log";
}
