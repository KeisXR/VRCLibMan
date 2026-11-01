/**
 * Small line icons shared by the right panel and the browser controls. They draw with
 * `currentColor`, so hover and disabled states follow the surrounding button.
 */
export function CloseGlyph() {
  return (
    <svg className="glyph" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M1.5 1.5 8.5 8.5M8.5 1.5 1.5 8.5" />
    </svg>
  );
}

export function PlusGlyph() {
  return (
    <svg className="glyph" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M6 1.5v9M1.5 6h9" />
    </svg>
  );
}

export function ArrowGlyph({ direction }: { direction: "back" | "forward" }) {
  return (
    <svg className="glyph" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      {direction === "back" ? <path d="M12 7H2.5M6.5 3 2.5 7l4 4" /> : <path d="M2 7h9.5M7.5 3l4 4-4 4" />}
    </svg>
  );
}

export function ReloadGlyph() {
  return (
    <svg className="glyph" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <path d="M11.5 7a4.5 4.5 0 1 1-1.3-3.2M11.5 1.8v2.4H9.1" />
    </svg>
  );
}

export function StopGlyph() {
  return (
    <svg className="glyph" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M2 2l8 8M10 2 2 10" />
    </svg>
  );
}

export function HomeGlyph() {
  return (
    <svg className="glyph" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <path d="M2 6.5 7 2.5l5 4M3.5 5.5v6h7v-6" />
    </svg>
  );
}

export function StarGlyph({ filled }: { filled: boolean }) {
  return (
    <svg className={filled ? "glyph filled" : "glyph"} width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <path d="M7 1.8l1.6 3.3 3.6.5-2.6 2.5.6 3.6L7 10l-3.2 1.7.6-3.6-2.6-2.5 3.6-.5z" />
    </svg>
  );
}

/** A window with an arrow out: show this in its own window. */
export function PopOutGlyph() {
  return (
    <svg className="glyph" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <path d="M8.5 2H12v3.5M12 2 7 7M10.5 8.5V12H2V3.5h3.5" />
    </svg>
  );
}

/** A window with the right column filled: show this in the right panel. */
export function DockGlyph() {
  return (
    <svg className="glyph" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <path d="M2 2.5h10v9H2zM8.5 2.5v9" />
    </svg>
  );
}
