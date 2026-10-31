import type { UnityPackageList, UnityStatus } from "./types";

/**
 * 「Unityで開く」の純粋な判断。zip は展開せずに一覧し、選んだものだけを取り出して
 * 設定の既定 Unity に `-openfile` で 1 件ずつ渡す（ダブルクリックと同じ仕組み）。
 */

export function isUnityOpenableName(name: string): boolean {
  return /\.(zip|unitypackage)$/i.test(name.trim());
}

/** 設定の既定 Unity が使えるか。 */
export function unityEditorReady(status: Pick<UnityStatus, "selectedPath" | "selectedMissing"> | null): boolean {
  return Boolean(status && status.selectedPath.trim() && !status.selectedMissing);
}

export type UnityOpenPlan =
  /** Unity パッケージが無い。 */
  | { kind: "none" }
  /** 1 件で、既定の Unity もある。ダイアログを出さずに開く。 */
  | { kind: "direct"; entries: string[] }
  /** 2 件以上。選ぶダイアログを出す。 */
  | { kind: "choose" }
  /** 1 件だが既定の Unity が無い。設定への誘導と関連付けで開く案内を出す。 */
  | { kind: "setup"; entries: string[] };

export function unityOpenPlan(list: Pick<UnityPackageList, "packages">, ready: boolean): UnityOpenPlan {
  const count = list.packages.length;
  if (count === 0) return { kind: "none" };
  if (count >= 2) return { kind: "choose" };
  const entries = [list.packages[0]!.entry];
  return ready ? { kind: "direct", entries } : { kind: "setup", entries };
}

/** チェックした項目を、一覧の順に並べる。 */
export function selectedEntries(list: Pick<UnityPackageList, "packages">, checked: ReadonlySet<string>): string[] {
  return list.packages.filter((item) => checked.has(item.entry)).map((item) => item.entry);
}

/** 一覧の中の 1 件だけに絞る（zip の中の 1 件を指したとき）。見つからなければ null。 */
export function onlyEntry<T extends Pick<UnityPackageList, "packages">>(list: T, entry: string): T | null {
  const found = list.packages.filter((item) => item.entry === entry);
  return found.length ? { ...list, packages: found } : null;
}

const SEPARATORS = /[\\/]+/;

function parts(path: string): string[] {
  return path.trim().split(SEPARATORS).filter(Boolean);
}

/** `path` が `root` の中か。Windows の大文字小文字と区切り文字の違いは無視する。 */
export function isInsideRoot(path: string, root: string | null | undefined): boolean {
  if (!root || !root.trim()) return false;
  const inner = parts(path).map((part) => part.toLowerCase());
  const outer = parts(root).map((part) => part.toLowerCase());
  if (outer.length === 0 || inner.length <= outer.length) return false;
  if (path.trim().startsWith("\\\\") !== root.trim().startsWith("\\\\")) return false;
  return outer.every((part, index) => inner[index] === part);
}

/** パスの中の商品 ID らしいフォルダ名（4〜12 桁の数字）。取り出し先のフォルダ名に使う。 */
export function boothIdFromPath(path: string): string | null {
  const folders = parts(path).slice(0, -1);
  for (let index = folders.length - 1; index >= 0; index -= 1) {
    if (/^\d{4,12}$/.test(folders[index]!)) return folders[index]!;
  }
  return null;
}

export function launchedMessage(count: number, mode: "unity" | "system"): string {
  return mode === "unity"
    ? `Unity に ${count} 件渡しました。Unity の Import 画面で確認してください`
    : `関連付けで ${count} 件開きました。Unity の Import 画面で確認してください`;
}

export const UNITY_NOT_SET =
  "既定の Unity が設定されていません。設定の「Unity」で選ぶと、その Unity で開きます。今回は OS の関連付けでも開けます。";
export const UNITY_MISSING =
  "設定した Unity が見つかりません。設定の「Unity」で選び直してください。今回は OS の関連付けでも開けます。";
