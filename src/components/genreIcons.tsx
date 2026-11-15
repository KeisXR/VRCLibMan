/**
 * Line icons for the genre column, drawn like the sidebar icons (navIcons.tsx): one 18px grid,
 * 1.5 stroke, round caps, `currentColor`. The collapsed genre column shows only these, so every
 * genre has its own shape. Icons are decorative; the genre name stays in the link.
 */
import type { ReactNode } from "react";

const PATHS: Record<string, ReactNode> = {
  // Four tiles: everything.
  all: <path d="M3 3h4.6v4.6H3zM10.4 3H15v4.6h-4.6zM3 10.4h4.6V15H3zM10.4 10.4H15V15h-4.6z" />,
  // A T-shirt.
  outfit: <path d="M6.4 2.8 2.6 4.6l-.8 3.6 2.6.9v6.2h9.2V9.1l2.6-.9-.8-3.6-3.8-1.8c-.4 1.2-1.4 2-2.6 2s-2.2-.8-2.6-2z" />,
  // A sock with a cuff.
  shoes_socks: (
    <>
      <path d="M6 2.6h4.6v6.6l3.2 2.3c1 .7 1.2 2 .6 2.9-.4.6-1 .9-1.7.9H7.6C6.7 15.3 6 14.6 6 13.7z" />
      <path d="M6 5.2h4.6" />
    </>
  ),
  // A face framed by long hair, with bangs.
  hair: (
    <>
      <path d="M4.2 15.6c-1-2.4-1.2-5-.6-7.2a5.6 5.6 0 0 1 10.8 0c.6 2.2.4 4.8-.6 7.2" />
      <path d="M6 9.4v1.4a3 3 0 0 0 6 0V9.4" />
      <path d="M6 9.4c2 0 3.6-.9 4.4-2.6.4 1 1 1.9 1.6 2.6" />
    </>
  ),
  // A cut gem.
  accessory: (
    <>
      <path d="M5.4 3h7.2l2.6 3.4L9 15.2 2.8 6.4z" />
      <path d="M2.8 6.4h12.4M9 15.2 6.8 6.4 8.2 3M9 15.2l2.2-8.8L9.8 3" />
    </>
  ),
  // A head with cat ears.
  head_ears_tail: <path d="M4.4 7.8 3.6 2.8l3.6 2.1a5.4 5.4 0 0 1 3.6 0l3.6-2.1-.8 5a5.4 5.4 0 1 1-9.2 0z" />,
  // A lipstick.
  makeup_skin: (
    <>
      <path d="M6.2 15.4h5.6V9.2H6.2zM6.2 12h5.6" />
      <path d="M7.2 9.2V6.4l3.6-3v5.8" />
    </>
  ),
  // An eye.
  eye_texture: (
    <>
      <path d="M1.8 9S4.4 4.4 9 4.4 16.2 9 16.2 9 13.6 13.6 9 13.6 1.8 9 1.8 9z" />
      <circle cx="9" cy="9" r="2.2" />
    </>
  ),
  // A smiling face.
  expression_animation: (
    <>
      <circle cx="9" cy="9" r="6.4" />
      <path d="M6.8 7.2v.6M11.2 7.2v.6M6.4 10.6c.6 1 1.5 1.6 2.6 1.6s2-.6 2.6-1.6" />
    </>
  ),
  // A figure with a raised arm.
  pose: (
    <>
      <circle cx="9" cy="3.4" r="1.6" />
      <path d="M9 5.8v4.6M4.4 4.6 9 7.4l4.4-1.6M9 10.4l-3 5M9 10.4l3 5" />
    </>
  ),
  // A switch: something to turn on in the avatar.
  gimmick: (
    <>
      <path d="M5.6 5.4h6.8a3.6 3.6 0 0 1 0 7.2H5.6a3.6 3.6 0 0 1 0-7.2z" />
      <circle cx="12.4" cy="9" r="1.9" />
    </>
  ),
  // Two sparkles.
  particle_effect: (
    <>
      <path d="M7.6 2.4c.4 3 1.6 4.2 4.6 4.6-3 .4-4.2 1.6-4.6 4.6-.4-3-1.6-4.2-4.6-4.6 3-.4 4.2-1.6 4.6-4.6z" />
      <path d="M13.4 11c.2 1.4.8 2 2.2 2.2-1.4.2-2 .8-2.2 2.2-.2-1.4-.8-2-2.2-2.2 1.4-.2 2-.8 2.2-2.2z" />
    </>
  ),
  // A shaded sphere.
  shader: (
    <>
      <circle cx="9" cy="9" r="6.2" />
      <path d="M5.6 7.2a3.8 3.8 0 0 1 2.4-2.2M3.6 11.8c3 1 7.4-.2 10.6-3.6" />
    </>
  ),
  // A wrench.
  tool_editor: <path d="M11.8 2.6a3.6 3.6 0 0 0-3.4 4.8L3 12.8A1.6 1.6 0 0 0 5.2 15l5.4-5.4a3.6 3.6 0 0 0 4.8-3.4l-2.2 2.2-2-.4-.4-2z" />,
  // More, in a rounded tile.
  other: (
    <>
      <rect x="2.6" y="2.6" width="12.8" height="12.8" rx="3" />
      <path d="M5.8 9h.01M9 9h.01M12.2 9h.01" strokeWidth="2" />
    </>
  ),
  // Mountains and a sun, the same as the sidebar's worlds icon.
  world: (
    <>
      <circle cx="12.6" cy="5.4" r="1.9" />
      <path d="M1.8 14.6 6.4 7.4l3.3 5 1.9-2.6 4.6 4.8z" />
    </>
  ),
  // A bed with a pillow.
  furniture_props: (
    <>
      <path d="M2.6 4v11.2M2.6 11.6h12.8v3.6M2.6 9h12.8v2.6" />
      <path d="M4.6 9V7.2h3.6V9" />
    </>
  ),
  // Code brackets: Udon.
  world_gimmick: <path d="M6 5.4 2.6 9 6 12.6M12 5.4 15.4 9 12 12.6M10.2 4 7.8 14" />,
  // A hatched swatch.
  texture_material: (
    <>
      <rect x="2.8" y="2.8" width="12.4" height="12.4" rx="2" />
      <path d="M2.8 8.4 8.4 2.8M2.8 14 14 2.8M8.4 15.2l6.8-6.8" />
    </>
  ),
  // A cloud.
  skybox: <path d="M5.2 13.8a3.1 3.1 0 0 1-.4-6.2 4.4 4.4 0 0 1 8.4-.8 3.5 3.5 0 0 1 .2 7z" />,
  // A sun.
  lighting_postprocess: (
    <>
      <circle cx="9" cy="9" r="2.8" />
      <path d="M9 1.8v1.8M9 14.4v1.8M1.8 9h1.8M14.4 9h1.8M3.9 3.9l1.3 1.3M12.8 12.8l1.3 1.3M3.9 14.1l1.3-1.3M12.8 5.2l1.3-1.3" />
    </>
  ),
  // Waves: water and other surfaces of a world.
  world_shader: <path d="M2 6.2c1.2-1.2 2.3-1.2 3.5 0s2.3 1.2 3.5 0 2.3-1.2 3.5 0 2.3 1.2 3.5 0M2 10c1.2-1.2 2.3-1.2 3.5 0s2.3 1.2 3.5 0 2.3-1.2 3.5 0 2.3 1.2 3.5 0M2 13.8c1.2-1.2 2.3-1.2 3.5 0s2.3 1.2 3.5 0 2.3-1.2 3.5 0 2.3 1.2 3.5 0" />,
  // A house.
  building_terrain: <path d="M2.6 15.4h12.8M4.2 15.4V7.2L9 3l4.8 4.2v8.2M7.6 15.4v-3.6h2.8v3.6" />,
  // Two notes.
  audio_bgm: (
    <>
      <path d="M7.2 13.2V3.8l7-1.4v9.2" />
      <circle cx="5.4" cy="13.2" r="1.8" />
      <circle cx="12.4" cy="11.6" r="1.8" />
    </>
  ),
  // A toolbox.
  tool_other: <path d="M2.6 6.4h12.8v8.6H2.6zM6.6 6.4V4.2h4.8v2.2M2.6 10.4h12.8" />,
  // A price tag: free downloads.
  free: (
    <>
      <path d="M2.8 9.4V3.2a.4.4 0 0 1 .4-.4h6.2l6 6-6.6 6.6z" />
      <circle cx="6.1" cy="6.1" r="1" />
    </>
  ),
  // A question mark: no genre yet.
  unset: (
    <>
      <circle cx="9" cy="9" r="6.4" />
      <path d="M7.2 7.2a1.9 1.9 0 1 1 2.6 1.8c-.5.2-.8.6-.8 1.1v.5M9 12.6h.01" />
    </>
  ),
};

/** Every key with its own icon: the genres, plus "all", "free" and "unset". */
export const GENRE_ICON_NAMES: readonly string[] = Object.keys(PATHS);

/** One genre's icon. Unknown genres use the "other" icon. */
export function GenreIcon({ name }: { name: string }) {
  return (
    <svg className="nav-icon genre-icon" width="18" height="18" viewBox="0 0 18 18" aria-hidden="true" focusable="false">
      {PATHS[name] ?? PATHS.other}
    </svg>
  );
}

/** The genre column itself, with an arrow showing which way the toggle moves it. */
export function GenreToggleIcon({ collapsed }: { collapsed: boolean }) {
  return (
    <svg className="nav-icon" width="18" height="18" viewBox="0 0 18 18" aria-hidden="true" focusable="false">
      <rect x="2.4" y="3.2" width="13.2" height="11.6" rx="2" />
      <path d="M6.6 3.2v11.6" />
      {collapsed ? <path d="m9.6 7 2 2-2 2" /> : <path d="m11.6 7-2 2 2 2" />}
    </svg>
  );
}
