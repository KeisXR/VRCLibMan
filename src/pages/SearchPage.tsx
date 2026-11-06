import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import type { AvatarCard, ItemView } from "../types";
import { errorText } from "../ui";
import { ItemCard } from "./AvatarDetailPage";
import { AvatarCardLink } from "./AvatarsPage";
import { filterAvatarsByQuery, filterProductsByQuery, searchTokens } from "../searchUtils";

interface SearchData {
  avatars: AvatarCard[];
  assets: ItemView[];
  worlds: ItemView[];
  assetTotal: number;
  worldTotal: number;
}

/**
 * One search over the whole library: avatars, asset products and world products.
 * The query itself lives in the shell (the sidebar search box writes it too), so it
 * survives navigation. Matching is `searchUtils` (whitespace tokens, AND).
 */
export function SearchPage() {
  const { revision, fail, searchQuery, setSearchQuery } = useShell();
  const [data, setData] = useState<SearchData | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      api.avatars(),
      api.productsByGenre("asset", null),
      api.productsByGenre("world", null),
    ]).then(
      ([avatars, assets, worlds]) => {
        if (cancelled) return;
        setData({
          avatars,
          assets: assets.items,
          worlds: worlds.items,
          assetTotal: assets.total,
          worldTotal: worlds.total,
        });
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision, fail]);

  const query = searchQuery ?? "";
  const hasQuery = searchTokens(query).length > 0;
  const foundAvatars = useMemo(
    () => (data && hasQuery ? filterAvatarsByQuery(data.avatars, query) : []),
    [data, hasQuery, query],
  );
  const foundAssets = useMemo(
    () => (data && hasQuery ? filterProductsByQuery(data.assets, query) : []),
    [data, hasQuery, query],
  );
  const foundWorlds = useMemo(
    () => (data && hasQuery ? filterProductsByQuery(data.worlds, query) : []),
    [data, hasQuery, query],
  );
  const total = foundAvatars.length + foundAssets.length + foundWorlds.length;

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>検索</h1>
        </div>
      </div>
      <input
        className="search search-page-input"
        type="search"
        aria-label="ライブラリを検索"
        placeholder="名前・ショップ名・説明で検索"
        value={query}
        autoFocus
        onChange={(event) => setSearchQuery(event.target.value)}
      />
      {data === null ? (
        <p className="meta">読み込み中</p>
      ) : !hasQuery ? (
        <p className="meta">
          アバター {data.avatars.length} 件、アセット {data.assetTotal} 件、ワールド {data.worldTotal} 件から探せます。
        </p>
      ) : total === 0 ? (
        <div className="empty">一致するものがありません。</div>
      ) : (
        <>
          <p className="meta" role="status">
            {total} 件見つかりました
          </p>
          {foundAvatars.length > 0 && (
            <div className="search-group">
              <h2>アバター（{foundAvatars.length}）</h2>
              <div className="cards">
                {foundAvatars.map((card) => (
                  <AvatarCardLink key={card.id} card={card} />
                ))}
              </div>
            </div>
          )}
          {foundAssets.length > 0 && (
            <div className="search-group">
              <h2>アセット（{foundAssets.length}）</h2>
              <div className="stack">
                {foundAssets.map((item) => (
                  <ItemCard key={item.boothId} item={item} />
                ))}
              </div>
            </div>
          )}
          {foundWorlds.length > 0 && (
            <div className="search-group">
              <h2>ワールド（{foundWorlds.length}）</h2>
              <div className="stack">
                {foundWorlds.map((item) => (
                  <ItemCard key={item.boothId} item={item} />
                ))}
              </div>
            </div>
          )}
        </>
      )}
    </section>
  );
}
