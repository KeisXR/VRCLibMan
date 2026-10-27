import { useState } from "react";
import type { DragEvent } from "react";
import { GENRE_FREE, GENRE_LABEL, GENRE_UNSET, genresForKind } from "../types";
import type { GenreKind } from "../types";
import { useGenreBrowse } from "../genreBrowseContext";
import { genreHref } from "../genreBrowseUtils";

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
  label,
  count,
  active,
  dropGenre,
  className,
  showCount = true,
  onDropId,
}: {
  href?: string;
  label: string;
  count?: number;
  active?: boolean;
  dropGenre?: string | null;
  className?: string;
  showCount?: boolean;
  onDropId: (boothId: string) => void;
}) {
  const [over, setOver] = useState(false);
  const classes = [className];
  if (active) classes.push("active");
  if (count === 0) classes.push("genre-link-zero");
  if (over) classes.push("drag-over");
  const classText = classes.filter(Boolean).join(" ");
  const handlers = dropProps(dropGenre ?? null, setOver, onDropId);
  const body = (
    <>
      <span className="genre-link-label" title={label}>{label}</span>
      {showCount && count !== undefined && <span className="genre-link-count">{count}</span>}
    </>
  );
  if (!href) {
    return (
      <button type="button" className={classText} {...handlers}>
        {body}
      </button>
    );
  }
  return (
    <a className={classText} href={href} aria-current={active ? "page" : undefined} {...handlers}>
      {body}
    </a>
  );
}

function otherKind(kind: GenreKind): GenreKind {
  return kind === "world" ? "asset" : "world";
}

export function GenreColumn() {
  const { kind, basePath, selected, data, draggingId, setDraggingId, moveProduct } = useGenreBrowse();
  const genres = genresForKind(kind);
  const unsetCount = data?.counts[GENRE_UNSET] ?? 0;
  const showUnset = unsetCount > 0 || selected === GENRE_UNSET;
  const across = otherKind(kind);
  const drop = (targetKind: GenreKind, genre: string) => (boothId: string) => {
    setDraggingId(null);
    void moveProduct(boothId, targetKind, genre);
  };

  return (
    <aside className="genre-rail" aria-label="ジャンル">
      <header className="genre-rail-head">
        <h2>ジャンル</h2>
      </header>
      <nav className="genre-rail-nav">
        <GenreTarget
          href={genreHref(basePath, null)}
          label="すべて"
          count={data?.total}
          active={selected === null}
          className="genre-link"
          onDropId={() => undefined}
        />
        {genres.map((value) => (
          <GenreTarget
            key={value}
            href={genreHref(basePath, value)}
            label={GENRE_LABEL[value] ?? value}
            count={data ? data.counts[value] ?? 0 : undefined}
            active={selected === value}
            dropGenre={value}
            className="genre-link"
            onDropId={drop(kind, value)}
          />
        ))}
        <GenreTarget
          href={genreHref(basePath, GENRE_FREE)}
          label={GENRE_LABEL[GENRE_FREE]}
          count={data ? data.counts[GENRE_FREE] ?? 0 : undefined}
          active={selected === GENRE_FREE}
          className="genre-link"
          onDropId={() => undefined}
        />
        {showUnset && (
          <GenreTarget
            href={genreHref(basePath, GENRE_UNSET)}
            label={GENRE_LABEL[GENRE_UNSET]}
            count={data ? unsetCount : undefined}
            active={selected === GENRE_UNSET}
            className="genre-link"
            onDropId={() => undefined}
          />
        )}
        {draggingId && (
          <div className="genre-rail-other">
            <p className="genre-rail-kicker">{across === "world" ? "ワールド" : "アセット"}</p>
            {genresForKind(across).map((value) => (
              <GenreTarget
                key={value}
                label={GENRE_LABEL[value] ?? value}
                dropGenre={value}
                className="genre-link"
                onDropId={drop(across, value)}
              />
            ))}
          </div>
        )}
      </nav>
    </aside>
  );
}
