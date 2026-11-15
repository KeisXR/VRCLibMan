import { createContext, useCallback, useContext, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { sameAvatarGenreCounts, selectedAvatarGenre } from "./avatarGenre";
import type { AvatarGenreCounts } from "./avatarGenre";

interface AvatarGenreValue {
  avatarId: string;
  /** The genre in the route, or null for "すべて". */
  selected: string | null;
  /** Counts of this avatar's items. Null until the page has loaded this avatar. */
  counts: AvatarGenreCounts | null;
  /** The detail page hands its counts to the column, which App draws outside the page. */
  publish: (counts: AvatarGenreCounts | null) => void;
  /** The genre column does not fit; the page shows the genre chip bar. */
  genreBar: boolean;
  /** Set when the chip bar can switch to the icon-only column. */
  showGenreIcons?: () => void;
}

const AvatarGenreContext = createContext<AvatarGenreValue | null>(null);

export function AvatarGenreProvider({
  avatarId,
  genre,
  genreBar,
  showGenreIcons,
  children,
}: {
  avatarId: string;
  genre: string | null;
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
  const selected = selectedAvatarGenre(genre);
  const value = useMemo(
    () => ({ avatarId, selected, counts, publish, genreBar, showGenreIcons }),
    [avatarId, selected, counts, publish, genreBar, showGenreIcons],
  );
  return <AvatarGenreContext.Provider value={value}>{children}</AvatarGenreContext.Provider>;
}

/** Null outside the avatar route (a page rendered on its own in tests). */
export function useAvatarGenre(): AvatarGenreValue | null {
  return useContext(AvatarGenreContext);
}

