import { scrollParent } from "./settingsNav";

/**
 * 検索の結果（画面と設定）から、その画面の該当箇所へ移る。設定画面などは読み込んでから描くので、
 * 要素が現れるまで最大 `APP_JUMP_WAIT_MS` 待ち、閉じた `<details>` を開き、スクロールして一瞬だけ枠で示す。
 * 要素が出なければ `fallback`（親の節）へ、それも無ければページの先頭のままにする。
 * 設定画面は節ごとに遅れて読み込まれ、上の節が伸びると行が下へずれる。そこで高さが落ち着いてから動かし、
 * スクロールが終わってもまだ見えていなければ一度だけ合わせ直す。途中で利用者が自分で動かしたらやめる。
 */

export const APP_JUMP_WAIT_MS = 3000;
export const APP_JUMP_FLASH_MS = 2000;
export const APP_JUMP_CLASS = "search-jump-target";
/** ページの高さがこの間変わらなければ落ち着いたとみなす。 */
const SETTLE_QUIET_MS = 150;
const SETTLE_MAX_MS = 1000;
/** `scrollend` が来ない環境で、合わせ直すまで待つ長さ。 */
const SCROLL_END_FALLBACK_MS = 900;

let generation = 0;

const reducedMotion = () =>
  typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const nextFrame = (run: () => void) => {
  if (typeof window.requestAnimationFrame === "function") window.requestAnimationFrame(run);
  else window.setTimeout(run, 16);
};

/** 節や見出しは上端に、行や開閉の欄は表示域の中ほどに寄せる。 */
export function jumpBlock(node: Element): ScrollLogicalPosition {
  return node.matches("section, h1, h2, h3, .settings-section") ? "start" : "center";
}

/** 枠で示す要素。設定の行の中なら行全体、見出しならその節。 */
export function jumpMark(node: HTMLElement): HTMLElement {
  const row = node.closest<HTMLElement>(".setting-row");
  if (row) return row;
  if (node.matches("h2, h3")) return node.closest<HTMLElement>("section") ?? node;
  return node;
}

function focusInside(mark: HTMLElement) {
  const control = mark.tagName === "DETAILS"
    ? (Array.from(mark.children).find((child) => child.tagName === "SUMMARY") as HTMLElement | undefined) ?? null
    : mark.querySelector<HTMLElement>("input:not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), a[href], summary");
  const self = !control && mark.matches("button:not([disabled]), a[href], input:not([disabled]), select:not([disabled])") ? mark : null;
  (control ?? self)?.focus?.({ preventScroll: true });
}

/** 要素がスクロール領域の見えている範囲に少しでも入っているか。 */
function inView(node: HTMLElement, scroller: HTMLElement | null): boolean {
  if (typeof node.getBoundingClientRect !== "function") return true;
  const rect = node.getBoundingClientRect();
  const area = scroller?.getBoundingClientRect();
  const top = area ? area.top : 0;
  const bottom = area ? area.bottom : window.innerHeight;
  return rect.bottom > top && rect.top < bottom;
}

function reveal(node: HTMLElement, alive: () => boolean) {
  for (let current: HTMLElement | null = node; current; current = current.parentElement) {
    if (current.tagName === "DETAILS" && !current.hasAttribute("open")) current.setAttribute("open", "");
  }
  const mark = jumpMark(node);
  const block = jumpBlock(node);
  const scroller = scrollParent(node);
  const heightOf = () => (scroller ?? document.documentElement).scrollHeight;
  const scrollTo = (behavior: ScrollBehavior) => node.scrollIntoView?.({ behavior, block });

  // 節そのものへ移るときはフォーカスを動かさない。行・開閉の欄・ボタンは最初の操作へ移す。
  if (!mark.matches("section, .settings-section")) focusInside(mark);
  mark.classList.add(APP_JUMP_CLASS);
  window.setTimeout(() => mark.classList.remove(APP_JUMP_CLASS), APP_JUMP_FLASH_MS);

  const started = Date.now();
  let height = heightOf();
  let quietSince = started;
  const settle = () => {
    if (!alive()) return;
    const now = Date.now();
    const next = heightOf();
    if (next !== height) {
      height = next;
      quietSince = now;
    }
    if (now - quietSince < SETTLE_QUIET_MS && now - started < SETTLE_MAX_MS) {
      nextFrame(settle);
      return;
    }
    scrollTo(reducedMotion() ? "instant" : "smooth");
    // スクロールの途中でさらに伸びたときは、終わったところで一度だけ合わせ直す。
    const target: HTMLElement | Window = scroller ?? window;
    let done = false;
    const correct = () => {
      if (done) return;
      done = true;
      target.removeEventListener("scrollend", correct);
      if (alive() && !inView(node, scroller)) scrollTo("instant");
    };
    target.addEventListener("scrollend", correct);
    window.setTimeout(correct, SCROLL_END_FALLBACK_MS);
  };
  nextFrame(settle);
}

export function jumpToAppLocation(href: string, target?: string, fallback?: string) {
  const ticket = ++generation;
  window.location.hash = href;
  if (!target && !fallback) return;
  // 利用者が自分でスクロール・操作したら、こちらからは動かさない。
  let touched = false;
  const touch = () => { touched = true; };
  const events = ["wheel", "touchstart", "keydown", "pointerdown"] as const;
  for (const name of events) window.addEventListener(name, touch, { capture: true, passive: true });
  window.setTimeout(() => {
    for (const name of events) window.removeEventListener(name, touch, { capture: true });
  }, APP_JUMP_WAIT_MS + SETTLE_MAX_MS + SCROLL_END_FALLBACK_MS);
  // 待っている間に別の結果を選んだり、別の画面へ移ったりしたらやめる。
  const alive = () => ticket === generation && !touched && window.location.hash === href;

  const started = Date.now();
  const step = () => {
    if (!alive()) return;
    const found = target ? document.getElementById(target) : null;
    if (found) {
      reveal(found, alive);
      return;
    }
    if (Date.now() - started < APP_JUMP_WAIT_MS) {
      nextFrame(step);
      return;
    }
    const parent = fallback ? document.getElementById(fallback) : null;
    if (parent) reveal(parent, alive);
  };
  nextFrame(step);
}
