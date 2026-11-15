import { RIGHT_PANEL_SEPARATOR, fitRightPanelWidth, rightPanelMaxWidth } from "./rightPanel";

export const UI_RAIL_WIDTH = { min: 180, max: 360, default: 232 };
/** Width of the icon-only left sidebar. It is fixed and not resizable. */
export const UI_RAIL_COLLAPSED_WIDTH = 60;
export const UI_GENRE_WIDTH = { min: 160, max: 320, default: 200 };
/** Width of the icon-only genre column. It is fixed and not resizable. */
export const UI_GENRE_COLLAPSED_WIDTH = 52;
export const UI_LIST_MIN_WIDTH = 480;
/** Windows this wide or narrower show the left sidebar as icons only, without saving it. */
export const UI_RAIL_AUTO_COLLAPSE_PX = 860;
/** Windows narrower than this show the right panel over the page instead of as a column. */
export const UI_PANEL_OVERLAY_PX = 1000;
/** Grid separator column beside the rail and the genre column. */
export const UI_SEPARATOR_PX = 6;
export const UI_RESIZE_STEP_PX = 16;

export function clampRailWidth(value: number): number {
  return Math.round(Math.min(UI_RAIL_WIDTH.max, Math.max(UI_RAIL_WIDTH.min, value)));
}

export function clampGenreWidth(value: number): number {
  return Math.round(Math.min(UI_GENRE_WIDTH.max, Math.max(UI_GENRE_WIDTH.min, value)));
}

/**
 * Shrink rail and genre proportionally so the list keeps at least UI_LIST_MIN_WIDTH.
 * A collapsed rail keeps its fixed icon width; only the genre column gives way. A collapsed
 * genre column keeps its fixed icon width too; only an open rail gives way.
 */
export function fitBrowseWidths(
  windowWidth: number,
  rail: number,
  genre: number,
  genreVisible: boolean,
  railCollapsed = false,
  genreCollapsed = false,
): { rail: number; genre: number } {
  const separators = genreVisible ? 12 : 6;
  if (genreVisible && genreCollapsed) {
    if (railCollapsed) return { rail: UI_RAIL_COLLAPSED_WIDTH, genre: UI_GENRE_COLLAPSED_WIDTH };
    const room = windowWidth - UI_LIST_MIN_WIDTH - UI_GENRE_COLLAPSED_WIDTH - separators;
    return { rail: clampRailWidth(Math.min(clampRailWidth(rail), room)), genre: UI_GENRE_COLLAPSED_WIDTH };
  }
  if (railCollapsed) {
    if (!genreVisible) return { rail: UI_RAIL_COLLAPSED_WIDTH, genre: 0 };
    const room = windowWidth - UI_LIST_MIN_WIDTH - UI_RAIL_COLLAPSED_WIDTH - separators;
    return { rail: UI_RAIL_COLLAPSED_WIDTH, genre: clampGenreWidth(Math.min(clampGenreWidth(genre), room)) };
  }
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

export interface AppLayoutInput {
  windowWidth: number;
  /** Saved sidebar width (`ui_rail_width`). */
  railWidth: number;
  /** Saved open/closed state (`ui_rail_collapsed`). */
  railCollapsed: boolean;
  /** Saved genre column width (`ui_genre_width`). */
  genreWidth: number;
  /** Saved open/closed state of the genre column (`ui_genre_collapsed`). */
  genreCollapsed?: boolean;
  /** The page has a genre column: assets, worlds, or one avatar's items. */
  browseGenre: boolean;
  panelOpen: boolean;
  panelMaximized: boolean;
  /** Saved right panel width (`preview_pane_width`). */
  panelStoredWidth: number;
}

export interface AppLayout {
  /** The window is too narrow for the open sidebar; it shows icons only for now. */
  railAuto: boolean;
  /** Icons only on screen: saved as closed, or `railAuto`. */
  railCollapsed: boolean;
  rail: number;
  genre: number;
  /** The genre column is shown beside the list. */
  genreVisible: boolean;
  /** The genre column is shown with icons only (saved as closed). */
  genreCollapsed: boolean;
  /** The genre column does not fit; a one-line chip bar above the list replaces it. */
  genreBar: boolean;
  /** The chip bar is shown, but the icon-only column would fit: the bar offers to switch to it. */
  genreCanCollapse: boolean;
  /** The right panel shows over the page (window narrower than `UI_PANEL_OVERLAY_PX`). */
  narrow: boolean;
  panelDocked: boolean;
  panelMaximized: boolean;
  panelWidth: number;
  panelMax: number;
  /** Width left for the list column (CSS px). */
  listWidth: number;
}

/**
 * Columns for the current window. The genre column is shown only when the list keeps
 * `UI_LIST_MIN_WIDTH` beside the rail, the genre column and a docked right panel, all at the
 * widths they would really get; otherwise the page shows the genre chip bar instead.
 * A genre column saved as closed is measured at its icon width.
 * Saved widths and the saved sidebar state are never changed here.
 */
export function computeAppLayout(input: AppLayoutInput): AppLayout {
  const windowWidth = input.windowWidth;
  const railAuto = windowWidth <= UI_RAIL_AUTO_COLLAPSE_PX;
  const railCollapsed = input.railCollapsed || railAuto;
  const genreCollapsed = input.genreCollapsed === true;
  const narrow = windowWidth < UI_PANEL_OVERLAY_PX;
  const panelDocked = input.panelOpen && !narrow;
  const panelMaximized = panelDocked && input.panelMaximized;
  const railColumn = (railCollapsed ? UI_RAIL_COLLAPSED_WIDTH : clampRailWidth(input.railWidth)) + UI_SEPARATOR_PX;

  /** `genre` is null without the genre column, else whether it is the icon-only one. */
  const attempt = (genre: null | "open" | "icons") => {
    const withGenre = genre !== null;
    const genreColumn = genre === "icons" ? UI_GENRE_COLLAPSED_WIDTH : clampGenreWidth(input.genreWidth);
    const leftColumns = withGenre ? railColumn + genreColumn + UI_SEPARATOR_PX : railColumn;
    const panelMax = rightPanelMaxWidth(windowWidth, leftColumns);
    const panelWidth = panelDocked ? fitRightPanelWidth(input.panelStoredWidth, windowWidth, leftColumns) : 0;
    const room = panelDocked && !panelMaximized ? windowWidth - panelWidth - RIGHT_PANEL_SEPARATOR : windowWidth;
    const fitted = fitBrowseWidths(room, input.railWidth, input.genreWidth, withGenre, railCollapsed, genre === "icons");
    const listWidth = room - fitted.rail - UI_SEPARATOR_PX - (withGenre ? fitted.genre + UI_SEPARATOR_PX : 0);
    return { panelMax, panelWidth, fitted, listWidth };
  };

  const base = { railAuto, railCollapsed, narrow, panelDocked, panelMaximized };
  if (input.browseGenre) {
    const wide = attempt(genreCollapsed ? "icons" : "open");
    if (wide.listWidth >= UI_LIST_MIN_WIDTH) {
      return {
        ...base,
        rail: wide.fitted.rail,
        genre: wide.fitted.genre,
        genreVisible: true,
        genreCollapsed,
        genreBar: false,
        genreCanCollapse: false,
        panelWidth: wide.panelWidth,
        panelMax: wide.panelMax,
        listWidth: wide.listWidth,
      };
    }
  }
  const plain = attempt(null);
  const iconsFit = input.browseGenre && !genreCollapsed && attempt("icons").listWidth >= UI_LIST_MIN_WIDTH;
  return {
    ...base,
    rail: plain.fitted.rail,
    genre: 0,
    genreVisible: false,
    genreCollapsed: false,
    genreBar: input.browseGenre,
    genreCanCollapse: iconsFit,
    panelWidth: plain.panelWidth,
    panelMax: plain.panelMax,
    listWidth: plain.listWidth,
  };
}

/**
 * Motion for the left sidebar toggle. The grid width slides on every toggle, also while the
 * right panel is open: the docked page (a native view) follows the viewport rectangle through
 * useDockedArea. When the toggle also moves or resizes the panel itself, `hold` is set and the
 * native view is hidden for the motion (see railMotionHoldsPanel).
 * Returned as data attributes for the app shell (spread into its props).
 */
export type RailMotion = "none" | "slide" | "hold";

export function railMotionAttrs(motion: RailMotion): Record<string, string> {
  if (motion === "none") return {};
  return motion === "hold" ? { "data-rail-motion": "slide", "data-rail-hold": "" } : { "data-rail-motion": "slide" };
}

/**
 * True when a sidebar toggle changes the docked right panel's own rectangle (left or width).
 * The docked panel is the last column, so its rectangle changes only with its width (clamped by
 * fitRightPanelWidth from the columns on its left, e.g. when the genre column flips) or when it
 * is maximized into the middle column. Only then is the native page hidden during the slide.
 * A genre column flip alone is absorbed by the list and does not move the panel.
 */
export function railMotionHoldsPanel(
  before: Pick<AppLayout, "panelDocked" | "panelMaximized" | "panelWidth">,
  after: Pick<AppLayout, "panelDocked" | "panelMaximized" | "panelWidth">,
): boolean {
  if (!before.panelDocked && !after.panelDocked) return false;
  if (before.panelDocked !== after.panelDocked || before.panelMaximized || after.panelMaximized) return true;
  return before.panelWidth !== after.panelWidth;
}
