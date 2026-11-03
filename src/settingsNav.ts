/** 設定画面のカテゴリ一覧。並びは画面の節の並びと同じにする。id は他の画面からの移動先でもあるので変えない。 */
export const SETTINGS_NAV_ITEMS: ReadonlyArray<{ id: string; label: string }> = [
  { id: "settings-library", label: "ライブラリ" },
  { id: "settings-unity", label: "Unity" },
  { id: "settings-startup", label: "起動と常駐" },
  { id: "settings-asset-updates", label: "更新候補" },
  { id: "settings-booth", label: "BOOTH" },
  { id: "settings-classify", label: "分類モデル" },
  { id: "settings-data", label: "データ" },
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
