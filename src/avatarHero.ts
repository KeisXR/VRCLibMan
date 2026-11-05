/** アバター詳細のヒーロー画像。基準は 2400×1200（2:1）を中央切り抜き。 */

export type AvatarHeroSize = "large" | "small" | "off";

export const AVATAR_HERO_BASE = { width: 2400, height: 1200 } as const;

export const AVATAR_HERO_HEIGHT = {
  large: 400,
  small: 320,
  narrow: 220,
} as const;

/** 設定値を正規化する。未知・空は小（320px）。 */
export function normalizeAvatarHero(value: unknown): AvatarHeroSize {
  const raw = typeof value === "string" ? value.trim().toLowerCase() : "";
  if (raw === "large" || raw === "small" || raw === "off") return raw;
  return "small";
}

export function avatarHeroHeightPx(size: AvatarHeroSize, viewportWidth: number): number {
  if (size === "off") return 0;
  if (viewportWidth > 0 && viewportWidth <= 860) return AVATAR_HERO_HEIGHT.narrow;
  return size === "large" ? AVATAR_HERO_HEIGHT.large : AVATAR_HERO_HEIGHT.small;
}

export function avatarHeroLabel(size: AvatarHeroSize): string {
  switch (size) {
    case "large":
      return "大 400px";
    case "small":
      return "小 320px";
    case "off":
      return "旧デザイン";
  }
}

/** sRGB 0–255 → 相対輝度（WCAG）。 */
export function relativeLuminance(r: number, g: number, b: number): number {
  const lin = [r, g, b].map((channel) => {
    const c = channel / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

export function contrastRatio(a: number, b: number): number {
  const lighter = Math.max(a, b);
  const darker = Math.min(a, b);
  return (lighter + 0.05) / (darker + 0.05);
}

/** 2色を alpha で混ぜる（fg over bg）。 */
export function mixRgb(
  fg: readonly [number, number, number],
  bg: readonly [number, number, number],
  alpha: number,
): [number, number, number] {
  const t = Math.min(1, Math.max(0, alpha));
  return [
    Math.round(fg[0] * t + bg[0] * (1 - t)),
    Math.round(fg[1] * t + bg[1] * (1 - t)),
    Math.round(fg[2] * t + bg[2] * (1 - t)),
  ];
}

/**
 * ヒーロー下端付近の文字コントラスト見積もり。
 * カード型スクリムは使わない。画像の最悪寄りの中間灰色の上に、
 * 背景への強めのフェード（約25%から、下端付近 alpha≈0.88）だけを載せ、
 * 本文色との比が 4.5 以上になること。
 */
export function heroTextContrast(
  text: readonly [number, number, number],
  pageBg: readonly [number, number, number],
  imageSample: readonly [number, number, number] = [128, 128, 128],
  fadeAlpha = 0.88,
  scrimAlpha = 0,
): number {
  const afterFade = mixRgb(pageBg, imageSample, fadeAlpha);
  const afterScrim = mixRgb(pageBg, afterFade, scrimAlpha);
  return contrastRatio(relativeLuminance(...text), relativeLuminance(...afterScrim));
}

export const AVATAR_HERO_CHOICES: ReadonlyArray<{ value: AvatarHeroSize; label: string; help: string }> = [
  { value: "small", label: "小 320px", help: "既定。ぼかし中のヒーロー。" },
  { value: "large", label: "大 400px", help: "高めのヒーロー。" },
  { value: "off", label: "旧デザイン", help: "画像なしの従来の見出し。" },
];
