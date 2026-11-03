import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { RIGHT_PANEL_SEPARATOR } from "./rightPanel";
import { UI_GENRE_WIDTH, UI_RAIL_WIDTH, clampGenreWidth, clampRailWidth, computeAppLayout } from "./uiLayout";

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
  const layout = useMemo(
    () =>
      computeAppLayout({
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
  const { genreVisible, panelDocked, panelMaximized, panelWidth } = layout;

  const persistRail = useCallback((value: number) => {
    const next = clampRailWidth(value);
    void api.updateSettings({ uiRailWidth: next });
  }, []);

  // The width does not animate: the docked browser page is a native view placed from the DOM,
  // so the new columns are applied in one frame and the panel re-measures right after.
  const setRailCollapsed = useCallback((next: boolean) => {
    railToggled.current = true;
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
    setRailWidth,
    setGenreWidth,
    persistRail,
    persistGenre,
  };
}
