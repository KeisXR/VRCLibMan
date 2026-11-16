import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { useShell } from "./App";
import { selectedGenre } from "./genreBrowseUtils";
import { browseGenreOrder } from "./genreSections";
import type { GenreKind, GenreProducts } from "./types";
import { errorText } from "./ui";
import { useGenreMove } from "./useGenreMove";
import { useGenreSpy } from "./useGenreSpy";
import type { GenreJump } from "./useGenreSpy";

interface GenreBrowseContextValue {
  kind: GenreKind;
  basePath: string;
  /** The genre in the route (`#/assets/<genre>`). The page opens scrolled to its heading. */
  routeGenre: string | null;
  /** ほかの画面から開いた商品。一覧で見える位置へ送り、枠で示す。 */
  focusId: string | null;
  /**
   * The whole list of the kind, headed by genre on the page. A refresh keeps the list on screen
   * until the new one arrives, so cards and thumbnails do not flicker.
   */
  data: GenreProducts | null;
  /** `data`'s kind; changes only when another list arrives. */
  dataKey: string | null;
  /** The list of this kind has not arrived yet. A refresh keeps the old list without this. */
  loading: boolean;
  error: string | null;
  /** The genre column does not fit; the page shows the genre chip bar. */
  genreBar: boolean;
  /** Set when the chip bar can switch to the icon-only column. */
  showGenreIcons?: () => void;
  reload: () => void;
  draggingId: string | null;
  setDraggingId: (id: string | null) => void;
  moveProduct: (boothId: string, kind: GenreKind, genre: string) => Promise<void>;
  /** The order of the genre headings. */
  order: readonly string[];
  /** The genre whose heading the page has scrolled to (useGenreSpy). */
  active: string | null;
  /** The last request to scroll to a heading; GenreBrowser carries it out. */
  jump: GenreJump | null;
  go: (genre: string, smooth?: boolean) => void;
  resync: () => void;
}

const GenreBrowseContext = createContext<GenreBrowseContextValue | null>(null);

export function GenreBrowseProvider({
  kind,
  genre,
  focus = null,
  basePath,
  genreBar,
  showGenreIcons,
  children,
}: {
  kind: GenreKind;
  genre: string | null;
  focus?: string | null;
  basePath: string;
  genreBar: boolean;
  showGenreIcons?: () => void;
  children: React.ReactNode;
}) {
  const { revision } = useShell();
  const moveProduct = useGenreMove();
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [loaded, setLoaded] = useState<{ kind: GenreKind; data: GenreProducts } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const order = useMemo(() => browseGenreOrder(kind), [kind]);
  const { active, jump, go, resync } = useGenreSpy(order);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    api.productsByGenre(kind, null).then(
      (data) => {
        if (!cancelled) setLoaded({ kind, data });
      },
      (err: unknown) => {
        if (!cancelled) setError(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [kind, revision, attempt]);

  // 別の種類（アセットとワールド）の一覧は残さない。画面ごと替わるため。
  const shown = loaded?.kind === kind ? loaded : null;
  const data = shown?.data ?? null;
  const dataKey = shown ? kind : null;
  const loading = !shown;

  // `#/assets/<genre>` opens the list at that heading, once per route.
  const routed = useRef<string | null>(null);
  const target = selectedGenre(kind, genre);
  useEffect(() => {
    const key = `${kind}:${genre ?? ""}`;
    if (!data || routed.current === key) return;
    routed.current = key;
    if (target) go(target, false);
  }, [data, kind, genre, target, go]);

  const value = useMemo(
    () => ({
      kind,
      basePath,
      routeGenre: genre,
      focusId: focus,
      data,
      dataKey,
      loading,
      error,
      genreBar,
      showGenreIcons,
      reload: () => setAttempt((value) => value + 1),
      draggingId,
      setDraggingId,
      moveProduct,
      order,
      active,
      jump,
      go,
      resync,
    }),
    [kind, basePath, genre, focus, data, dataKey, loading, error, genreBar, showGenreIcons, draggingId, moveProduct, order, active, jump, go, resync],
  );

  return <GenreBrowseContext.Provider value={value}>{children}</GenreBrowseContext.Provider>;
}

export function useGenreBrowse(): GenreBrowseContextValue {
  const value = useContext(GenreBrowseContext);
  if (!value) throw new Error("genre browse missing");
  return value;
}

