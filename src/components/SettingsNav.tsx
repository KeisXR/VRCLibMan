import { useEffect, useRef, useState } from "react";
import { SETTINGS_NAV_ITEMS, activeSettingsSection, scrollParent } from "../settingsNav";

const reducedMotion = () => typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * 設定のカテゴリ一覧。広い幅では左の縦の列、設定ページが 800px 以下では上に固定した1行の帯になる（styles.css）。
 * 今いるカテゴリに `aria-current` を付ける。詳しくは docs/design/README.md の「設定画面」。
 */
export function SettingsNav() {
  const navRef = useRef<HTMLElement>(null);
  const [active, setActive] = useState<string>(SETTINGS_NAV_ITEMS[0].id);
  // ボタンで移動している間は、途中のスクロール位置で強調を動かさない。
  const pinned = useRef<{ id: string; timer: number } | null>(null);
  const resync = useRef<() => void>(() => {});

  useEffect(() => {
    const nav = navRef.current;
    if (!nav) return;
    const layout = nav.parentElement;
    const scroller = scrollParent(nav);
    const target: HTMLElement | Window = scroller ?? window;
    let frame = 0;
    const measure = () => {
      frame = 0;
      layout?.style.setProperty("--settings-nav-h", `${Math.ceil(nav.getBoundingClientRect().height)}px`);
      if (pinned.current) return;
      const top = scroller ? scroller.getBoundingClientRect().top : 0;
      const bar = typeof window.getComputedStyle === "function" && window.getComputedStyle(nav).flexDirection === "row";
      const height = scroller ? scroller.clientHeight : window.innerHeight;
      // 見出しが帯の下から表示域の上 1/4 までに来た節を「今いる」とする。
      const threshold = Math.max((bar ? nav.getBoundingClientRect().height : 0) + 32, height * 0.25);
      const tops = SETTINGS_NAV_ITEMS.flatMap(({ id }) => {
        const node = document.getElementById(id);
        return node ? [{ id, top: node.getBoundingClientRect().top - top }] : [];
      });
      const scrollTop = scroller ? scroller.scrollTop : window.scrollY;
      const full = scroller ? scroller.scrollHeight : document.documentElement.scrollHeight;
      const atBottom = scrollTop > 0 && scrollTop + height >= full - 2;
      const next = activeSettingsSection(tops, threshold, atBottom);
      if (next) setActive(next);
    };
    const raf = typeof window.requestAnimationFrame === "function" ? window.requestAnimationFrame.bind(window) : (run: () => void) => window.setTimeout(run, 16);
    const schedule = () => { if (!frame) frame = raf(measure); };
    resync.current = schedule;
    const release = () => {
      if (!pinned.current) return;
      window.clearTimeout(pinned.current.timer);
      pinned.current = null;
      schedule();
    };
    target.addEventListener("scroll", schedule, { passive: true });
    target.addEventListener("scrollend", release);
    target.addEventListener("wheel", release, { passive: true });
    target.addEventListener("touchstart", release, { passive: true });
    window.addEventListener("resize", schedule);
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(schedule) : null;
    observer?.observe(nav);
    if (layout) observer?.observe(layout);
    schedule();
    return () => {
      target.removeEventListener("scroll", schedule);
      target.removeEventListener("scrollend", release);
      target.removeEventListener("wheel", release);
      target.removeEventListener("touchstart", release);
      window.removeEventListener("resize", schedule);
      observer?.disconnect();
      if (frame) (typeof window.cancelAnimationFrame === "function" ? window.cancelAnimationFrame(frame) : window.clearTimeout(frame));
      if (pinned.current) window.clearTimeout(pinned.current.timer);
    };
  }, []);

  // 帯が横にあふれているとき、今いるカテゴリを帯の中で見える位置へ寄せる（縦のスクロールは動かさない）。
  useEffect(() => {
    const nav = navRef.current;
    const button = nav?.querySelector<HTMLElement>(`[data-target="${active}"]`);
    if (!nav || !button || nav.scrollWidth <= nav.clientWidth) return;
    const left = button.offsetLeft - nav.offsetLeft;
    const right = left + button.offsetWidth;
    if (left < nav.scrollLeft || right > nav.scrollLeft + nav.clientWidth) {
      const goal = Math.max(0, left - (nav.clientWidth - button.offsetWidth) / 2);
      if (typeof nav.scrollTo === "function") nav.scrollTo({ left: goal, behavior: reducedMotion() ? "instant" : "smooth" });
      else nav.scrollLeft = goal;
    }
  }, [active]);

  const go = (id: string) => {
    const node = document.getElementById(id);
    if (!node) return;
    if (node.tagName === "DETAILS") node.setAttribute("open", "");
    if (pinned.current) window.clearTimeout(pinned.current.timer);
    // scrollend が来ない環境でも、少し待てば強調をスクロール位置に戻す。
    pinned.current = { id, timer: window.setTimeout(() => { pinned.current = null; resync.current(); }, reducedMotion() ? 50 : 900) };
    setActive(id);
    node.scrollIntoView?.({ behavior: reducedMotion() ? "instant" : "smooth", block: "start" });
  };

  return (
    <nav className="settings-nav" aria-label="設定の項目" ref={navRef}>
      {SETTINGS_NAV_ITEMS.map(({ id, label }) => (
        <button key={id} type="button" data-target={id} aria-current={active === id ? "location" : undefined} onClick={() => go(id)}>{label}</button>
      ))}
    </nav>
  );
}
