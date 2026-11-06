/**
 * Line icons for the left sidebar. One 18px grid, 1.5 stroke, round caps, `currentColor`, so
 * hover, active and dark mode follow the link colour. Icons are decorative: the label text
 * stays in the link (visually hidden when the sidebar is collapsed) and names it.
 */
import type { ReactNode } from "react";

export type NavIconName = "avatars" | "assets" | "worlds" | "search" | "log" | "import" | "settings" | "browser";

function Icon({ children, className = "nav-icon" }: { children: ReactNode; className?: string }) {
  return (
    <svg className={className} width="18" height="18" viewBox="0 0 18 18" aria-hidden="true" focusable="false">
      {children}
    </svg>
  );
}

const PATHS: Record<NavIconName, ReactNode> = {
  // A person: head and shoulders.
  avatars: (
    <>
      <circle cx="9" cy="6" r="3" />
      <path d="M3.2 15.5c.8-2.7 3-4.2 5.8-4.2s5 1.5 5.8 4.2" />
    </>
  ),
  // A box: parts and outfits.
  assets: (
    <>
      <path d="M9 2.2 15 5.4v7.2L9 15.8 3 12.6V5.4z" />
      <path d="M3 5.4 9 8.6l6-3.2M9 8.6v7.2" />
    </>
  ),
  // A landscape, mountains and a sun: places to visit. No frame and no globe, so it does not
  // read as the browser.
  worlds: (
    <>
      <circle cx="12.6" cy="5.4" r="1.9" />
      <path d="M1.8 14.6 6.4 7.4l3.3 5 1.9-2.6 4.6 4.8z" />
    </>
  ),
  // A magnifier: search the whole library.
  search: (
    <>
      <circle cx="8" cy="8" r="4.6" />
      <path d="m11.5 11.5 3.8 3.8" />
    </>
  ),
  // A page of lines: a record to read.
  log: (
    <>
      <path d="M4.2 2.5h9.6v13H4.2z" />
      <path d="M6.6 6.2h4.8M6.6 9h4.8M6.6 11.8h3" />
    </>
  ),
  // An arrow into a tray.
  import: (
    <>
      <path d="M9 2.5v8.2M5.6 7.4 9 10.8l3.4-3.4" />
      <path d="M2.8 11.5v3h12.4v-3" />
    </>
  ),
  // Sliders.
  settings: (
    <>
      <path d="M3 4.8h3.2M9.4 4.8H15M3 9h7.2M13.4 9H15M3 13.2h1.6M7.8 13.2H15" />
      <circle cx="7.8" cy="4.8" r="1.6" />
      <circle cx="11.8" cy="9" r="1.6" />
      <circle cx="6.2" cy="13.2" r="1.6" />
    </>
  ),
  // A browser window.
  browser: (
    <>
      <rect x="2.4" y="3.2" width="13.2" height="11.6" rx="2" />
      <path d="M2.4 6.6h13.2M4.8 4.9h.01M6.8 4.9h.01" />
    </>
  ),
};

export function NavIcon({ name }: { name: NavIconName }) {
  return <Icon>{PATHS[name]}</Icon>;
}

/** The app mark: three spines on a shelf, one leaning, for a library of avatars and parts. */
export function BrandMark() {
  return (
    <Icon className="nav-icon brand-mark">
      <path d="M2.6 15.4h12.8" />
      <rect x="3.6" y="3.4" width="2.8" height="12" rx=".8" />
      <rect x="7.6" y="5.4" width="2.8" height="10" rx=".8" />
      <path d="m11.2 6.3 2.6-.7 2.3 9.1-2.6.7z" />
    </Icon>
  );
}

/** The sidebar itself, with an arrow showing which way the toggle moves it. */
export function SidebarToggleIcon({ collapsed }: { collapsed: boolean }) {
  return (
    <Icon>
      <rect x="2.4" y="3.2" width="13.2" height="11.6" rx="2" />
      <path d="M6.6 3.2v11.6" />
      {collapsed ? <path d="m9.6 7 2 2-2 2" /> : <path d="m11.6 7-2 2 2 2" />}
    </Icon>
  );
}

/** A circular arrow: the classification rules changed. */
export function ReclassifyIcon() {
  return (
    <Icon>
      <path d="M14.6 9A5.6 5.6 0 1 1 13 5.1M13.4 2.4v2.9h-2.9" />
    </Icon>
  );
}

/** An arrow up out of a line: an app update is waiting. */
export function UpdateIcon() {
  return (
    <Icon>
      <path d="M9 12.6V3.4M5.4 7 9 3.4 12.6 7M3.4 15h11.2" />
    </Icon>
  );
}
