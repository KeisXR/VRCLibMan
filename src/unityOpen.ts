import type { PackageNeed, UnityInstance, UnityPackageList, UnityStatus } from "./types";

/**
 * 「Unityで開く」の純粋な判断。zip は展開せずに一覧し、選んだものだけを取り出して
 * 設定の既定 Unity に `-openfile` で 1 件ずつ渡す（ダブルクリックと同じ仕組み）。
 */

/** Unity パッケージに包んで渡せる単体のファイル。Rust の `unity_wrap::LOOSE_EXTENSIONS` と同じ並び。 */
const LOOSE = /.\.(mp3|wav|ogg|aif|aiff|png|jpg|jpeg|psd|tga|fbx|blend)$/i;

export function isUnityLooseName(name: string): boolean {
  return LOOSE.test(name.trim());
}

export function isUnityOpenableName(name: string): boolean {
  return /\.(zip|unitypackage)$/i.test(name.trim()) || isUnityLooseName(name);
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
  | { kind: "setup"; entries: string[] }
  /** mp3・png などの単体のファイル。入る場所を見せてから渡す。 */
  | { kind: "loose"; entries: string[] };

export function unityOpenPlan(list: Pick<UnityPackageList, "packages"> & { kind?: UnityPackageList["kind"] }, ready: boolean): UnityOpenPlan {
  const count = list.packages.length;
  if (list.kind === "loose" && count === 1) return { kind: "loose", entries: [list.packages[0]!.entry] };
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

/** 入る場所のうち、フォルダの部分（`Assets/VRCLibMan/<商品名>/`）。 */
export function destinationFolder(destination: string): string {
  const cut = destination.lastIndexOf("/");
  return cut < 0 ? destination : destination.slice(0, cut + 1);
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

/** 渡す先を選ばせるか。Unity が 2 つ以上開いているときだけ選ぶ。0〜1 つならこれまでどおり渡す。 */
/** Rust の `EDITOR_NOT_RUNNING`（unity_import.rs）。設定した Unity のエディタが 1 つも開いていない。 */
export function isUnityNotRunning(message: string): boolean {
  return message.includes("Unity が起動していません");
}

/**
 * 渡す先に足りないもの（段階 3）を文にする部品。VPM パッケージは名前を「、」でつなぎ、
 * ライブラリの商品は題を「」で囲む。どちらも無ければ空文字。
 */
export function missingNeedsText(needs: readonly PackageNeed[]): { packages: string; versions: string; products: string } {
  return {
    packages: needs
      .filter((need) => need.kind === "vpm")
      .map((need) => need.name)
      .join("、"),
    versions: needs
      .filter((need) => need.kind === "vpmVersion")
      .map((need) => need.name)
      .join("、"),
    products: needs
      .filter((need) => need.kind === "product")
      .map((need) => `「${need.name}」`)
      .join("、"),
  };
}

export function needsUnityTarget(instances: readonly unknown[]): boolean {
  return instances.length >= 2;
}

/** 渡す先の一覧で、同じプロジェクト名が並ぶときに見分ける補足。 */
export function unityInstanceDetail(instance: Pick<UnityInstance, "scene" | "version">): string {
  return [instance.scene, instance.version && `Unity ${instance.version}`].filter(Boolean).join(" ・ ");
}
