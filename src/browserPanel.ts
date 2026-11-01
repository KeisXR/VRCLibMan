import type { BrowserPanelRect } from "./types";

/**
 * The docked browser is a native view drawn above the app's HTML. Anything that has to show
 * over the right panel (menus, dialogs, popovers, overlays) would be hidden under it, so the
 * view is hidden while one of these overlaps it. Notices in the main column never reach the panel.
 */
export const BROWSER_PANEL_COVER_SELECTORS = [
  '[role="menu"]',
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[aria-modal="true"]',
  "dialog[open]",
  ".account-popover",
  ".product-actions-panel",
  ".avatar-add-panel",
  ".preview-overlay",
  ".setup-screen",
] as const;

export const BROWSER_PANEL_COVER_SELECTOR = BROWSER_PANEL_COVER_SELECTORS.join(",");

/** Smaller than this and the view is hidden instead of drawn as a sliver. */
export const BROWSER_PANEL_MIN_EDGE = 2;

export interface RectLike {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Whole CSS pixels contained inside the viewport, or null when nothing fits. */
export function panelRect(rect: RectLike): BrowserPanelRect | null {
  const values = [rect.left, rect.top, rect.width, rect.height];
  if (!values.every(Number.isFinite)) return null;
  // Native views draw above HTML. Rounding the top down can cover the last row of
  // browser controls; keep every edge inside the measured viewport instead.
  const left = Math.ceil(rect.left);
  const top = Math.ceil(rect.top);
  const width = Math.floor(rect.left + rect.width) - left;
  const height = Math.floor(rect.top + rect.height) - top;
  if (width < BROWSER_PANEL_MIN_EDGE || height < BROWSER_PANEL_MIN_EDGE) return null;
  return { left, top, width, height };
}

/** The two rectangles share some area (touching edges do not count). */
export function rectsOverlap(a: RectLike, b: RectLike): boolean {
  if (a.width <= 0 || a.height <= 0 || b.width <= 0 || b.height <= 0) return false;
  return a.left < b.left + b.width && b.left < a.left + a.width && a.top < b.top + b.height && b.top < a.top + a.height;
}

export interface PanelVisibility {
  /** The right panel shows over the page (narrow window), where the native view cannot fit in. */
  overlay: boolean;
  /** A pane edge or an item is being dragged. */
  dragging: boolean;
  /** Rectangles of menus, dialogs and other layers on screen. */
  covers: RectLike[];
}

/** Whether the native view may be drawn at `area` right now. */
export function browserPanelVisible(area: BrowserPanelRect | null, state: PanelVisibility): boolean {
  const tabDrag = typeof document !== "undefined" && document.body.classList.contains("tab-dragging");
  if (!area || state.overlay || state.dragging || tabDrag) return false;
  return !state.covers.some((cover) => rectsOverlap(area, cover));
}

/** Layers that may cover the panel. The layer that holds the browser panel itself is skipped. */
export function coveringRects(root: ParentNode, viewport: Element): RectLike[] {
  const rects: RectLike[] = [];
  for (const element of Array.from(root.querySelectorAll(BROWSER_PANEL_COVER_SELECTOR))) {
    if (element.contains(viewport)) continue;
    const rect = element.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) rects.push(rect);
  }
  return rects;
}

/** Same rectangle and visibility as the last report, so nothing needs to be sent. */
export function sameReport(
  a: { area: BrowserPanelRect | null; visible: boolean } | null,
  b: { area: BrowserPanelRect | null; visible: boolean },
): boolean {
  if (!a || a.visible !== b.visible) return false;
  if (!a.area || !b.area) return a.area === b.area;
  return a.area.left === b.area.left && a.area.top === b.area.top && a.area.width === b.area.width && a.area.height === b.area.height;
}
