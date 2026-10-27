import { GENRE_FREE, GENRE_UNSET, genresForKind } from "./types";
import type { GenreKind, ItemView } from "./types";

export function genreHref(basePath: string, genre: string | null): string {
  return genre ? `${basePath}/${encodeURIComponent(genre)}` : basePath;
}

export function filterItems(items: ItemView[], filter: string): ItemView[] {
  const query = filter.trim().toLocaleLowerCase("ja");
  if (!query) return items;
  return items.filter((item) => `${item.title}\n${item.shop}`.toLocaleLowerCase("ja").includes(query));
}

export function selectedGenre(kind: GenreKind, genre: string | null): string | null {
  if (!genre) return null;
  if (genre === GENRE_UNSET || genre === GENRE_FREE) return genre;
  return genresForKind(kind).includes(genre) ? genre : null;
}
