import { useCallback, useEffect, useState } from "react";

/**
 * Long product lists draw this many cards when they open, then add the same number after each
 * paint until every card is there. Drawing a few hundred cards at once held the window for a
 * moment on every page change; the first screenful now shows at once. The list still ends up
 * complete, so scrolling, keyboard moves and links to a product work as before.
 */
export const LIST_STEP = 40;

/** Runs `run` once the browser has painted the current frame. Returns a cancel function. */
export function afterPaint(run: () => void): () => void {
  if (typeof window.requestAnimationFrame !== "function") {
    const timer = window.setTimeout(run, 16);
    return () => window.clearTimeout(timer);
  }
  let timer: number | undefined;
  const frame = window.requestAnimationFrame(() => {
    timer = window.setTimeout(run, 0);
  });
  return () => {
    window.cancelAnimationFrame(frame);
    if (timer !== undefined) window.clearTimeout(timer);
  };
}

/** Splits a list into the groups drawn together (each group is memoized as one part). */
export function chunked<T>(items: readonly T[], size = LIST_STEP): T[][] {
  const out: T[][] = [];
  for (let start = 0; start < items.length; start += size) out.push(items.slice(start, start + size));
  return out;
}

/** How many groups to draw for `count` items. */
export function chunksFor(count: number, size = LIST_STEP): number {
  return Math.ceil(Math.max(0, count) / size);
}

/**
 * How many items of the list to draw now. A new `listKey` (another genre or filter) starts over
 * from the first step; the same list read again (after a refresh) keeps what is drawn, so the
 * scroll position stays. `reveal(index)` draws at least up to that item at once.
 */
export function useProgressiveCount(listKey: string, total: number) {
  const [progress, setProgress] = useState({ key: listKey, count: LIST_STEP });
  const drawn = progress.key === listKey ? progress.count : LIST_STEP;
  const count = Math.min(total, drawn);

  useEffect(() => {
    if (count >= total) return;
    return afterPaint(() =>
      setProgress((previous) => {
        const current = previous.key === listKey ? previous.count : LIST_STEP;
        return { key: listKey, count: Math.max(current, count) + LIST_STEP };
      }),
    );
  }, [listKey, count, total]);

  const reveal = useCallback(
    (index: number) => {
      setProgress((previous) => {
        const current = previous.key === listKey ? previous.count : LIST_STEP;
        return index < current ? previous : { key: listKey, count: index + 1 };
      });
    },
    [listKey],
  );

  return { count, reveal };
}
