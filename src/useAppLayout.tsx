import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api";
import {
  UI_GENRE_AUTO_COLLAPSE_PX,
  UI_GENRE_WIDTH,
  UI_RAIL_WIDTH,
  clampGenreWidth,
  clampRailWidth,
  fitBrowseWidths,
} from "./uiLayout";

export function useAppLayout(browseGenre: boolean) {
  const [railWidth, setRailWidth] = useState(UI_RAIL_WIDTH.default);
  const [genreWidth, setGenreWidth] = useState(UI_GENRE_WIDTH.default);
  const [genreCollapsed, setGenreCollapsed] = useState(false);
  const [windowWidth, setWindowWidth] = useState(
    typeof window !== "undefined" ? window.innerWidth : 1280,
  );

  useEffect(() => {
    void api.settings().then((settings) => {
      setRailWidth(clampRailWidth(settings.uiRailWidth));
      setGenreWidth(clampGenreWidth(settings.uiGenreWidth));
      setGenreCollapsed(settings.uiGenreCollapsed);
    });
  }, []);

  useEffect(() => {
    const onResize = () => setWindowWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const narrow = windowWidth < UI_GENRE_AUTO_COLLAPSE_PX;
  const effectiveCollapsed = genreCollapsed || narrow;
  const genreVisible = browseGenre && !effectiveCollapsed;

  const fitted = useMemo(
    () => fitBrowseWidths(windowWidth, railWidth, genreWidth, genreVisible),
    [windowWidth, railWidth, genreWidth, genreVisible],
  );

  const persistRail = useCallback((value: number) => {
    const next = clampRailWidth(value);
    void api.updateSettings({ uiRailWidth: next });
  }, []);

  const persistGenre = useCallback((value: number) => {
    const next = clampGenreWidth(value);
    void api.updateSettings({ uiGenreWidth: next });
  }, []);

  const persistGenreCollapsed = useCallback((value: boolean) => {
    setGenreCollapsed(value);
    void api.updateSettings({ uiGenreCollapsed: value });
  }, []);

  const gridStyle = useMemo(() => {
    const cols: string[] = [`${fitted.rail}px`, "6px"];
    if (genreVisible) cols.push(`${fitted.genre}px`, "6px");
    cols.push("minmax(0, 1fr)");
    return {
      gridTemplateColumns: cols.join(" "),
      ["--sidebar-width" as string]: `${fitted.rail}px`,
      ["--genre-width" as string]: `${fitted.genre}px`,
    };
  }, [fitted, genreVisible]);

  return {
    railWidth: fitted.rail,
    genreWidth: fitted.genre,
    genreCollapsed,
    effectiveCollapsed,
    genreVisible,
    narrow,
    gridStyle,
    setRailWidth,
    setGenreWidth,
    persistRail,
    persistGenre,
    persistGenreCollapsed,
  };
}
