import { useEffect, useRef, useState, type ReactNode } from "react";
import { mediaSrc } from "../media";
import type { AvatarHeroSize } from "../avatarHero";

export function AvatarHero({
  size,
  path,
  url,
  name,
  kicker,
  subtitle,
  nav,
  actions,
}: {
  size: Exclude<AvatarHeroSize, "off">;
  path: string | null;
  url: string | null;
  name: string;
  kicker: string;
  subtitle: string;
  nav?: ReactNode;
  actions?: ReactNode;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const mediaRef = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState(false);
  const [reduced, setReduced] = useState(false);
  const src = failed ? null : mediaSrc(path, url);

  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => setReduced(mq.matches);
    sync();
    mq.addEventListener?.("change", sync);
    return () => mq.removeEventListener?.("change", sync);
  }, []);

  useEffect(() => {
    if (reduced) return;
    if (typeof requestAnimationFrame !== "function" || typeof cancelAnimationFrame !== "function") return;
    const root = rootRef.current;
    const media = mediaRef.current;
    if (!root || !media) return;
    const scroller = root.closest(".main");
    if (!scroller) return;
    let frame = 0;
    const onScroll = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const top = (scroller as HTMLElement).scrollTop;
        const height = root.offsetHeight || 1;
        const progress = Math.min(1, Math.max(0, top / height));
        media.style.opacity = String(1 - progress * 0.35);
        media.style.transform = `translateY(${progress * 18}px)`;
      });
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
    return () => {
      cancelAnimationFrame(frame);
      scroller.removeEventListener("scroll", onScroll);
      media.style.opacity = "";
      media.style.transform = "";
    };
  }, [reduced, size, src]);

  return (
    <div
      className="avatar-hero"
      data-size={size}
      data-has-image={src ? "1" : "0"}
      ref={rootRef}
    >
      <div className="avatar-hero-media" ref={mediaRef}>
        {src ? (
          <img
            className="avatar-hero-image"
            src={src}
            alt=""
            draggable={false}
            referrerPolicy="no-referrer"
            onError={() => setFailed(true)}
          />
        ) : (
          <div className="avatar-hero-fallback" aria-hidden="true" />
        )}
        <div className="avatar-hero-blur" aria-hidden="true">
          <i />
          <i />
          <i />
        </div>
        <div className="avatar-hero-fade" aria-hidden="true" />
      </div>
      {nav ? <div className="avatar-hero-nav">{nav}</div> : null}
      <div className="avatar-hero-body">
        <div className="avatar-hero-copy">
          <p className="avatar-hero-kicker">{kicker}</p>
          <h1 className="avatar-hero-title">{name}</h1>
          <p className="avatar-hero-sub">{subtitle}</p>
        </div>
        {actions ? <div className="avatar-hero-actions">{actions}</div> : null}
      </div>
    </div>
  );
}
