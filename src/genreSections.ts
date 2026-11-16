import { GENRE_FREE, GENRE_UNSET, genresForKind } from "./types";
import type { GenreKind } from "./types";

/** One genre heading of a list and its items. `start` is the index of its first item in the whole list. */
export interface GenreSection<T> {
  key: string;
  items: T[];
  start: number;
}

/** The order of the genre headings on the asset and world lists: the genres, then 無料, then 未設定. */
export function browseGenreOrder(kind: GenreKind): readonly string[] {
  return [...genresForKind(kind), GENRE_FREE, GENRE_UNSET];
}

/**
 * Splits a list into genre headings in `order`, keeping the list's own order inside each.
 * Keys missing from `order` come last, in the order they first appear. Empty genres are left out.
 */
export function groupGenreSections<T>(items: readonly T[], keyOf: (item: T) => string, order: readonly string[]): GenreSection<T>[] {
  const groups = new Map<string, T[]>();
  for (const item of items) {
    const key = keyOf(item);
    const group = groups.get(key);
    if (group) group.push(item);
    else groups.set(key, [item]);
  }
  const keys = [...order.filter((key) => groups.has(key)), ...[...groups.keys()].filter((key) => !order.includes(key))];
  let start = 0;
  return keys.map((key) => {
    const section = { key, items: groups.get(key) ?? [], start };
    start += section.items.length;
    return section;
  });
}

/** The element id of a genre heading. One genre list is on screen at a time. */
export function genreSectionId(key: string): string {
  return `genre-section-${key}`;
}

