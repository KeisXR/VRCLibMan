import type { ItemView } from "./types";

/**
 * 「ファイルを取得」の純粋な判断。ファイル情報が 1 件も無い商品だけに出し、
 * 内蔵ブラウザが無効のときと、BOOTH で非公開の商品には出さない
 * （ダウンロードは内蔵ブラウザの 1 件ずつの経路を使い、非公開の商品は公開情報を読めないため）。
 * 押すと BOOTH の公開情報を 1 回だけ読み、無料配布のファイルだけを行として加える。
 * ダウンロード・購入・ライブラリへの追加はしない。
 */
export function offerFreeFileCheck(
  item: Pick<ItemView, "downloads" | "localFileCount" | "unpublished">,
  builtinBrowser: boolean,
): boolean {
  return builtinBrowser && !item.unpublished && item.downloads.length === 0 && item.localFileCount === 0;
}

export const FREE_FILE_CHECK_LABEL = "ファイルを取得";
export const FREE_FILE_CHECK_BUSY_LABEL = "取得中…";


export function freeFilesFoundMessage(count: number): string {
  return count > 0
    ? `無料ファイルが ${count} 件見つかりました。行の「ダウンロード」を押すと 1 件ずつ保存します（BOOTH へのログインが必要です）`
    : "無料でダウンロードできるファイルはありません";
}
