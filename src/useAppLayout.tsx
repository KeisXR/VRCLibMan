import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api";
import { fitRightPanelWidth, rightPanelMaxWidth, RIGHT_PANEL_SEPARATOR } from "./rightPanel";
import {
  UI_GENRE_AUTO_COLLAPSE_PX,
  UI_GENRE_WIDTH,
  UI_RAIL_WIDTH,
  clampGenreWidth,
  clampRailWidth,
  fitBrowseWidths,
} from "./uiLayout";

export interface PanelLayoutInput {
  open: boolean;
  maximized: boolean;
  width: number;
}

export function useAppLayout(browseGenre: boolean, panel: PanelLayoutInput = { open: false, maximized: false, width: 0 }) {
  const [railWidth, setRailWidth] = useState(UI_RAIL_WIDTH.default);
  const [genreWidth, setGenreWidth] = useState(UI_GENRE_WIDTH.default);
  const [windowWidth, setWindowWidth] = useState(
    typeof window !== "undefined" ? window.innerWidth : 1280,
  );

  useEffect(() => {
    void api.settings().then((settings) => {
      setRailWidth(clampRailWidth(settings.uiRailWidth));
      setGenreWidth(clampGenreWidth(settings.uiGenreWidth));
    });
  }, []);

  useEffect(() => {
    const onResize = () => setWindowWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  // Narrow windows show a genre picker above the list instead of the column,
  // and the right panel over the page instead of beside it.
  const narrow = windowWidth < UI_GENRE_AUTO_COLLAPSE_PX;
  const genreVisible = browseGenre && !narrow;
  const panelDocked = panel.open && !narrow;

  const leftColumns = useMemo(() => {
    const rail = clampRailWidth(railWidth) + 6;
    return genreVisible ? rail + clampGenreWidth(genreWidth) + 6 : rail;
  }, [railWidth, genreWidth, genreVisible]);
  const panelMax = rightPanelMaxWidth(windowWidth, leftColumns);
  const panelWidth = panelDocked ? fitRightPanelWidth(panel.width, windowWidth, leftColumns) : 0;
  const panelMaximized = panelDocked && panel.maximized;

  const fitted = useMemo(
    () =>
      fitBrowseWidths(
        panelDocked && !panelMaximized ? windowWidth - panelWidth - RIGHT_PANEL_SEPARATOR : windowWidth,
        railWidth,
        genreWidth,
        genreVisible,
      ),
    [windowWidth, railWidth, genreWidth, genreVisible, panelDocked, panelMaximized, panelWidth],
  );

  const persistRail = useCallback((value: number) => {
    const next = clampRailWidth(value);
    void api.updateSettings({ uiRailWidth: next });
  }, []);

  const persistGenre = useCallback((value: number) => {
    const next = clampGenreWidth(value);
    void api.updateSettings({ uiGenreWidth: next });
  }, []);

  const gridStyle = useMemo(() => {
    const cols: string[] = [`${fitted.rail}px`, "6px"];
    if (genreVisible) cols.push(`${fitted.genre}px`, "6px");
    if (panelMaximized) cols.push("minmax(0, 1fr)");
    else {
      cols.push("minmax(0, 1fr)");
      if (panelDocked) cols.push(`${RIGHT_PANEL_SEPARATOR}px`, `${panelWidth}px`);
    }
    return {
      gridTemplateColumns: cols.join(" "),
      ["--sidebar-width" as string]: `${fitted.rail}px`,
      ["--genre-width" as string]: `${fitted.genre}px`,
      ["--right-panel-width" as string]: `${panelWidth}px`,
    };
  }, [fitted, genreVisible, panelDocked, panelMaximized, panelWidth]);

  return {
    railWidth: fitted.rail,
    genreWidth: fitted.genre,
    genreVisible,
    narrow,
    panelDocked,
    panelMaximized,
    panelWidth,
    panelMax,
    gridStyle,
    setRailWidth,
    setGenreWidth,
    persistRail,
    persistGenre,
  };
}
