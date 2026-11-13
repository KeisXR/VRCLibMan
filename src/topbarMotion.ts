import { useCallback, useLayoutEffect, useRef, useState, type AnimationEvent } from "react";

/**
 * 上部の帯（通知・失敗・進捗）の出入りの動き。速さは左サイドバーの開閉と同じ（styles.css の「上部の帯の出入り」）。
 * 出るときは帯の高さ分だけ上の余白を負から戻して下の内容を押し下げ、閉じるときは逆に詰める。
 * 高さと元の余白は、出始めと閉じ始めにこのフックが測って `--topbar-motion-top` / `--topbar-motion-height` に置く。
 */

/** 閉じる動きが終わらないときに取り除くまでの上限。CSS の長さより長くする。 */
export const TOPBAR_LEAVE_FALLBACK_MS = 400;
/** 閉じる動きの終わりを知らせる keyframes の名前。 */
export const TOPBAR_COLLAPSE_ANIMATION = "topbar-collapse";

/**
 * 帯を動かすか。OS の「動きを減らす」と、matchMedia の無い環境（テスト）ではすぐ出し入れする。
 * CSS 側も prefers-reduced-motion で animation を止めるので、animationend を待たない。
 */
export function topbarMotionEnabled(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/** 動きを外した状態の上の余白と高さを測って置く。測る間だけ animation を外す（動きの値を読まないため）。 */
function measure(el: HTMLElement) {
  el.style.animation = "none";
  el.style.setProperty("--topbar-motion-top", window.getComputedStyle?.(el).marginTop || "0px");
  el.style.setProperty("--topbar-motion-height", `${el.offsetHeight ?? 0}px`);
  el.style.animation = "";
}

/**
 * 帯の要素に付ける ref と属性。`leaving` になったら閉じる動きを見せ、終わったら `onLeft` を1回だけ呼ぶ。
 * animationend が来ないとき（画面が隠れているなど）も上限の時間で呼ぶ。
 */
export function useTopbarMotion<E extends HTMLElement = HTMLDivElement>(leaving: boolean | undefined, onLeft: (() => void) | undefined) {
  const ref = useRef<E>(null);
  const done = useRef(false);
  const finish = useCallback(() => {
    if (done.current) return;
    done.current = true;
    onLeft?.();
  }, [onLeft]);
  useLayoutEffect(() => {
    if (ref.current) measure(ref.current);
  }, []);
  useLayoutEffect(() => {
    if (!leaving) {
      done.current = false;
      return;
    }
    if (ref.current) measure(ref.current);
    const fallback = window.setTimeout(finish, TOPBAR_LEAVE_FALLBACK_MS);
    return () => window.clearTimeout(fallback);
  }, [leaving, finish]);
  const onAnimationEnd = (event: AnimationEvent<E>) => {
    if (leaving && event.target === event.currentTarget && event.animationName === TOPBAR_COLLAPSE_ANIMATION) finish();
  };
  return {
    ref,
    onAnimationEnd,
    "data-topbar-motion": "",
    "data-leaving": leaving ? "true" : undefined,
  } as const;
}

/**
 * 値が null になっても、閉じる動きの間は最後の値を出し続ける。
 * 値が戻ったら閉じる動きをやめて新しい値を出す（要素は作り直さないので、出る動きはもう一度は付かない）。
 */
export function useExitPresence<T>(value: T | null, animate: boolean): { value: T | null; leaving: boolean; onLeft: () => void } {
  const last = useRef<T | null>(value);
  if (value !== null) last.current = value;
  const [present, setPresent] = useState(value !== null);
  const [leaving, setLeaving] = useState(false);
  if (value !== null && (!present || leaving)) {
    setPresent(true);
    setLeaving(false);
  } else if (value === null && present && !leaving) {
    if (animate) setLeaving(true);
    else setPresent(false);
  }
  const onLeft = useCallback(() => {
    setPresent(false);
    setLeaving(false);
  }, []);
  return { value: present ? (value ?? last.current) : null, leaving: leaving && value === null, onLeft };
}
