import { ASSET_GENRES, GENRE_UNSET } from "./types";
import type { AvatarDetail, ItemView } from "./types";

/**
 * Genres of one avatar's items on the avatar detail page. The page groups items by category
 * (衣装・髪…); the genre column narrows them to one asset genre (靴・靴下、頭・耳・しっぽ…).
 * Items whose genre is missing or belongs to worlds count as "ジャンル未設定".
 */
export const AVATAR_GENRES: readonly string[] = ASSET_GENRES;

export function avatarItemGenre(item: Pick<ItemView, "genre">): string {
  return item.genre && AVATAR_GENRES.includes(item.genre) ? item.genre : GENRE_UNSET;
}

/** The genre picked in the route, or null for "すべて" (also for unknown values). */
export function selectedAvatarGenre(genre: string | null | undefined): string | null {
  if (!genre) return null;
  if (genre === GENRE_UNSET) return genre;
  return AVATAR_GENRES.includes(genre) ? genre : null;
}

/** `#/avatar/<id>` for all items, `#/avatar/<id>/genre/<genre>` for one genre. */
export function avatarGenreHref(avatarId: string, genre: string | null): string {
  const base = `#/avatar/${encodeURIComponent(avatarId)}`;
  return genre ? `${base}/genre/${encodeURIComponent(genre)}` : base;
}

export interface AvatarGenreCounts {
  avatarId: string;
  counts: Record<string, number>;
  /** The avatar's items (`対応アイテム`): the category sections and the generic items. */
  total: number;
}

/** Counts the items the page calls 対応アイテム, each product once. */
export function countAvatarGenres(detail: Pick<AvatarDetail, "id" | "sections" | "genericItems">): AvatarGenreCounts {
  const counts: Record<string, number> = {};
  const seen = new Set<string>();
  for (const item of [...detail.sections.flatMap((section) => section.items), ...detail.genericItems]) {
    if (seen.has(item.boothId)) continue;
    seen.add(item.boothId);
    const genre = avatarItemGenre(item);
    counts[genre] = (counts[genre] ?? 0) + 1;
  }
  return { avatarId: detail.id, counts, total: seen.size };
}

export function sameAvatarGenreCounts(a: AvatarGenreCounts | null, b: AvatarGenreCounts | null): boolean {
  if (a === b) return true;
  if (!a || !b || a.avatarId !== b.avatarId || a.total !== b.total) return false;
  const keys = new Set([...Object.keys(a.counts), ...Object.keys(b.counts)]);
  for (const key of keys) if ((a.counts[key] ?? 0) !== (b.counts[key] ?? 0)) return false;
  return true;
}

type ItemLists = Pick<AvatarDetail, "sections" | "genericItems" | "unsetEditionItems" | "unsupportedOutfits">;

/**
 * The item lists of the page narrowed to one genre; null keeps them all. Category sections
 * left empty are dropped. The avatar body itself (`ownedProducts`) is never filtered.
 */
export function filterAvatarItems(detail: ItemLists, genre: string | null): ItemLists {
  if (genre === null) return detail;
  const keep = (items: ItemView[]) => items.filter((item) => avatarItemGenre(item) === genre);
  return {
    sections: detail.sections
      .map((section) => ({ ...section, items: keep(section.items) }))
      .filter((section) => section.items.length > 0),
    genericItems: keep(detail.genericItems),
    unsetEditionItems: keep(detail.unsetEditionItems ?? []),
    unsupportedOutfits: keep(detail.unsupportedOutfits),
  };
}

