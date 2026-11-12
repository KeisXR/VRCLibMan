import type { AssetUpdateStatus } from "./types";

/** 設定内の確認対象一覧は、既定で閉じる（長い一覧が設定のスクロールを占領しない）。 */
export function assetUpdateListStartsOpen(_rowCount: number): boolean {
  return false;
}

/** 折り畳み時にも件数と状態が分かる一行。 */
export function assetUpdateListSummary(status: AssetUpdateStatus | null): string {
  if (!status) return "読み込み中…";
  if (status.running) return status.progress ? `確認中… ${status.progress.done} / ${status.progress.total} 件` : "確認中…";
  const total = status.rows.length;
  if (total === 0) return "確認を許可した商品はありません";
  const enabled = status.rows.filter((r) => r.enabled).length;
  const changed = status.rows.filter((r) => r.changes.length > 0).length;
  const errors = status.rows.filter((r) => r.error).length;
  const parts = [`確認対象 ${enabled} 件`];
  if (changed > 0) parts.push(`更新 ${changed} 件`);
  if (errors > 0) parts.push(`確認失敗 ${errors} 件`);
  if (changed === 0 && errors === 0) parts.push("更新なし");
  if (enabled !== total) parts.push(`登録 ${total} 件`);
  return parts.join("・");
}
