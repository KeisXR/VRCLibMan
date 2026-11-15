import { useEffect, useRef, type RefObject } from "react";

/**
 * 切替のたびに要素の `switch-rise`（2px の短い rise）を最初から再生する。
 * 要素を作り直さないので、一覧の中身（サムネイルなど）は保ったまま。
 * 動きは CSS の animation なので、`prefers-reduced-motion` のグローバル停止がそのまま効く。
 * `key` が null の間（最初の読み込み前）は数えず、最初に決まった値では動かさない。
 */
export function useReplayRise<T extends HTMLElement>(ref: RefObject<T | null>, key: unknown): void {
  const first = useRef(true);
  useEffect(() => {
    if (key == null) return;
    if (first.current) {
      first.current = false;
      return;
    }
    const node = ref.current;
    if (!node) return;
    node.classList.remove("switch-rise");
    void node.offsetWidth; // reflow して animation を再始動する
    node.classList.add("switch-rise");
  }, [ref, key]);
}
