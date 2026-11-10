import { createContext, useContext, useEffect, useMemo, useState } from "react";
import { api } from "./api";
import { useShell } from "./App";
import { selectedGenre } from "./genreBrowseUtils";
import type { GenreKind, GenreProducts } from "./types";
import { errorText } from "./ui";
import { useGenreMove } from "./useGenreMove";

interface GenreBrowseContextValue {
  kind: GenreKind;
  basePath: string;
  routeGenre: string | null;
  selected: string | null;
  /** ほかの画面から開いた商品。一覧で見える位置へ送り、枠で示す。 */
  focusId: string | null;
  data: GenreProducts | null;
  error: string | null;
  /** The genre column does not fit; the page shows the genre chip bar. */
  genreBar: boolean;
  reload: () => void;
  draggingId: string | null;
  setDraggingId: (id: string | null) => void;
  moveProduct: (boothId: string, kind: GenreKind, genre: string) => Promise<void>;
}

const GenreBrowseContext = createContext<GenreBrowseContextValue | null>(null);

export function GenreBrowseProvider({
  kind,
  genre,
  focus = null,
  basePath,
  genreBar,
  children,
}: {
  kind: GenreKind;
  genre: string | null;
  focus?: string | null;
  basePath: string;
  genreBar: boolean;
  children: React.ReactNode;
}) {
  const { revision } = useShell();
  const moveProduct = useGenreMove();
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const selected = selectedGenre(kind, genre);
  const key = `${kind}:${selected ?? ""}`;
  const [loaded, setLoaded] = useState<{ key: string; data: GenreProducts } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    api.productsByGenre(kind, selected).then(
      (data) => {
        if (!cancelled) setLoaded({ key, data });
      },
      (err: unknown) => {
        if (!cancelled) setError(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [kind, selected, key, revision, attempt]);

  const data = loaded?.key === key ? loaded.data : null;
  const value = useMemo(
    () => ({
      kind,
      basePath,
      routeGenre: genre,
      selected,
      focusId: focus,
      data,
      error,
      genreBar,
      reload: () => setAttempt((value) => value + 1),
      draggingId,
      setDraggingId,
      moveProduct,
    }),
    [kind, basePath, genre, selected, focus, data, error, genreBar, draggingId, moveProduct],
  );

  return <GenreBrowseContext.Provider value={value}>{children}</GenreBrowseContext.Provider>;
}

export function useGenreBrowse(): GenreBrowseContextValue {
  const value = useContext(GenreBrowseContext);
  if (!value) throw new Error("genre browse missing");
  return value;
}
