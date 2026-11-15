import { useAvatarGenre } from "../avatarGenreContext";
import { useGenreBrowse } from "../genreBrowseContext";
import { ChipBar } from "./ChipBar";
import { GenreTarget, useAvatarGenreEntries, useBrowseGenreEntries } from "./GenreColumn";
import type { GenreEntry, GenreOtherGroup } from "./GenreColumn";
import { GenreToggleIcon } from "./genreIcons";

const CHIP = { className: "chip-bar-item", labelClassName: "chip-bar-label", countClassName: "chip-bar-count" };

/** The chip bar shows names only. */
function withoutIcon({ icon: _icon, ...entry }: GenreEntry): GenreEntry {
  return entry;
}

/**
 * The genre column as a one-line strip above the list, for windows where the column would
 * leave the list less than 480px. Same links, counts and drop targets as the column, without
 * icons. When the icon-only column would fit, the strip starts with a button that shows it.
 */
function GenreChipStrip({
  entries,
  other,
  activeKey,
  onShowIcons,
}: {
  entries: GenreEntry[];
  other: GenreOtherGroup | null;
  activeKey: string;
  onShowIcons?: () => void;
}) {
  const iconsLabel = "ジャンルをアイコンの列で表示";
  return (
    <ChipBar label="ジャンル" activeKey={activeKey} className="genre-chip-bar">
      {onShowIcons && (
        <>
          <button type="button" className="chip-bar-icon" aria-label={iconsLabel} title={iconsLabel} onClick={onShowIcons}>
            <GenreToggleIcon collapsed />
          </button>
          <span className="chip-bar-sep" aria-hidden="true" />
        </>
      )}
      {entries.map(withoutIcon).map(({ key, ...entry }) => (
        <GenreTarget {...CHIP} key={key} {...entry} />
      ))}
      {other && (
        <>
          <span className="chip-bar-sep" aria-hidden="true" />
          <span className="chip-bar-kicker">{other.label}</span>
          {other.entries.map(withoutIcon).map(({ key, ...entry }) => (
            <GenreTarget {...CHIP} key={key} {...entry} />
          ))}
        </>
      )}
    </ChipBar>
  );
}

/** The asset or world genres as a chip bar. */
export function GenreChipBar() {
  const { showGenreIcons } = useGenreBrowse();
  const { entries, other, activeKey } = useBrowseGenreEntries();
  return <GenreChipStrip entries={entries} other={other} activeKey={activeKey} onShowIcons={showGenreIcons} />;
}

/** One avatar's item genres as a chip bar. */
export function AvatarGenreChipBar() {
  const avatar = useAvatarGenre();
  const { entries, activeKey } = useAvatarGenreEntries();
  return <GenreChipStrip entries={entries} other={null} activeKey={activeKey} onShowIcons={avatar?.showGenreIcons} />;
}

