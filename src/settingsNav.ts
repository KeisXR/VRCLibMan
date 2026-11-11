/** 設定画面のカテゴリ一覧。並びは画面の節の並びと同じにする。id は他の画面からの移動先でもあるので変えない。 */
export const SETTINGS_NAV_ITEMS: ReadonlyArray<{ id: string; label: string }> = [
  { id: "settings-library", label: "ライブラリ" },
  { id: "settings-unity", label: "Unity" },
  { id: "settings-startup", label: "起動と常駐" },
  { id: "settings-design", label: "デザイン" },
  { id: "settings-asset-updates", label: "更新設定" },
  { id: "settings-booth", label: "BOOTH" },
  { id: "settings-data", label: "バージョン情報" },
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

/**
 * 横にあふれたカテゴリの帯の上でホイールを回したときに、帯を横へ動かす量（px）。
 * 帯があふれていないとき、Ctrl（ピンチ・拡大縮小）のときは null を返し、ページの縦スクロールや拡大に任せる。
 * 縦と横のうち大きいほうの量を使うので、マウスホイールの縦回転もトラックパッドの横スワイプも帯の中だけで動く。
 */
export function settingsNavWheelDelta(
  wheel: { deltaX: number; deltaY: number; deltaMode: number; ctrlKey: boolean },
  box: { clientWidth: number; scrollWidth: number },
): number | null {
  if (wheel.ctrlKey || box.scrollWidth <= box.clientWidth) return null;
  const delta = Math.abs(wheel.deltaX) >= Math.abs(wheel.deltaY) ? wheel.deltaX : wheel.deltaY;
  // deltaMode: 0 = px、1 = 行、2 = ページ。
  const unit = wheel.deltaMode === 1 ? 16 : wheel.deltaMode === 2 ? box.clientWidth : 1;
  return delta * unit;
}

/**
 * 1行の帯のどちら側に、隠れた項目が続いているか。縦の列（`bar` が false）と、あふれていない帯は両方 false。
 * 1px 未満の端数は続きとみなさない。
 */
export function settingsNavEdges(
  bar: boolean,
  box: { clientWidth: number; scrollWidth: number; scrollLeft: number },
): { start: boolean; end: boolean } {
  if (!bar || box.scrollWidth <= box.clientWidth + 1) return { start: false, end: false };
  return {
    start: box.scrollLeft > 1,
    end: box.scrollLeft + box.clientWidth < box.scrollWidth - 1,
  };
}
