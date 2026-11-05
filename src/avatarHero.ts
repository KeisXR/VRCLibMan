/** アバター詳細のヒーロー画像。基準は 2400×1200（2:1）を中央切り抜き。 */

export type AvatarHeroSize = "large" | "small" | "off";

/** 横長フルブリードか、左ブロック＋ぼかし背景か。画像の幅/高さ >= しきい値で wide。 */
export type AvatarHeroLayout = "wide" | "block";

/** コンポーネント内の状態。"pending" は未計測（画像は出さずフォールバック背景のみ）。 */
export type AvatarHeroLayoutState = AvatarHeroLayout | "pending";

export const AVATAR_HERO_BASE = { width: 2400, height: 1200 } as const;

/** これ以上のアスペクト比は全幅ヒーロー（wide）。未満は左ブロック（block）。 */
export const AVATAR_HERO_WIDE_ASPECT = 1.6;

/** 横長ヒーローのみ。自然幅がこれ未満なら低解像度の注意を出す。 */
export const AVATAR_HERO_LOW_RES_WIDTH = 2000;

export const AVATAR_HERO_HEIGHT = {
  large: 400,
  small: 320,
  narrow: 220,
} as const;

/** ヒーロー内の補足色（グローバル --text-secondary とは別）。 */
export const AVATAR_HERO_SUB = {
  light: [0x5d, 0x61, 0x6c] as const,
  dark: [0xb4, 0xb4, 0xbe] as const,
};

/** 覆い（上→下）の --bg 合成率。 */
export const AVATAR_HERO_VEIL = { top: 0.45, mid: 0.82, bottom: 1 } as const;

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

/** 自然サイズからレイアウトを決める。未計測は wide 扱い（従来の全幅）。 */
export function avatarHeroLayout(naturalWidth: number, naturalHeight: number): AvatarHeroLayout {
  if (!(naturalWidth > 0 && naturalHeight > 0)) return "wide";
  return naturalWidth / naturalHeight >= AVATAR_HERO_WIDE_ASPECT ? "wide" : "block";
}

/** 横長ヒーローで自然幅が足りないときだけ true。 */
export function avatarHeroLowRes(layout: AvatarHeroLayout, naturalWidth: number): boolean {
  return layout === "wide" && naturalWidth > 0 && naturalWidth < AVATAR_HERO_LOW_RES_WIDTH;
}

/** ブロックレイアウト用のアスペクト比（極端な縦長は 0.75 まで）。 */
export function avatarHeroBlockAspect(naturalWidth: number, naturalHeight: number): number {
  if (!(naturalWidth > 0 && naturalHeight > 0)) return 1;
  return Math.max(0.75, naturalWidth / naturalHeight);
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
 * 覆い（既定 mid=0.82）だけを載せ、本文色との比が 4.5 以上になること。
 */
export function heroTextContrast(
  text: readonly [number, number, number],
  pageBg: readonly [number, number, number],
  imageSample: readonly [number, number, number] = [128, 128, 128],
  fadeAlpha = AVATAR_HERO_VEIL.mid,
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

export const AVATAR_ASSETS_OPEN_KEY = "vrclibman.avatarDetail.assetsOpen";

export function readAssetsOpenPref(storage?: Pick<Storage, "getItem"> | null): boolean {
  try {
    const raw = storage?.getItem(AVATAR_ASSETS_OPEN_KEY);
    if (raw === "0" || raw === "false") return false;
    if (raw === "1" || raw === "true") return true;
  } catch {
    /* ignore */
  }
  return true;
}

export function writeAssetsOpenPref(open: boolean, storage?: Pick<Storage, "setItem"> | null): void {
  try {
    storage?.setItem(AVATAR_ASSETS_OPEN_KEY, open ? "1" : "0");
  } catch {
    /* ignore */
  }
}
