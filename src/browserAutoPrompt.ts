/**
 * 内蔵ブラウザの「取得した商品の分類」を、今出ている分だけ後回しにするための判断。
 * 登録と pending は残し、ここからジャンルは決めない。
 */

const NONE: readonly string[] = [];

function sameIds(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

/** 今一覧にある商品を後回しにする。一覧から消えた ID は残さない。 */
export function postponeShown(postponedIds: readonly string[], shownIds: readonly string[]): readonly string[] {
  const shown = new Set(shownIds);
  const next: string[] = [];
  for (const id of postponedIds) {
    if (shown.has(id) && !next.includes(id)) next.push(id);
  }
  for (const id of shownIds) {
    if (!next.includes(id)) next.push(id);
  }
  return sameIds(postponedIds, next) ? postponedIds : next;
}

/**
 * 未分類の一覧が更新されたとき。
 * 後回しにしていない商品があれば後回しを解いてダイアログを開き直す。
 * そうでなければ、もう一覧に無い ID だけを落とす。
 */
export function reconcilePostponed(postponedIds: readonly string[], itemIds: readonly string[]): readonly string[] {
  const current = new Set(itemIds);
  const still = postponedIds.filter((id) => current.has(id));
  const hasUnasked = itemIds.some((id) => !still.includes(id));
  if (hasUnasked) return postponedIds.length === 0 ? postponedIds : NONE;
  return sameIds(postponedIds, still) ? postponedIds : still;
}

/** 未分類があり、そのうち後回しにしていない商品がある。 */
export function promptOpen(itemIds: readonly string[], postponedIds: readonly string[]): boolean {
  return itemIds.some((id) => !postponedIds.includes(id));
}

/** 未分類が残っていて、いまはダイアログを出さない。 */
export function showsResume(itemIds: readonly string[], postponedIds: readonly string[]): boolean {
  return itemIds.length > 0 && !promptOpen(itemIds, postponedIds);
}
