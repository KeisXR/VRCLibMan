/**
 * 横に並んだ帯（設定のカテゴリ、右パネルのタブ）があふれたときの共通の計算。
 */

/**
 * 帯のどちら側に、隠れた項目が続いているか。横の帯でない（`horizontal` が false）か、
 * あふれていない帯は両方 false。1px 未満の端数は続きとみなさない。
 */
export function overflowEdges(
  horizontal: boolean,
  box: { clientWidth: number; scrollWidth: number; scrollLeft: number },
): { start: boolean; end: boolean } {
  if (!horizontal || box.scrollWidth <= box.clientWidth + 1) return { start: false, end: false };
  return {
    start: box.scrollLeft > 1,
    end: box.scrollLeft + box.clientWidth < box.scrollWidth - 1,
  };
}

/**
 * あふれた帯の上でホイールを回したときに、帯を横へ動かす量（px）。
 * 帯があふれていないとき、Ctrl（ピンチ・拡大縮小）のときは null を返し、ページの縦スクロールや拡大に任せる。
 * 縦と横のうち大きいほうの量を使うので、マウスホイールの縦回転もトラックパッドの横スワイプも帯の中だけで動く。
 */
export function horizontalWheelDelta(
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
 * 帯の中の項目（帯の左端からの位置 `left`、幅 `width`）を見える位置に入れる scrollLeft。
 * すでに見えていれば null。端に `pad` だけ余白を残す（端のぼかしに隠れないように）。
 */
export function revealScrollLeft(
  item: { left: number; width: number },
  box: { clientWidth: number; scrollLeft: number },
  pad = 16,
): number | null {
  const start = item.left - pad;
  const end = item.left + item.width + pad;
  if (start < box.scrollLeft) return Math.max(0, start);
  if (end > box.scrollLeft + box.clientWidth) return Math.max(0, end - box.clientWidth);
  return null;
}

