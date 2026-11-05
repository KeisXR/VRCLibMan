import { useEffect, useRef, useState, type CSSProperties, type ReactNode, type SyntheticEvent } from "react";
import { mediaSrc } from "../media";
import {
  avatarHeroBlockAspect,
  avatarHeroLayout,
  avatarHeroLowRes,
  AVATAR_HERO_LOW_RES_WIDTH,
  type AvatarHeroLayout,
  type AvatarHeroSize,
} from "../avatarHero";

export function AvatarHero({
  size,
  path,
  url,
  name,
  kicker,
  subtitle,
  nav,
}: {
  size: Exclude<AvatarHeroSize, "off">;
  path: string | null;
  url: string | null;
  name: string;
  kicker: string;
  subtitle: string;
  nav?: ReactNode;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const mediaRef = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState(false);
  const [reduced, setReduced] = useState(false);
  const [layout, setLayout] = useState<AvatarHeroLayout>("wide");
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
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
    setFailed(false);
    setNatural(null);
    setLayout("wide");
  }, [path, url]);

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
  }, [reduced, size, src, layout]);

  const onImageLoad = (event: SyntheticEvent<HTMLImageElement>) => {
    const img = event.currentTarget;
    const w = img.naturalWidth;
    const h = img.naturalHeight;
    setNatural({ w, h });
    setLayout(avatarHeroLayout(w, h));
  };

  const lowRes = natural ? avatarHeroLowRes(layout, natural.w) : false;
  const blockAspect = natural ? avatarHeroBlockAspect(natural.w, natural.h) : 1;
  const copy = (
    <div className="avatar-hero-copy">
      <p className="avatar-hero-kicker">{kicker}</p>
      <h1 className="avatar-hero-title">{name}</h1>
      <p className="avatar-hero-sub">{subtitle}</p>
      {lowRes ? (
        <p className="avatar-hero-warn meta">
          画像が小さいため、横長ヒーローでは粗く見えることがあります（目安 {AVATAR_HERO_LOW_RES_WIDTH}px 以上）。
        </p>
      ) : null}
    </div>
  );

  return (
    <div
      className="avatar-hero"
      data-size={size}
      data-layout={layout}
      data-has-image={src ? "1" : "0"}
      ref={rootRef}
    >
      <div className="avatar-hero-media" ref={mediaRef}>
        {layout === "block" && src ? (
          <>
            <img
              className="avatar-hero-bg"
              src={src}
              alt=""
              draggable={false}
              referrerPolicy="no-referrer"
              aria-hidden="true"
            />
            <div className="avatar-hero-veil" aria-hidden="true" />
          </>
        ) : (
          <>
            {src ? (
              <img
                className="avatar-hero-image"
                src={src}
                alt=""
                draggable={false}
                referrerPolicy="no-referrer"
                onLoad={onImageLoad}
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
          </>
        )}
        {/* block でも自然サイズ計測用に隠した img を1枚読む */}
        {layout === "block" && src ? (
          <img
            className="avatar-hero-probe"
            src={src}
            alt=""
            draggable={false}
            referrerPolicy="no-referrer"
            onLoad={onImageLoad}
            onError={() => setFailed(true)}
          />
        ) : null}
      </div>
      {nav ? <div className="avatar-hero-nav">{nav}</div> : null}
      {layout === "block" && src ? (
        <div className="avatar-hero-block">
          <img
            className="avatar-hero-sharp"
            src={src}
            alt=""
            draggable={false}
            referrerPolicy="no-referrer"
            style={{ "--avatar-hero-ar": String(blockAspect) } as CSSProperties}
          />
          {copy}
        </div>
      ) : (
        <div className="avatar-hero-body">{copy}</div>
      )}
    </div>
  );
}
