export const UI_RAIL_WIDTH = { min: 180, max: 360, default: 232 };
export const UI_GENRE_WIDTH = { min: 160, max: 320, default: 200 };
export const UI_LIST_MIN_WIDTH = 480;
export const UI_GENRE_AUTO_COLLAPSE_PX = 1000;
export const UI_RESIZE_STEP_PX = 16;

export function clampRailWidth(value: number): number {
  return Math.round(Math.min(UI_RAIL_WIDTH.max, Math.max(UI_RAIL_WIDTH.min, value)));
}

export function clampGenreWidth(value: number): number {
  return Math.round(Math.min(UI_GENRE_WIDTH.max, Math.max(UI_GENRE_WIDTH.min, value)));
}

/** Shrink rail and genre proportionally so the list keeps at least UI_LIST_MIN_WIDTH. */
export function fitBrowseWidths(
  windowWidth: number,
  rail: number,
  genre: number,
  genreVisible: boolean,
): { rail: number; genre: number } {
  const separators = genreVisible ? 12 : 6;
  let nextRail = clampRailWidth(rail);
  let nextGenre = genreVisible ? clampGenreWidth(genre) : 0;
  const minTotal = UI_LIST_MIN_WIDTH + nextRail + nextGenre + separators;
  if (windowWidth >= minTotal) return { rail: nextRail, genre: nextGenre };
  const slack = Math.max(0, windowWidth - UI_LIST_MIN_WIDTH - separators);
  if (!genreVisible) {
    return { rail: clampRailWidth(Math.min(nextRail, slack)), genre: 0 };
  }
  const railShare = nextRail / (nextRail + nextGenre);
  nextRail = clampRailWidth(slack * railShare);
  nextGenre = clampGenreWidth(slack - nextRail);
  return { rail: nextRail, genre: nextGenre };
}
