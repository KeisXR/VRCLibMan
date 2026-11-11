/**
 * 「分類を編集」の対応アバターの候補。名前と別名を比べ、一致しないアバターは出さない。
 * 名前の一部が一致するもの（close）と、文字の並びが似ているだけのもの（loose）を分けて返し、
 * 画面は loose を「一致は不確か」として別に見せる。
 */
export interface AvatarHit {
  id: string;
  displayName: string;
  /** 表示名と別名。小文字にしてある。 */
  names: string[];
}

export interface RankedAvatars {
  close: AvatarHit[];
  loose: AvatarHit[];
}

/** 完全一致・前方一致・部分一致はこれ以上。 */
export const CLOSE_SCORE = 0.75;
/** 文字の並び（2 文字ずつ）の重なりがこれ未満の候補は出さない。 */
export const LOOSE_SCORE = 0.3;

export function avatarHit(avatar: { id: string; displayName: string; aliases: string[] }): AvatarHit {
  return {
    id: avatar.id,
    displayName: avatar.displayName,
    names: [avatar.displayName, ...avatar.aliases].map((name) => name.trim().toLowerCase()).filter(Boolean),
  };
}

function bigrams(text: string): string[] {
  const chars = [...text];
  const grams: string[] = [];
  for (let index = 0; index < chars.length - 1; index += 1) grams.push(chars[index] + chars[index + 1]);
  return grams;
}

export function nameScore(query: string, name: string): number {
  if (!query || !name) return 0;
  if (name === query) return 1;
  if (name.startsWith(query) || query.startsWith(name)) return 0.92;
  if (name.includes(query) || query.includes(name)) return CLOSE_SCORE;
  const left = bigrams(query);
  const right = bigrams(name);
  if (left.length === 0 || right.length === 0) return 0;
  const counts = new Map<string, number>();
  for (const gram of right) counts.set(gram, (counts.get(gram) ?? 0) + 1);
  let overlap = 0;
  for (const gram of left) {
    const have = counts.get(gram) ?? 0;
    if (have > 0) {
      overlap += 1;
      counts.set(gram, have - 1);
    }
  }
  // Below the containment score: a similar spelling is never shown as a sure match.
  return Math.min(CLOSE_SCORE - 0.01, (2 * overlap) / (left.length + right.length));
}

export function rankAvatars(index: readonly AvatarHit[], query: string, limit: number): RankedAvatars {
  const needle = query.trim().toLowerCase();
  if (!needle) return { close: [], loose: [] };
  const ranked = index
    .map((hit) => ({ hit, score: Math.max(0, ...hit.names.map((name) => nameScore(needle, name))) }))
    .filter((item) => item.score >= LOOSE_SCORE);
  ranked.sort((left, right) => right.score - left.score || left.hit.displayName.localeCompare(right.hit.displayName, "ja"));
  const top = ranked.slice(0, limit);
  return {
    close: top.filter((item) => item.score >= CLOSE_SCORE).map((item) => item.hit),
    loose: top.filter((item) => item.score < CLOSE_SCORE).map((item) => item.hit),
  };
}

