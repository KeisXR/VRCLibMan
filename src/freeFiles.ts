import type { ItemView } from "./types";

/**
 * ファイル情報が 1 件も無い商品に出す操作。内蔵ブラウザが無効のときは出さない
 * （ダウンロードは内蔵ブラウザの 1 件ずつの経路を使うため）。
 * - `purchased`: 購入履歴にある商品（贈ったギフトを除く）。「購入したファイルを読み込む」で
 *   BOOTH のライブラリからその商品のファイル一覧を読む。購入完了ページから登録した商品は
 *   ファイル情報が無いので、無料配布を探しても見つからないため。
 * - `free`: それ以外。「無料配布のファイルを探す」で BOOTH の公開情報を 1 回だけ読み、
 *   無料配布のファイルだけを行として加える。非公開の商品は公開情報を読めないので出さない。
 * どちらもダウンロード・購入・ライブラリへの追加はしない。
 */
export type FileCheckKind = "purchased" | "free";

export function fileCheckKind(
  item: Pick<ItemView, "downloads" | "localFileCount" | "unpublished"> & { purchased?: boolean },
  builtinBrowser: boolean,
): FileCheckKind | null {
  if (!builtinBrowser || item.downloads.length > 0 || item.localFileCount > 0) return null;
  if (item.purchased) return "purchased";
  return item.unpublished ? null : "free";
}

/** 「無料配布のファイルを探す」を出すか。購入履歴にある商品には出さない。 */
export function offerFreeFileCheck(
  item: Pick<ItemView, "downloads" | "localFileCount" | "unpublished"> & { purchased?: boolean },
  builtinBrowser: boolean,
): boolean {
  return fileCheckKind(item, builtinBrowser) === "free";
}

export const FREE_FILE_CHECK_LABEL = "無料配布のファイルを探す";
export const FREE_FILE_CHECK_BUSY_LABEL = "探しています…";
export const PURCHASED_FILE_CHECK_LABEL = "購入したファイルを読み込む";
export const PURCHASED_FILE_CHECK_BUSY_LABEL = "読み込んでいます…";

export function fileCheckLabel(kind: FileCheckKind, busy: boolean): string {
  if (kind === "purchased") return busy ? PURCHASED_FILE_CHECK_BUSY_LABEL : PURCHASED_FILE_CHECK_LABEL;
  return busy ? FREE_FILE_CHECK_BUSY_LABEL : FREE_FILE_CHECK_LABEL;
}


/**
 * 押したあとの通知。見つからなかったとき（有料・一覧なし）は失敗ではないので、Rust の理由をそのまま通常の通知で出す。
 */
export function freeFilesResultMessage(report: { files: number; notice?: string | null }): string {
  return report.notice || freeFilesFoundMessage(report.files);
}

/** 「購入したファイルを読み込む」のあと。ライブラリに無かったときは Rust の理由を通常の通知で出す。 */
export function purchasedFilesResultMessage(report: { files: number; notice?: string | null }): string {
  if (report.notice) return report.notice;
  return report.files > 0
    ? `購入したファイルが ${report.files} 件見つかりました。行の「ダウンロード」を押すと 1 件ずつ保存します`
    : "BOOTH のライブラリに、この商品のファイルはありませんでした";
}

export function freeFilesFoundMessage(count: number): string {
  return count > 0
    ? `無料ファイルが ${count} 件見つかりました。行の「ダウンロード」を押すと 1 件ずつ保存します（BOOTH へのログインが必要です）`
    : "無料でダウンロードできるファイルはありません";
}
