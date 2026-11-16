import { useCallback, useEffect, useRef, useState } from "react";
import { activeSettingsSection } from "./settingsNav";
import { genreSectionId } from "./genreSections";

const reducedMotion = () => typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** A request to bring a genre heading into view. The page owning the list carries it out. */
export interface GenreJump {
  key: string;
  nonce: number;
  smooth: boolean;
}

/**
 * The genre column as a table of contents, like the settings page (SettingsNav.tsx): `active` is
 * the genre whose heading the page has scrolled to, and `go(key)` asks the page to scroll to a
 * heading (`jump`). While that scroll runs, the clicked genre stays current.
 * `keys` is the order of the headings; headings not on screen yet are skipped.
 */
export function useGenreSpy(keys: readonly string[]) {
  const [active, setActive] = useState<string | null>(null);
  const [jump, setJump] = useState<GenreJump | null>(null);
  const pinned = useRef<number | null>(null);
  const keysRef = useRef(keys);
  keysRef.current = keys;
  const schedule = useRef<() => void>(() => {});

  useEffect(() => {
    let frame = 0;
    const measure = () => {
      frame = 0;
      if (pinned.current !== null) return;
      const scroller = document.querySelector<HTMLElement>(".app-shell .main");
      if (!scroller) return;
      const top = scroller.getBoundingClientRect().top;
      const tops = keysRef.current.flatMap((key) => {
        const node = document.getElementById(genreSectionId(key));
        return node ? [{ id: key, top: node.getBoundingClientRect().top - top }] : [];
      });
      const atBottom = scroller.scrollTop > 0 && scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2;
      // A heading counts as reached once it is in the top quarter (below the sticky banners).
      const next = activeSettingsSection(tops, Math.max(120, scroller.clientHeight * 0.25), atBottom);
      setActive((current) => (current === next ? current : next));
    };
    const raf = typeof window.requestAnimationFrame === "function" ? window.requestAnimationFrame.bind(window) : (run: () => void) => window.setTimeout(run, 16);
    const run = () => {
      if (!frame) frame = raf(measure);
    };
    schedule.current = run;
    const release = () => {
      if (pinned.current === null) return;
      window.clearTimeout(pinned.current);
      pinned.current = null;
      run();
    };
    document.addEventListener("scroll", run, { capture: true, passive: true });
    document.addEventListener("scrollend", release, { capture: true });
    window.addEventListener("wheel", release, { passive: true });
    window.addEventListener("touchstart", release, { passive: true });
    window.addEventListener("resize", run);
    run();
    return () => {
      document.removeEventListener("scroll", run, { capture: true });
      document.removeEventListener("scrollend", release, { capture: true });
      window.removeEventListener("wheel", release);
      window.removeEventListener("touchstart", release);
      window.removeEventListener("resize", run);
      if (frame) (typeof window.cancelAnimationFrame === "function" ? window.cancelAnimationFrame(frame) : window.clearTimeout(frame));
      if (pinned.current !== null) window.clearTimeout(pinned.current);
      schedule.current = () => {};
    };
  }, []);

  // New headings (another list, more cards drawn) can change which one is reached.
  useEffect(() => schedule.current(), [keys]);

  const go = useCallback((key: string, smooth = true) => {
    if (pinned.current !== null) window.clearTimeout(pinned.current);
    // scrollend does not come everywhere; after a while the scroll position decides again.
    pinned.current = window.setTimeout(() => {
      pinned.current = null;
      schedule.current();
    }, reducedMotion() || !smooth ? 50 : 900);
    setActive(key);
    setJump((current) => ({ key, nonce: (current?.nonce ?? 0) + 1, smooth }));
  }, []);

  /** Measure again after the page drew more of the list. */
  const resync = useCallback(() => schedule.current(), []);

  return { active, jump, go, resync };
}

/** Scrolls the heading of `key` to the top, smoothly unless reduced motion is asked for. */
export function scrollToGenreSection(key: string, smooth: boolean): boolean {
  const node = document.getElementById(genreSectionId(key));
  if (!node) return false;
  node.scrollIntoView?.({ behavior: smooth && !reducedMotion() ? "smooth" : "auto", block: "start" });
  return true;
}
