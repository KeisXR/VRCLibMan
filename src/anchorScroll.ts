/**
 * 画面を移ったあとに、その画面の節までスクロールする。
 *
 * 移った先の画面は、設定の読み込みなどを待ってから節を描くので、節が現れるまで待つ（最大 `timeoutMs`）。
 * 節の上にある欄があとから読み込まれて伸びると位置がずれるので、少しのあいだ置き直す。
 * 利用者がホイールやキーで動かしたら、置き直しはやめる。
 */

const reducedMotion = () =>
  typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** 見出しに付いた id は、見出しを含むカード（`.settings-section`）ごと見せる。カードの上の余白が切れないように。 */
export function anchorTarget(node: HTMLElement): HTMLElement {
  if (/^H[1-6]$/.test(node.tagName)) {
    const card = node.closest<HTMLElement>(".settings-section");
    if (card) return card;
  }
  return node;
}

/** 節の位置の置き直しをいつ行うか（最初のスクロールからのミリ秒）。 */
export const ANCHOR_SETTLE_MS = [250, 600, 1200] as const;

let cancelPending: (() => void) | null = null;

export function scrollToAnchor(id: string, { smooth = false, timeoutMs = 3000 } = {}): () => void {
  cancelPending?.();
  let stopped = false;
  const timers: number[] = [];
  const stop = () => {
    if (stopped) return;
    stopped = true;
    timers.forEach((timer) => window.clearTimeout(timer));
    for (const name of ["wheel", "touchstart", "keydown", "pointerdown"] as const) {
      window.removeEventListener(name, stop, true);
    }
    if (cancelPending === stop) cancelPending = null;
  };
  cancelPending = stop;
  const startedAt = Date.now();

  const place = (behavior: ScrollBehavior) => {
    const node = document.getElementById(id);
    if (!node) return false;
    if (node.tagName === "DETAILS") node.setAttribute("open", "");
    anchorTarget(node).scrollIntoView?.({ behavior, block: "start" });
    return true;
  };

  const attempt = () => {
    if (stopped) return;
    if (!place(smooth && !reducedMotion() ? "smooth" : "auto")) {
      if (Date.now() - startedAt < timeoutMs) timers.push(window.setTimeout(attempt, 50));
      else stop();
      return;
    }
    // ここから先は利用者が動かしたら止める。
    for (const name of ["wheel", "touchstart", "keydown", "pointerdown"] as const) {
      window.addEventListener(name, stop, { capture: true, passive: true });
    }
    // なめらかに動かしているあいだは置き直さない（動きが止まる）。
    if (smooth && !reducedMotion()) {
      timers.push(window.setTimeout(stop, ANCHOR_SETTLE_MS[ANCHOR_SETTLE_MS.length - 1]));
      return;
    }
    ANCHOR_SETTLE_MS.forEach((delay, index) => {
      timers.push(
        window.setTimeout(() => {
          if (stopped) return;
          place("auto");
          if (index === ANCHOR_SETTLE_MS.length - 1) stop();
        }, delay),
      );
    });
  };
  attempt();
  return stop;
}

/** 設定の節の id。他の画面からの移動先なので、設定画面の節の id と同じにする（AGENTS.md）。 */
export type SettingsSectionId =
  | "settings-library"
  | "settings-unity"
  | "settings-startup"
  | "settings-design"
  | "settings-asset-updates"
  | "settings-booth"
  | "settings-data"
  | "settings-app-update"
  | "settings-knowledge"
  | "settings-debug";

/** 設定画面を開き、その節までスクロールする。設定画面にいるときは、その場でなめらかに動かす。 */
export function openSettingsSection(section: SettingsSectionId): void {
  const here = window.location.hash.replace(/^#/, "").split("/").filter(Boolean)[0] === "settings";
  if (!here) window.location.hash = "#/settings";
  scrollToAnchor(section, { smooth: here });
}
