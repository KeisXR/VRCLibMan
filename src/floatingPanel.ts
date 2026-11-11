/**
 * Geometry of the floating right panel. Windows narrower than UI_PANEL_OVERLAY_PX have no room for
 * the panel column, so the panel floats over the page as a small window inside the app window.
 * The user moves it by its tab strip and resizes it from its edges. Rectangles are CSS pixels of
 * the app viewport and always stay whole and fully on screen, because a docked browser page (a
 * native view) is placed at the same rectangle and must not leave the window.
 */

export interface FloatRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Edges that resize the panel. The top edge is the tab strip, which moves it instead. */
export type FloatEdge = "e" | "w" | "s" | "se" | "sw";

export const FLOAT_PANEL_EDGES: readonly FloatEdge[] = ["e", "w", "s", "se", "sw"];

/** Gap kept between the panel and the window edges. */
export const FLOAT_PANEL_MARGIN = 8;
/** Gap of the first placement, matching the old overlay's padding. */
export const FLOAT_PANEL_INSET = 16;
export const FLOAT_PANEL_SIZE = {
  minWidth: 260,
  minHeight: 220,
  defaultWidth: 420,
  defaultHeight: 720,
} as const;
/** Arrow keys on the grip move (or with Shift, resize) by this much. */
export const FLOAT_PANEL_STEP = 16;

function finite(value: number, fallback: number) {
  return Number.isFinite(value) ? value : fallback;
}

/** Largest size that fits the viewport, never below 0 for windows smaller than the minimum. */
function room(viewWidth: number, viewHeight: number) {
  return {
    width: Math.max(0, Math.floor(viewWidth) - 2 * FLOAT_PANEL_MARGIN),
    height: Math.max(0, Math.floor(viewHeight) - 2 * FLOAT_PANEL_MARGIN),
  };
}

/**
 * Keeps the panel whole, at least the minimum size where the window allows it, and inside the
 * viewport with FLOAT_PANEL_MARGIN on every side.
 */
export function clampFloatRect(rect: FloatRect, viewWidth: number, viewHeight: number): FloatRect {
  const max = room(viewWidth, viewHeight);
  const width = Math.min(max.width, Math.max(FLOAT_PANEL_SIZE.minWidth, Math.round(finite(rect.width, FLOAT_PANEL_SIZE.defaultWidth))));
  const height = Math.min(max.height, Math.max(FLOAT_PANEL_SIZE.minHeight, Math.round(finite(rect.height, FLOAT_PANEL_SIZE.defaultHeight))));
  const maxLeft = FLOAT_PANEL_MARGIN + max.width - width;
  const maxTop = FLOAT_PANEL_MARGIN + max.height - height;
  const left = Math.min(maxLeft, Math.max(FLOAT_PANEL_MARGIN, Math.round(finite(rect.left, maxLeft))));
  const top = Math.min(maxTop, Math.max(FLOAT_PANEL_MARGIN, Math.round(finite(rect.top, FLOAT_PANEL_MARGIN))));
  return { left, top, width, height };
}

/** The first placement: the top-right corner, where the old overlay showed the panel. */
export function defaultFloatRect(viewWidth: number, viewHeight: number): FloatRect {
  const width = Math.min(FLOAT_PANEL_SIZE.defaultWidth, Math.floor(viewWidth) - 2 * FLOAT_PANEL_INSET);
  const height = Math.min(FLOAT_PANEL_SIZE.defaultHeight, Math.floor(viewHeight) - 2 * FLOAT_PANEL_INSET);
  return clampFloatRect(
    { left: Math.floor(viewWidth) - FLOAT_PANEL_INSET - width, top: FLOAT_PANEL_INSET, width, height },
    viewWidth,
    viewHeight,
  );
}

/** Where the panel shows: the user's placement kept on screen, or the default one. */
export function placeFloatRect(placed: FloatRect | null, viewWidth: number, viewHeight: number): FloatRect {
  return placed ? clampFloatRect(placed, viewWidth, viewHeight) : defaultFloatRect(viewWidth, viewHeight);
}

/** The panel while the preview has nothing to show yet. */
export const FLOAT_PANEL_COMPACT = { width: 320, height: 136 } as const;

/**
 * The preview with no product selected: a small note in the top-right corner, where the panel first
 * opens, so it does not cover the list the user is about to pick from and grows from the same
 * corner once a product is selected. Not movable or resizable; the user's own placement comes back
 * as soon as a product is selected.
 */
export function compactFloatRect(viewWidth: number, viewHeight: number): FloatRect {
  const max = room(viewWidth, viewHeight);
  const width = Math.min(max.width, FLOAT_PANEL_COMPACT.width);
  const height = Math.min(max.height, FLOAT_PANEL_COMPACT.height);
  // The usual 16px from the edges, down to the 8px margin when the window is barely larger.
  const inset = (span: number, size: number) =>
    Math.max(FLOAT_PANEL_MARGIN, Math.min(FLOAT_PANEL_INSET, Math.floor((Math.floor(span) - size) / 2)));
  return { left: Math.floor(viewWidth) - inset(viewWidth, width) - width, top: inset(viewHeight, height), width, height };
}

/** Moves the panel by the pointer's offset from where the drag started. */
export function moveFloatRect(start: FloatRect, dx: number, dy: number, viewWidth: number, viewHeight: number): FloatRect {
  return clampFloatRect({ ...start, left: start.left + dx, top: start.top + dy }, viewWidth, viewHeight);
}

/**
 * Resizes from an edge. The opposite edge stays where it was: dragging the left edge keeps the
 * right edge still, also when the minimum width or the window edge stops the drag.
 */
export function resizeFloatRect(
  start: FloatRect,
  edge: FloatEdge,
  dx: number,
  dy: number,
  viewWidth: number,
  viewHeight: number,
): FloatRect {
  const max = room(viewWidth, viewHeight);
  const right = start.left + start.width;
  let { left, width, height } = start;
  if (edge === "e" || edge === "se") {
    width = Math.min(FLOAT_PANEL_MARGIN + max.width - start.left, Math.max(FLOAT_PANEL_SIZE.minWidth, start.width + dx));
  }
  if (edge === "w" || edge === "sw") {
    left = Math.max(FLOAT_PANEL_MARGIN, Math.min(right - FLOAT_PANEL_SIZE.minWidth, start.left + dx));
    width = right - left;
  }
  if (edge === "s" || edge === "se" || edge === "sw") {
    height = Math.min(FLOAT_PANEL_MARGIN + max.height - start.top, Math.max(FLOAT_PANEL_SIZE.minHeight, start.height + dy));
  }
  return clampFloatRect({ left, top: start.top, width, height }, viewWidth, viewHeight);
}

/** Class on <body> while the panel moves or resizes; the docked page hides until it settles. */
export const FLOAT_PANEL_MOVING_CLASS = "right-panel-moving";
/** Window event sent after the panel lands somewhere new, so a docked page follows at once. */
export const FLOAT_PANEL_MOVED_EVENT = "right-panel-moved";
