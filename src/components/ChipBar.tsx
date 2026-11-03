import { useLayoutEffect, useRef } from "react";
import type { ReactNode } from "react";

/**
 * A one-line strip of choices that stays at the top of the scrolling page (`.chip-bar`).
 * It scrolls sideways when the choices do not fit. Used for the genres of a narrow window and
 * meant for other section pickers too (see docs/design/README.md#チップ帯).
 *
 * Children are links or buttons with `.chip-bar-item`; the current one has `.active` and
 * `aria-current`. An optional `.chip-bar-count` shows a count, `.chip-bar-sep` and
 * `.chip-bar-kicker` divide groups. `activeKey` changes when the current choice changes, and
 * the strip then scrolls sideways (never vertically) to keep that choice in view.
 */
export function ChipBar({
  label,
  activeKey,
  className,
  children,
}: {
  label: string;
  activeKey?: string | null;
  className?: string;
  children: ReactNode;
}) {
  const trackRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const track = trackRef.current;
    if (!track) return;
    const active = track.querySelector<HTMLElement>(".chip-bar-item.active");
    if (!active) return;
    const start = active.offsetLeft - track.offsetLeft;
    const end = start + active.offsetWidth;
    if (start < track.scrollLeft) track.scrollLeft = Math.max(0, start - 8);
    else if (end > track.scrollLeft + track.clientWidth) track.scrollLeft = end - track.clientWidth + 8;
  }, [activeKey]);

  return (
    <nav className={className ? `chip-bar ${className}` : "chip-bar"} aria-label={label}>
      <div className="chip-bar-track" ref={trackRef}>
        {children}
      </div>
    </nav>
  );
}
