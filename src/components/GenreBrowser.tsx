import { useMemo, useState } from "react";
import { ItemCard } from "../pages/AvatarDetailPage";
import { GENRE_LABEL, GENRE_UNSET } from "../types";
import { useGenreBrowse } from "../genreBrowseContext";
import { filterItems, genreHref, selectedGenre } from "../genreBrowseUtils";

const NOUN: Record<string, string> = { asset: "アセット", world: "ワールド" };

export { filterItems, genreHref, selectedGenre } from "../genreBrowseUtils";

export function GenreBrowser({ title, lede }: { title: string; lede?: string }) {
  const { kind, basePath, routeGenre, selected, data, error, narrowLayout, reload } = useGenreBrowse();
  const [filter, setFilter] = useState("");
  const current = data;
  const shown = useMemo(() => filterItems(current?.items ?? [], filter), [current, filter]);
  const noun = NOUN[kind];
  const selectedLabel = selected ? GENRE_LABEL[selected] ?? selected : "すべて";
  const unknownGenre = routeGenre != null && routeGenre !== "" && selectedGenre(kind, routeGenre) === null;

  return (
    <section className="genre-browser">
      {narrowLayout && (
        <details className="genre-mobile-picker">
          <summary>ジャンル：{selectedLabel} ▾</summary>
          <nav className="genre-mobile-nav" aria-label="ジャンル（狭い画面）">
            <a href={genreHref(basePath, null)}>すべて</a>
          </nav>
        </details>
      )}
      <div className="page-head genre-list-head">
        <div>
          <h1>{title}</h1>
          {lede && <p>{lede}</p>}
        </div>
      </div>
      <input
        className="search genre-list-search"
        type="search"
        aria-label={`${title}を絞り込む`}
        placeholder="商品名・ショップ名で絞り込み"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
      />
      {unknownGenre && <p className="note genre-notice">指定されたジャンルが見つからないため、すべてを表示しています。</p>}
      {selected === GENRE_UNSET && (
        <p className="note genre-notice">
          ジャンルが決まっていない{noun}の商品です。確認画面や再分類でジャンルが付くと、各ジャンルに移ります。
        </p>
      )}
      {error ? (
        <div className="empty" role="alert">
          <p>{noun}の一覧を読み込めませんでした。{error}</p>
          <button type="button" onClick={reload}>
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
