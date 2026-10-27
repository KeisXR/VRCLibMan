import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import { ItemCard } from "../pages/AvatarDetailPage";
import { GENRE_LABEL, genresForKind } from "../types";
import type { GenreKind, GenreProducts, ItemView } from "../types";
import { errorText } from "../ui";

const NOUN: Record<GenreKind, string> = { asset: "アセット", world: "ワールド" };

export function genreHref(basePath: string, genre: string | null): string {
  return genre ? `${basePath}/${encodeURIComponent(genre)}` : basePath;
}

/** Title / shop filter shared by the genre list. */
export function filterItems(items: ItemView[], filter: string): ItemView[] {
  const query = filter.trim().toLocaleLowerCase("ja");
  if (!query) return items;
  return items.filter((item) => `${item.title}\n${item.shop}`.toLocaleLowerCase("ja").includes(query));
}

interface Loaded {
  key: string;
  data: GenreProducts;
}

/** Genre tabs with counts plus the product list of the selected genre. The genre lives in the hash route. */
export function GenreBrowser({
  kind,
  genre,
  basePath,
  title,
  lede,
}: {
  kind: GenreKind;
  genre: string | null;
  basePath: string;
  title: string;
  lede?: string;
}) {
  const { revision } = useShell();
  const genres = genresForKind(kind);
  const selected = genre && genres.includes(genre) ? genre : null;
  const unknownGenre = genre != null && genre !== "" && selected === null;
  const key = `${kind}:${selected ?? ""}`;
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [filter, setFilter] = useState("");

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

  const data = loaded?.data ?? null;
  const current = loaded?.key === key ? loaded.data : null;
  const shown = useMemo(() => filterItems(current?.items ?? [], filter), [current, filter]);
  const noun = NOUN[kind];
  const selectedLabel = selected ? GENRE_LABEL[selected] ?? selected : "すべて";

  return (
    <section className="genre-browser">
      <div className="page-head">
        <div>
          <h1>{title}</h1>
          {lede && <p>{lede}</p>}
        </div>
        <input
          className="search"
          type="search"
          aria-label={`${title}を絞り込む`}
          placeholder="商品名・ショップ名で絞り込み"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </div>
      <nav className="tabs genre-tabs" aria-label={`${title}のジャンル`}>
        <GenreTab href={genreHref(basePath, null)} label="すべて" count={data?.total} active={selected === null} />
        {genres.map((value) => (
          <GenreTab
            key={value}
            href={genreHref(basePath, value)}
            label={GENRE_LABEL[value] ?? value}
            count={data ? data.counts[value] ?? 0 : undefined}
            active={selected === value}
          />
        ))}
      </nav>
      {unknownGenre && <p className="note genre-notice">指定されたジャンルが見つからないため、すべてを表示しています。</p>}
      {error ? (
        <div className="empty" role="alert">
          <p>{noun}の一覧を読み込めませんでした。{error}</p>
          <button type="button" onClick={() => setAttempt((value) => value + 1)}>
            もう一度読み込む
          </button>
        </div>
      ) : current === null ? (
        <p className="meta">読み込み中</p>
      ) : current.total === 0 ? (
        <div className="empty">
          <p>{noun}の商品はまだありません。インポートして分類すると、ジャンル別に表示されます。</p>
          <a href="#/import">インポートへ</a>
        </div>
      ) : current.items.length === 0 ? (
        <p className="empty">「{selectedLabel}」の商品はまだありません。</p>
      ) : shown.length === 0 ? (
        <p className="empty">条件に合う商品はありません。</p>
      ) : (
        <>
          <p className="meta genre-result" role="status">
            {selectedLabel} {shown.length} 件
          </p>
          <div className="stack">
            {shown.map((item) => (
              <ItemCard key={item.boothId} item={item} />
            ))}
          </div>
        </>
      )}
    </section>
  );
}

function GenreTab({ href, label, count, active }: { href: string; label: string; count: number | undefined; active: boolean }) {
  const classes = ["tab"];
  if (active) classes.push("active");
  if (count === 0) classes.push("tab-zero");
  return (
    <a className={classes.join(" ")} href={href} aria-current={active ? "page" : undefined}>
      {label}
      {count !== undefined && <span className="tab-count">{count}</span>}
    </a>
  );
}
