import { ASSET_GENRES, GENRE_UNSET } from "./types";
import type { AvatarDetail, ItemView } from "./types";
import { groupGenreSections } from "./genreSections";
import type { GenreSection } from "./genreSections";

/**
 * Genres of one avatar's items on the avatar detail page. The items the page lists by
 * avatar (its own and the shared body's) are headed by asset genre (衣装、靴・靴下…), and the
 * genre column jumps between those headings. Items whose genre is missing or belongs to worlds
 * go under "ジャンル未設定". 汎用・対応版未設定・未対応の衣装 stay in their own blocks below.
 */
export const AVATAR_GENRES: readonly string[] = ASSET_GENRES;
export const AVATAR_GENRE_ORDER: readonly string[] = [...ASSET_GENRES, GENRE_UNSET];

export function avatarItemGenre(item: Pick<ItemView, "genre">): string {
  return item.genre && AVATAR_GENRES.includes(item.genre) ? item.genre : GENRE_UNSET;
}

/** The avatar's items under genre headings, each product once, in the page's order. */
export function avatarGenreSections(detail: Pick<AvatarDetail, "sections">): GenreSection<ItemView>[] {
  const seen = new Set<string>();
  const items = detail.sections.flatMap((section) => section.items).filter((item) => {
    if (seen.has(item.boothId)) return false;
    seen.add(item.boothId);
    return true;
  });
  return groupGenreSections(items, avatarItemGenre, AVATAR_GENRE_ORDER);
}

export interface AvatarGenreCounts {
  avatarId: string;
  /** Items under each genre heading. */
  counts: Record<string, number>;
  total: number;
}

export function countAvatarGenres(avatarId: string, sections: readonly GenreSection<unknown>[]): AvatarGenreCounts {
  const counts: Record<string, number> = {};
  let total = 0;
  for (const section of sections) {
    counts[section.key] = section.items.length;
    total += section.items.length;
  }
  return { avatarId, counts, total };
}

export function sameAvatarGenreCounts(a: AvatarGenreCounts | null, b: AvatarGenreCounts | null): boolean {
  if (a === b) return true;
  if (!a || !b || a.avatarId !== b.avatarId || a.total !== b.total) return false;
  const keys = new Set([...Object.keys(a.counts), ...Object.keys(b.counts)]);
  for (const key of keys) if ((a.counts[key] ?? 0) !== (b.counts[key] ?? 0)) return false;
  return true;
}

