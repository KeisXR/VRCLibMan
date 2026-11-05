import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { RIGHT_PANEL_SEPARATOR } from "./rightPanel";
import { UI_GENRE_WIDTH, UI_RAIL_WIDTH, clampGenreWidth, clampRailWidth, computeAppLayout, railMotionAttrs, railMotionHoldsPanel } from "./uiLayout";
import type { AppLayoutInput, RailMotion } from "./uiLayout";

/** How long the toggle keeps the motion attributes on (width 140ms + label 30ms delay + 120ms, rounded up). */
export const RAIL_MOTION_HOLD_MS = 220;

export interface PanelLayoutInput {
  open: boolean;
  maximized: boolean;
  width: number;
}

/** The window width, or 1280 where there is none to measure (server rendering, DOM stubs). */
function currentWindowWidth() {
  const width = typeof window !== "undefined" ? window.innerWidth : NaN;
  return Number.isFinite(width) && width > 0 ? width : 1280;
}

export function useAppLayout(browseGenre: boolean, panel: PanelLayoutInput = { open: false, maximized: false, width: 0 }) {
  const [railWidth, setRailWidth] = useState(UI_RAIL_WIDTH.default);
  const [railCollapsed, setRailCollapsedState] = useState(false);
  const railToggled = useRef(false);
  const [genreWidth, setGenreWidth] = useState(UI_GENRE_WIDTH.default);
  const [windowWidth, setWindowWidth] = useState(currentWindowWidth);
  // Set only by the toggle, so window resizes, separator drags and the first load never animate.
  const [railMotion, setRailMotion] = useState<RailMotion>("none");
  const railMotionTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => { if (railMotionTimer.current) clearTimeout(railMotionTimer.current); }, []);

  useEffect(() => {
    void api.settings().then((settings) => {
      setRailWidth(clampRailWidth(settings.uiRailWidth));
      if (!railToggled.current) setRailCollapsedState(settings.uiRailCollapsed === true);
      setGenreWidth(clampGenreWidth(settings.uiGenreWidth));
    });
  }, []);

  useEffect(() => {
    const onResize = () => setWindowWidth(currentWindowWidth());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // See computeAppLayout: icon rail at 860px or less (not saved), the genre column only when
  // the list keeps 480px, and the right panel over the page below 1000px.
  const layoutInput = useMemo<AppLayoutInput>(
    () => ({
      windowWidth,
      railWidth,
      railCollapsed,
      genreWidth,
      browseGenre,
      panelOpen: panel.open,
      panelMaximized: panel.maximized,
      panelStoredWidth: panel.width,
    }),
    [windowWidth, railWidth, railCollapsed, genreWidth, browseGenre, panel.open, panel.maximized, panel.width],
  );
  const layout = useMemo(() => computeAppLayout(layoutInput), [layoutInput]);
  const layoutInputRef = useRef(layoutInput);
  layoutInputRef.current = layoutInput;
  const { genreVisible, panelDocked, panelMaximized, panelWidth } = layout;

  const persistRail = useCallback((value: number) => {
    const next = clampRailWidth(value);
    void api.updateSettings({ uiRailWidth: next });
  }, []);

  // The width slides on every toggle. With the right panel open the docked page (a native view)
  // follows the viewport; if the toggle would also move or resize the panel itself, the page is
  // hidden for the motion instead ("hold").
  const setRailCollapsed = useCallback((next: boolean) => {
    railToggled.current = true;
    const input = layoutInputRef.current;
    const hold = railMotionHoldsPanel(computeAppLayout(input), computeAppLayout({ ...input, railCollapsed: next }));
    setRailMotion(hold ? "hold" : "slide");
    if (railMotionTimer.current) clearTimeout(railMotionTimer.current);
    railMotionTimer.current = setTimeout(() => setRailMotion("none"), RAIL_MOTION_HOLD_MS);
    setRailCollapsedState(next);
    void api.updateSettings({ uiRailCollapsed: next }).catch(() => undefined);
  }, []);

  const persistGenre = useCallback((value: number) => {
    const next = clampGenreWidth(value);
    void api.updateSettings({ uiGenreWidth: next });
  }, []);

  const gridStyle = useMemo(() => {
    const cols: string[] = [`${layout.rail}px`, "6px"];
    if (genreVisible) cols.push(`${layout.genre}px`, "6px");
    if (panelMaximized) cols.push("minmax(0, 1fr)");
    else {
      cols.push("minmax(0, 1fr)");
      if (panelDocked) cols.push(`${RIGHT_PANEL_SEPARATOR}px`, `${panelWidth}px`);
    }
    return {
      gridTemplateColumns: cols.join(" "),
      ["--sidebar-width" as string]: `${layout.rail}px`,
      ["--genre-width" as string]: `${layout.genre}px`,
      ["--right-panel-width" as string]: `${panelWidth}px`,
    };
  }, [layout, genreVisible, panelDocked, panelMaximized, panelWidth]);

  return {
    railWidth: layout.rail,
    /** Icons only on screen (saved as closed, or a narrow window). */
    railCollapsed: layout.railCollapsed,
    /** The saved state; the toggle flips this one. */
    railCollapsedSaved: railCollapsed,
    railAuto: layout.railAuto,
    setRailCollapsed,
    genreWidth: layout.genre,
    genreVisible,
    genreBar: layout.genreBar,
    narrow: layout.narrow,
    panelDocked,
    panelMaximized,
    panelWidth,
    panelMax: layout.panelMax,
    gridStyle,
    /** data-rail-motion for the app shell while a toggle is animating. */
    railMotionAttrs: railMotionAttrs(railMotion),
    setRailWidth,
    setGenreWidth,
    persistRail,
    persistGenre,
  };
}
