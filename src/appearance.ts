export type Appearance = "classic" | "liquid";

export function normalizeAppearance(value: unknown): Appearance {
  return typeof value === "string" && value.trim() === "liquid" ? "liquid" : "classic";
}

export function applyAppearance(value: unknown): void {
  if (typeof document !== "undefined") {
    document.documentElement.classList.toggle("theme-liquid", normalizeAppearance(value) === "liquid");
  }
}

/** Subscribe before reading so a slow startup response cannot undo a newer saved choice. */
export async function startAppearance(
  load: () => Promise<{ appearance: unknown }>,
  subscribe: (changed: (appearance: unknown) => void) => Promise<() => void>,
  apply: (appearance: unknown) => void = applyAppearance,
): Promise<() => void> {
  let changed = false;
  const stop = await subscribe((appearance) => {
    changed = true;
    apply(appearance);
  }).catch(() => () => {});
  try {
    const settings = await load();
    if (!changed) apply(settings.appearance);
  } catch {
    if (!changed) apply("classic");
  }
  return stop;
}
