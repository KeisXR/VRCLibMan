import { createContext, useCallback, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { AVATAR_GENRE_ORDER, sameAvatarGenreCounts } from "./avatarGenre";
import type { AvatarGenreCounts } from "./avatarGenre";
import { useGenreSpy } from "./useGenreSpy";
import type { GenreJump } from "./useGenreSpy";

interface AvatarGenreValue {
  avatarId: string;
  /** Counts of this avatar's genre headings. Null until the page has loaded this avatar. */
  counts: AvatarGenreCounts | null;
  /** The detail page hands its counts to the column, which App draws outside the page. */
  publish: (counts: AvatarGenreCounts | null) => void;
  /** The genre column does not fit; the page shows the genre chip bar. */
  genreBar: boolean;
  /** Set when the chip bar can switch to the icon-only column. */
  showGenreIcons?: () => void;
  /** The genres with a heading on the page, in order. */
  keys: readonly string[];
  /** The genre whose heading the page has scrolled to (useGenreSpy). */
  active: string | null;
  /** The last request to scroll to a heading; the detail page carries it out. */
  jump: GenreJump | null;
  go: (genre: string, smooth?: boolean) => void;
}

const AvatarGenreContext = createContext<AvatarGenreValue | null>(null);

export function AvatarGenreProvider({
  avatarId,
  genreBar,
  showGenreIcons,
  children,
}: {
  avatarId: string;
  genreBar: boolean;
  showGenreIcons?: () => void;
  children: ReactNode;
}) {
  const [published, setPublished] = useState<AvatarGenreCounts | null>(null);
  const publish = useCallback((next: AvatarGenreCounts | null) => {
    setPublished((current) => (sameAvatarGenreCounts(current, next) ? current : next));
  }, []);
  // Another avatar's counts are not shown while this one loads.
  const counts = published?.avatarId === avatarId ? published : null;
  const keys = useMemo(() => AVATAR_GENRE_ORDER.filter((key) => (counts?.counts[key] ?? 0) > 0), [counts]);
  const { active, jump, go } = useGenreSpy(keys);
  const value = useMemo(
    () => ({ avatarId, counts, publish, genreBar, showGenreIcons, keys, active, jump, go }),
    [avatarId, counts, publish, genreBar, showGenreIcons, keys, active, jump, go],
  );
  return <AvatarGenreContext.Provider value={value}>{children}</AvatarGenreContext.Provider>;
}

/** Null outside the avatar route (a page rendered on its own in tests). */
export function useAvatarGenre(): AvatarGenreValue | null {
  return useContext(AvatarGenreContext);
}

