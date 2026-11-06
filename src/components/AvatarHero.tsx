import { useEffect, useRef, useState, type CSSProperties, type ReactNode, type SyntheticEvent } from "react";
import { mediaSrc } from "../media";
import {
  avatarHeroBlockAspect,
  avatarHeroLayout,
  avatarHeroLowRes,
  versionedHeroSrc,
  AVATAR_HERO_LOW_RES_WIDTH,
  type AvatarHeroLayoutState,
  type AvatarHeroSize,
} from "../avatarHero";

export function AvatarHero({
  size,
  path,
  url,
  version,
  name,
  kicker,
  subtitle,
  nav,
  menu,
}: {
  size: Exclude<AvatarHeroSize, "off">;
  path: string | null;
  url: string | null;
  /** 同じパスで差し替えたカスタム画像の読み直し用。変わると計測をやり直す。 */
  version?: number | null;
  name: string;
  kicker: string;
  subtitle: string;
  nav?: ReactNode;
  menu?: ReactNode;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const mediaRef = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState(false);
  const [reduced, setReduced] = useState(false);
  const probeRef = useRef<HTMLImageElement>(null);
  // 計測前は "pending"。wide 固定にすると正方形サムネが全幅 cover で引き伸ばされる。
  const [layout, setLayout] = useState<AvatarHeroLayoutState>("pending");
  const [natural, setNatural] = useState<{ w: number; h: number } | null>(null);
  const src = failed ? null : versionedHeroSrc(mediaSrc(path, url), version);

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
    setLayout("pending");
  }, [path, url, version]);

  const measure = (img: HTMLImageElement | null) => {
    if (!img || !img.complete || !(img.naturalWidth > 0 && img.naturalHeight > 0)) return false;
    const w = img.naturalWidth;
    const h = img.naturalHeight;
    setNatural((prev) => (prev && prev.w === w && prev.h === h ? prev : { w, h }));
    setLayout(avatarHeroLayout(w, h));
    return true;
  };

  // キャッシュ済み・即時完了で onLoad を取り逃しても、マウント時/src 変更時に complete を見て計測する。
  useEffect(() => {
    if (!src) return;
    measure(probeRef.current);
  }, [src]);

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
    measure(event.currentTarget);
  };

  const pending = layout === "pending" && !!src;
  const lowRes = natural && layout !== "pending" ? avatarHeroLowRes(layout, natural.w) : false;
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
            {src && !pending ? (
              <img
                className="avatar-hero-image"
                src={src}
                alt=""
                draggable={false}
                referrerPolicy="no-referrer"
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
        {/* レイアウトに関係なく常設の計測用 img（onLoad + complete 両方で計測） */}
        {src ? (
          <img
            key={src}
            ref={probeRef}
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
      {menu ? <div className="avatar-hero-menu">{menu}</div> : null}
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
