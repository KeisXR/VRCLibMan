import { GENRE_LABEL } from "./types";
import type { AvatarCard, ItemView } from "./types";

/**
 * Library-wide search matching. The query is split on whitespace and every token must
 * appear in the item's haystack (AND search), using the same case folding as the
 * per-page filters (`toLocaleLowerCase("ja")`).
 */
export function searchTokens(query: string): string[] {
  return query
    .trim()
    .toLocaleLowerCase("ja")
    .split(/\s+/)
    .filter((token) => token.length > 0);
}

function matchesAll(haystack: string, tokens: string[]): boolean {
  const text = haystack.toLocaleLowerCase("ja");
  return tokens.every((token) => text.includes(token));
}

export function avatarHaystack(card: AvatarCard): string {
  return [card.displayName, card.baseBodyName ?? "", ...card.aliases].join("\n");
}

export function productHaystack(item: ItemView): string {
  return [
    item.title,
    item.shop,
    item.descriptionSnippet,
    item.genre ? (GENRE_LABEL[item.genre] ?? item.genre) : "",
    item.boothId,
  ].join("\n");
}

/** Empty queries match nothing here; the search page renders its hint state instead. */
export function filterAvatarsByQuery(cards: AvatarCard[], query: string): AvatarCard[] {
  const tokens = searchTokens(query);
  if (tokens.length === 0) return [];
  return cards.filter((card) => matchesAll(avatarHaystack(card), tokens));
}

export function filterProductsByQuery(items: ItemView[], query: string): ItemView[] {
  const tokens = searchTokens(query);
  if (tokens.length === 0) return [];
  return items.filter((item) => matchesAll(productHaystack(item), tokens));
}
