import { useState } from "react";
import type { DragEvent } from "react";
import { GENRE_FREE, GENRE_LABEL, GENRE_UNSET, genresForKind } from "../types";
import type { GenreKind } from "../types";
import { useAvatarGenre } from "../avatarGenreContext";
import { useGenreBrowse } from "../genreBrowseContext";
import { genreHref } from "../genreBrowseUtils";
import { GenreIcon, GenreToggleIcon } from "./genreIcons";

/**
 * One choice of the genre column or the chip bar. On the asset and world lists it is a link
 * that narrows the list to the genre (`href`); on the avatar detail page it scrolls to the
 * genre's heading (`onSelect`), like the settings page's contents.
 */
export interface GenreEntry {
  key: string;
  href?: string;
  onSelect?: () => void;
  label: string;
  /** Icon name (genreIcons.tsx). The chip bar shows no icons. */
  icon?: string;
  count?: number;
  active?: boolean;
  /** Dropping a product card here moves it to this genre. */
  dropGenre?: string | null;
  onDropId?: (boothId: string) => void;
}

/** Choices shown only while a product card is dragged: the genres of the other kind. */
export interface GenreOtherGroup {
  label: string;
  entries: GenreEntry[];
}

function dropProps(
  genre: string | null,
  setOver: (value: boolean) => void,
  onDropId: (boothId: string) => void,
) {
  if (!genre) return {};
  return {
    "data-drop-genre": genre,
    onDragOver: (event: DragEvent) => {
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      setOver(true);
    },
    onDragLeave: () => setOver(false),
    onDrop: (event: DragEvent) => {
      event.preventDefault();
      setOver(false);
      const id = event.dataTransfer.getData("text/plain");
      if (id) onDropId(id);
    },
  };
}

export function GenreTarget({
  href,
  onSelect,
  label,
  icon,
  count,
  active,
  dropGenre,
  className,
  labelClassName = "genre-link-label",
  countClassName = "genre-link-count",
  showCount = true,
  compact = false,
  onDropId,
}: Omit<GenreEntry, "key"> & {
  className?: string;
  labelClassName?: string;
  countClassName?: string;
  showCount?: boolean;
  /** Icon only: the name and count stay in the link for screen readers and show as a tooltip. */
  compact?: boolean;
}) {
  const [over, setOver] = useState(false);
  const classes = [className];
  if (active) classes.push("active");
  if (count === 0) classes.push("genre-link-zero");
  if (over) classes.push("drag-over");
  const classText = classes.filter(Boolean).join(" ");
  const handlers = dropProps(dropGenre ?? null, setOver, onDropId ?? (() => undefined));
  const tooltip = compact ? (count === undefined ? label : `${label}（${count}）`) : undefined;
  const body = (
    <>
      {icon && <GenreIcon name={icon} />}
      <span className={labelClassName} title={compact ? undefined : label}>{label}</span>
      {showCount && count !== undefined && <span className={countClassName}>{count}</span>}
    </>
  );
  if (!href) {
    return (
      <button
        type="button"
        className={classText}
        title={tooltip}
        aria-current={active ? "location" : undefined}
        onClick={onSelect}
        {...handlers}
      >
        {body}
      </button>
    );
  }
  return (
    <a className={classText} href={href} title={tooltip} aria-current={active ? "page" : undefined} {...handlers}>
      {body}
    </a>
  );
}

/**
 * The genre column beside the list. Closed, it is a narrow column of icons, like the closed
 * left sidebar; the toggle in its head opens and closes it (saved as `ui_genre_collapsed`).
 */
export function GenreRail({
  entries,
  other,
  collapsed,
  onToggle,
}: {
  entries: GenreEntry[];
  other?: GenreOtherGroup | null;
  collapsed: boolean;
  onToggle: () => void;
}) {
  const toggleLabel = collapsed ? "ジャンル列を開く" : "ジャンル列を閉じる";
  return (
    <aside className={collapsed ? "genre-rail collapsed" : "genre-rail"} aria-label="ジャンル" data-collapsed={collapsed ? "true" : "false"}>
      <header className="genre-rail-head">
        <h2>ジャンル</h2>
        <button
          type="button"
          className="genre-rail-toggle"
          aria-expanded={!collapsed}
          aria-controls="genre-rail-nav"
          aria-label={toggleLabel}
          title={toggleLabel}
          onClick={onToggle}
        >
          <GenreToggleIcon collapsed={collapsed} />
        </button>
      </header>
      <nav id="genre-rail-nav" className="genre-rail-nav">
        {entries.map(({ key, ...entry }) => (
          <GenreTarget key={key} {...entry} className="genre-link" compact={collapsed} />
        ))}
        {other && (
          <div className="genre-rail-other">
            <p className="genre-rail-kicker">{other.label}</p>
            {other.entries.map(({ key, ...entry }) => (
              <GenreTarget key={key} {...entry} className="genre-link" compact={collapsed} />
            ))}
          </div>
        )}
      </nav>
    </aside>
  );
}

function otherKind(kind: GenreKind): GenreKind {
  return kind === "world" ? "asset" : "world";
}

/** The asset or world genres with their counts and drop targets, shared by the column and the chip bar. */
export function useBrowseGenreEntries(): { entries: GenreEntry[]; other: GenreOtherGroup | null; activeKey: string } {
  const { kind, basePath, selected, data, draggingId, setDraggingId, moveProduct } = useGenreBrowse();
  const unsetCount = data?.counts[GENRE_UNSET] ?? 0;
  const across = otherKind(kind);
  const drop = (targetKind: GenreKind, genre: string) => (boothId: string) => {
    setDraggingId(null);
    void moveProduct(boothId, targetKind, genre);
  };
  const entries: GenreEntry[] = [
    { key: "", href: genreHref(basePath, null), label: "すべて", icon: "all", count: data?.total, active: selected === null },
    ...genresForKind(kind).map((value) => ({
      key: value,
      href: genreHref(basePath, value),
      label: GENRE_LABEL[value] ?? value,
      icon: value,
      count: data ? data.counts[value] ?? 0 : undefined,
      active: selected === value,
      dropGenre: value,
      onDropId: drop(kind, value),
    })),
    {
      key: GENRE_FREE,
      href: genreHref(basePath, GENRE_FREE),
      label: GENRE_LABEL[GENRE_FREE],
      icon: GENRE_FREE,
      count: data ? data.counts[GENRE_FREE] ?? 0 : undefined,
      active: selected === GENRE_FREE,
    },
  ];
  if (unsetCount > 0 || selected === GENRE_UNSET) {
    entries.push({
      key: GENRE_UNSET,
      href: genreHref(basePath, GENRE_UNSET),
      label: GENRE_LABEL[GENRE_UNSET],
      icon: GENRE_UNSET,
      count: data ? unsetCount : undefined,
      active: selected === GENRE_UNSET,
    });
  }
  const other = draggingId
    ? {
        label: across === "world" ? "ワールド" : "アセット",
        entries: genresForKind(across).map((value) => ({
          key: `${across}-${value}`,
          label: GENRE_LABEL[value] ?? value,
          icon: value,
          dropGenre: value,
          onDropId: drop(across, value),
        })),
      }
    : null;
  return { entries, other, activeKey: selected ?? "" };
}

/**
 * The genre headings of one avatar's items (avatarGenre.ts), only those with items. Counts
 * come from the avatar detail page once it has loaded; there are no drop targets.
 */
export function useAvatarGenreEntries(): { entries: GenreEntry[]; other: null; activeKey: string } {
  const avatar = useAvatarGenre();
  if (!avatar) return { entries: [], other: null, activeKey: "" };
  const { keys, counts, active, go } = avatar;
  const entries: GenreEntry[] = keys.map((value) => ({
    key: value,
    onSelect: () => go(value),
    label: GENRE_LABEL[value] ?? value,
    icon: value,
    count: counts?.counts[value] ?? 0,
    active: active === value,
  }));
  return { entries, other: null, activeKey: active ?? "" };
}

/** The asset or world genre column. */
export function GenreColumn({ collapsed, onToggle }: { collapsed: boolean; onToggle: () => void }) {
  const { entries, other } = useBrowseGenreEntries();
  return <GenreRail entries={entries} other={other} collapsed={collapsed} onToggle={onToggle} />;
}

/** The genre column of the avatar detail page. */
export function AvatarGenreColumn({ collapsed, onToggle }: { collapsed: boolean; onToggle: () => void }) {
  const { entries } = useAvatarGenreEntries();
  return <GenreRail entries={entries} collapsed={collapsed} onToggle={onToggle} />;
}

