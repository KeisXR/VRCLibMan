/** Wait until the selection settles before asking Booth for one image. */
export const PREVIEW_SETTLE_MS = 600;

let rememberedPreview = false;

export function rememberedPreviewOpen(): boolean {
  return rememberedPreview;
}

export function rememberPreviewOpen(open: boolean): void {
  rememberedPreview = open;
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
