import { GENRE_LABEL, GENRE_UNSET, genresForKind } from "../types";
import { useGenreBrowse } from "../genreBrowseContext";
import { genreHref } from "../genreBrowseUtils";

function GenreLink({
  href,
  label,
  count,
  active,
}: {
  href: string;
  label: string;
  count: number | undefined;
  active: boolean;
}) {
  const classes = ["genre-link"];
  if (active) classes.push("active");
  if (count === 0) classes.push("genre-link-zero");
  return (
    <a className={classes.join(" ")} href={href} aria-current={active ? "page" : undefined}>
      <span>{label}</span>
      {count !== undefined && <span className="genre-link-count">{count}</span>}
    </a>
  );
}

export function GenreColumn() {
  const { kind, basePath, selected, data } = useGenreBrowse();
  const genres = genresForKind(kind);
  const unsetCount = data?.counts[GENRE_UNSET] ?? 0;
  const showUnset = unsetCount > 0 || selected === GENRE_UNSET;

  return (
    <aside className="genre-rail" aria-label="ジャンル">
      <header className="genre-rail-head">
        <h2>ジャンル</h2>
      </header>
      <nav className="genre-rail-nav">
        <GenreLink href={genreHref(basePath, null)} label="すべて" count={data?.total} active={selected === null} />
        {genres.map((value) => (
          <GenreLink
            key={value}
            href={genreHref(basePath, value)}
            label={GENRE_LABEL[value] ?? value}
            count={data ? data.counts[value] ?? 0 : undefined}
            active={selected === value}
          />
        ))}
        {showUnset && (
          <GenreLink
            href={genreHref(basePath, GENRE_UNSET)}
            label={GENRE_LABEL[GENRE_UNSET]}
            count={data ? unsetCount : undefined}
            active={selected === GENRE_UNSET}
          />
        )}
      </nav>
    </aside>
  );
}
