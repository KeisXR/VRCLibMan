import { formatBytes } from "./media";
import type { CopyResult } from "./types";

/** 「リモートへコピー」（push）と「ローカルへコピー」（pull）。 */
export type CopyDirection = "push" | "pull";

function size(value: number | null): string {
  return value == null ? "大きさ不明" : formatBytes(value);
}

/** 同じ名前の別のファイルがあったときに、上書きしてよいかを尋ねる文。 */
export function conflictQuestion(direction: CopyDirection, filename: string, result: CopyResult): string {
  return direction === "push"
    ? `リモートに「${filename}」という別のファイルがあります（リモート ${size(result.existingSize)}、手元 ${size(result.sourceSize)}）。\nリモートのファイルを手元のファイルで置き換えますか？置き換えたファイルは元に戻せません。`
    : `手元のフォルダに「${filename}」という別のファイルがあります（手元 ${size(result.existingSize)}、リモート ${size(result.sourceSize)}）。\n手元のファイルをリモートのファイルで置き換えますか？置き換えたファイルは元に戻せません。`;
}

export function copyDoneMessage(direction: CopyDirection, filename: string, result: CopyResult): string {
  if (result.status === "already") {
    return direction === "push" ? `${filename} はリモートに同じファイルがあります` : `${filename} は手元のフォルダに同じファイルがあります`;
  }
  if (result.status === "conflict") return `${filename} はコピーしませんでした。同じ名前の別のファイルを残しています`;
  return direction === "push" ? `${filename} をリモートへコピーしました` : `${filename} を手元のフォルダへコピーしました`;
}

/**
 * まず上書きなしでコピーし、同じ名前の別のファイルがあれば尋ねてから上書きする。
 * 上書きは利用者が確認した 1 件だけ。戻り値は通知に出す文。
 */
export async function copyWithConfirm(
  direction: CopyDirection,
  filename: string,
  run: (overwrite: boolean) => Promise<CopyResult>,
  confirm: (message: string) => boolean = (message) => window.confirm(message),
): Promise<string> {
  const first = await run(false);
  if (first.status !== "conflict" || !confirm(conflictQuestion(direction, filename, first))) {
    return copyDoneMessage(direction, filename, first);
  }
  return copyDoneMessage(direction, filename, await run(true));
}

