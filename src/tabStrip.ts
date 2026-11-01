/** Pointer travel before a tab press becomes a drag. Clicks stay below this. */
export const TAB_DRAG_THRESHOLD_PX = 6;

export function pointerInsideWindow(x: number, y: number, width: number, height: number): boolean {
  return Number.isFinite(x) && Number.isFinite(y) && x >= 0 && y >= 0 && x <= width && y <= height;
}

/** Gap index (0 at the front) for a pointer moving across tabs from left to right. */
export function insertionIndex(pointerX: number, rects: { left: number; width: number }[]): number {
  for (let index = 0; index < rects.length; index += 1) {
    const rect = rects[index]!;
    if (pointerX < rect.left + rect.width / 2) return index;
  }
  return rects.length;
}

export type DragOutcome = "click" | "cancel" | "reorder" | "detach";

/**
 * A drag ends in a reorder while the pointer is inside the window.
 * Leaving the window detaches a page tab. `dragend` alone is not a reason to detach:
 * callers pass `dragEndOnly` for that event and get no detach.
 */
export function dragOutcome(input: {
  moved: boolean;
  cancelled: boolean;
  insideWindow: boolean;
  detachable: boolean;
  /** `dragend` without a pointer release outside the window does not detach. */
  dragEndOnly?: boolean;
}): DragOutcome {
  if (!input.moved) return "click";
  if (input.cancelled || input.dragEndOnly) return "cancel";
  if (input.insideWindow) return "reorder";
  if (input.detachable) return "detach";
  return "cancel";
}
