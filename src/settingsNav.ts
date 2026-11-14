import { horizontalWheelDelta, overflowEdges } from "./scrollEdges";

/** 設定画面のカテゴリ一覧。並びは画面の節の並びと同じにする。id は他の画面からの移動先でもあるので変えない。 */
export const SETTINGS_NAV_ITEMS: ReadonlyArray<{ id: string; label: string }> = [
  { id: "settings-library", label: "ライブラリ" },
  { id: "settings-unity", label: "Unity" },
  { id: "settings-startup", label: "起動と常駐" },
  { id: "settings-design", label: "デザイン" },
  { id: "settings-asset-updates", label: "アセットの更新" },
  { id: "settings-booth", label: "BOOTH" },
  { id: "settings-data", label: "アプリ情報とデータ" },
  { id: "settings-knowledge", label: "対応表を編集" },
  { id: "settings-debug", label: "デバッグ" },
];

/**
 * スクロール位置から「今いるカテゴリ」を決める。
 * `tops` は各節の上端（スクロール領域の上端からの距離、px）で、画面の並び順。
 * 上端が `threshold` より上にある最後の節を選ぶ。末尾までスクロールしたときは最後の節にする。
 */
export function activeSettingsSection(
  tops: ReadonlyArray<{ id: string; top: number }>,
  threshold: number,
  atBottom: boolean,
): string | null {
  if (!tops.length) return null;
  if (atBottom) return tops[tops.length - 1].id;
  let active = tops[0].id;
  for (const { id, top } of tops) {
    if (top <= threshold) active = id;
    else break;
  }
  return active;
}

/** 縦にスクロールする最も近い祖先。無ければ文書全体。 */
export function scrollParent(node: HTMLElement | null): HTMLElement | null {
  if (typeof window.getComputedStyle !== "function") return null;
  for (let current = node?.parentElement ?? null; current; current = current.parentElement) {
    const overflow = window.getComputedStyle(current).overflowY;
    if (overflow === "auto" || overflow === "scroll") return current;
  }
  return null;
}

/** 横にあふれたカテゴリの帯の上でのホイール（`scrollEdges.ts` の `horizontalWheelDelta`）。 */
export const settingsNavWheelDelta = horizontalWheelDelta;

/** 1行の帯のどちら側に続きがあるか（`scrollEdges.ts` の `overflowEdges`）。縦の列では両方 false。 */
export const settingsNavEdges = overflowEdges;
