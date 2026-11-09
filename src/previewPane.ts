/** Wait until the selection settles before asking Booth for one image. */
export const PREVIEW_SETTLE_MS = 600;

/** Right-hand preview column. The maximum is about half the window, and the list keeps 240px. */
export const PREVIEW_PANE_WIDTH = { min: 240, default: 300, absoluteMax: 1600 };
const PREVIEW_LIST_MIN = 240;
const PREVIEW_SPLIT_GAP = 24;

export function previewPaneMaxWidth(windowWidth: number, splitWidth: number | null = null): number {
  const half = Math.max(PREVIEW_PANE_WIDTH.min, Math.floor(windowWidth / 2));
  const capped = Math.min(PREVIEW_PANE_WIDTH.absoluteMax, half);
  if (splitWidth == null || splitWidth <= 0) return capped;
  const room = Math.floor(splitWidth - PREVIEW_SPLIT_GAP - PREVIEW_LIST_MIN);
  return Math.max(PREVIEW_PANE_WIDTH.min, Math.min(capped, room));
}

export function clampPreviewPaneWidth(
  value: number,
  windowWidth: number,
  splitWidth: number | null = null,
): number {
  const max = previewPaneMaxWidth(windowWidth, splitWidth);
  if (!Number.isFinite(value)) return Math.min(max, PREVIEW_PANE_WIDTH.default);
  return Math.round(Math.min(max, Math.max(PREVIEW_PANE_WIDTH.min, value)));
}

/** Width stored in settings, independent of the window that happens to be open. */
export function clampStoredPreviewPaneWidth(value: number): number {
  if (!Number.isFinite(value)) return PREVIEW_PANE_WIDTH.default;
  return Math.round(
    Math.min(PREVIEW_PANE_WIDTH.absoluteMax, Math.max(PREVIEW_PANE_WIDTH.min, value)),
  );
}

let rememberedPreview = false;
let rememberedWidth = PREVIEW_PANE_WIDTH.default;

export function rememberedPreviewOpen(): boolean {
  return rememberedPreview;
}

export function rememberPreviewOpen(open: boolean): void {
  rememberedPreview = open;
}

export function rememberedPreviewWidth(): number {
  return rememberedWidth;
}

export function rememberPreviewWidth(width: number): void {
  rememberedWidth = clampStoredPreviewPaneWidth(width);
}

/** Age-restricted products stay on the small image until the user asks. */
export function shouldRequestLargePreview(isAdult: boolean, revealed: boolean): boolean {
  return !isAdult || revealed;
}

const MOVE_KEYS = new Set(["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]);

export function isPreviewMoveKey(key: string): boolean {
  return MOVE_KEYS.has(key);
}

export function nextPreviewId(
  ids: readonly string[],
  currentId: string | null,
  key: string,
): string | null {
  if (!isPreviewMoveKey(key) || ids.length === 0) return currentId;
  const forward = key === "ArrowDown" || key === "ArrowRight";
  const index = currentId == null ? -1 : ids.indexOf(currentId);
  if (index < 0) return forward ? ids[0]! : ids[ids.length - 1]!;
  const next = index + (forward ? 1 : -1);
  if (next < 0 || next >= ids.length) return ids[index]!;
  return ids[next]!;
}

const FIELD_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

export function previewKeysBlocked(tagName: string, editable: boolean, inMenu: boolean): boolean {
  return editable || inMenu || FIELD_TAGS.has(tagName);
}
