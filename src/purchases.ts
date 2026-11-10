import type { PurchaseOrderView, PurchaseWalkReport } from "./types";

/** 購入履歴の 1 ページの注文数。Rust の `purchases::PAGE_SIZE` と同じ。 */
export const PURCHASE_PAGE_SIZE = 50;
/** 贈ったギフトの印。ラベルとは別に、注文ごとに付く。 */
export const GIFT_LABEL = "ギフト（贈った）";

export function formatYen(value: number | null | undefined): string {
  if (value == null) return "金額不明";
  return `¥${value.toLocaleString("ja-JP")}`;
}

/** BOOTH の表示どおりの日本時間を、変換せずに「2026/11/02 09:30:20」にする。 */
export function formatPurchaseTime(value: string | null | undefined): string {
  const match = value?.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})/);
  if (!match) return "";
  return `${match[1]}/${match[2]}/${match[3]} ${match[4]}:${match[5]}:${match[6]}`;
}

/** 最後に読み込んだ日時（UTC で保存）を、この端末の時刻で出す。 */
export function formatReadAt(value: string | null | undefined): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString("ja-JP", { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });
}

export type GiftChoice = "auto" | "gift" | "self";

export function giftChoice(order: Pick<PurchaseOrderView, "giftOverride">): GiftChoice {
  if (order.giftOverride == null) return "auto";
  return order.giftOverride ? "gift" : "self";
}

export function giftValue(choice: GiftChoice): boolean | null {
  if (choice === "auto") return null;
  return choice === "gift";
}

export type PurchaseView = "history" | "summary";

export function purchasesHref(view: PurchaseView, group?: string): string {
  if (view === "summary") return "#/purchases/summary";
  return group ? `#/purchases/group/${encodeURIComponent(group)}` : "#/purchases";
}

export function walkSummary(report: PurchaseWalkReport): string {
  const parts = [`新しい注文 ${report.added} 件`, `詳細 ${report.details} 件`];
  if (report.removed > 0) parts.push(`キャンセルなどで外した注文 ${report.removed} 件`);
  return parts.join("、");
}
