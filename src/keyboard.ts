import { useEffect } from "react";
import type { RefObject } from "react";

/**
 * 横に並んだタブの矢印キー・Home・End の移り先。端では反対側へ回る。対象外のキーは null。
 */
export function tabKeyTarget(key: string, index: number, count: number): number | null {
  if (count <= 0) return null;
  switch (key) {
    case "ArrowRight":
      return (index + 1) % count;
    case "ArrowLeft":
      return (index - 1 + count) % count;
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}

const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type=hidden])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "summary",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export function focusableIn(container: HTMLElement): HTMLElement[] {
  return [...container.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((element) => {
    const tabindex = element.getAttribute("tabindex");
    return (tabindex === null || Number(tabindex) >= 0) && !element.closest("[hidden]");
  });
}

/**
 * Tab と Shift+Tab をダイアログの中で回す。最後で Tab なら最初へ、最初で Shift+Tab なら最後へ。
 * フォーカスが外にあるときも中へ戻す。処理したら true。
 */
export function trapTab(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "preventDefault">,
  container: HTMLElement,
  active: Element | null = document.activeElement,
): boolean {
  if (event.key !== "Tab") return false;
  const items = focusableIn(container);
  if (items.length === 0) {
    event.preventDefault();
    container.focus();
    return true;
  }
  const first = items[0]!;
  const last = items[items.length - 1]!;
  const inside = active != null && container.contains(active);
  if (!inside || (event.shiftKey && active === first) || (!event.shiftKey && active === last)) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
    return true;
  }
  return false;
}

/**
 * モーダルな重ね表示のフォーカス。開いている間は Tab を中で回し、閉じたら開く前の場所へ戻す。
 */
export function useModalFocus(ref: RefObject<HTMLElement | null>, active = true) {
  useEffect(() => {
    if (!active) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const onKey = (event: KeyboardEvent) => {
      const container = ref.current;
      if (container) trapTab(event, container);
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      if (before && before.isConnected) before.focus();
    };
  }, [ref, active]);
}

