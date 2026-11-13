import { useEffect, useMemo, useState } from "react";
import { openPageSection } from "../anchorScroll";
import { api } from "../api";
import { useShell } from "../App";
import { CATEGORY_LABEL } from "../types";
import type { AvatarCard, LibraryStats } from "../types";
import { Thumb, errorText } from "../ui";

export type OwnedAvatarsHint =
  | { kind: "no-products" }
  | { kind: "unclassified"; count: number }
  | { kind: "no-body" };

/**
 * 持っているアバターが 1 体も無いときの案内。読み込んでいないのか、分類していないのか、
 * 分類したが本体が見つからなかったのかで、次にすることが違う。
 */
export function ownedAvatarsHint(stats: LibraryStats | null, ownedCount: number): OwnedAvatarsHint | null {
  if (!stats || ownedCount > 0) return null;
  if (stats.products === 0) return { kind: "no-products" };
  if (stats.unextracted > 0) return { kind: "unclassified", count: stats.unextracted };
  return { kind: "no-body" };
}

function OwnedAvatarsEmpty({ hint }: { hint: OwnedAvatarsHint }) {
  if (hint.kind === "no-products") {
    return (
      <div className="empty">
        <p>BOOTH の商品情報をまだ読み込んでいません。読み込んで分類すると、持っているアバターと対応する衣装がここに出ます。</p>
        <a className="button secondary" href="#/import" onClick={(event) => { event.preventDefault(); openPageSection("#/import", "import-booth"); }}>
          BOOTH から読み込む
        </a>
      </div>
    );
  }
  if (hint.kind === "unclassified") {
    return (
      <div className="empty">
        <p>読み込んだ商品のうち {hint.count} 件がまだ分類されていません。分類すると、アバター本体と対応する衣装がここに出ます。</p>
        <a className="button secondary" href="#/import" onClick={(event) => { event.preventDefault(); openPageSection("#/import", "import-classification"); }}>
          分類へ
        </a>
      </div>
    );
  }
  return (
    <div className="empty">
      <p>持っているアバター本体が見つかりませんでした。アバター本体の商品が別のジャンルに入っているときは、その商品カードの「⋯ → アバター本体として登録」で登録できます。</p>
      <a className="button secondary" href="#/assets">アセットを見る</a>
    </div>
  );
}

export function AvatarsPage() {
  const { revision, fail } = useShell();
  const [cards, setCards] = useState<AvatarCard[] | null>(null);
  const [query, setQuery] = useState("");
  const [grouped, setGrouped] = useState(true);
  const [onlyOwned, setOnlyOwned] = useState(false);
  const [stats, setStats] = useState<LibraryStats | null>(null);

  useEffect(() => {
    let cancelled = false;
    api.avatars().then(
      (value) => {
        if (!cancelled) setCards(value);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    api.stats().then(
      (value) => {
        if (!cancelled) setStats(value);
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [revision, fail]);

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return (cards ?? []).filter((card) => {
      if (onlyOwned && !card.owned) return false;
      if (!needle) return true;
      const haystack = [card.displayName, card.baseBodyName ?? "", ...card.aliases]
        .join(" ")
        .toLowerCase();
      return haystack.includes(needle);
    });
  }, [cards, query, onlyOwned]);
  // 一覧を読めたあとだけ案内する（読み込み中に「見つかりません」と出さない）。
  const hint = cards ? ownedAvatarsHint(stats, cards.filter((card) => card.owned).length) : null;

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>アバター</h1>
        </div>
        <div className="toolbar">
          <input
            className="search"
            value={query}
            placeholder="名前で絞り込む"
            onChange={(event) => setQuery(event.target.value)}
          />
          <label className="switch-label">
            <input type="checkbox" checked={onlyOwned} onChange={(event) => setOnlyOwned(event.target.checked)} />
            持っているアバターだけ表示
          </label>
          <button className="button" type="button" onClick={() => setGrouped((value) => !value)}>
            {grouped ? "素体別" : "一覧"}
          </button>
        </div>
      </div>
      {hint && !query.trim() && (
        <div className="ownership">
          {!onlyOwned && <h2 className="ownership-title">持っているアバター</h2>}
          <OwnedAvatarsEmpty hint={hint} />
        </div>
      )}
      {cards && cards.length > 0 && filtered.length === 0 && (query.trim() || !hint) && <div className="empty">一致するアバターがありません。</div>}
      <AvatarGroups title={onlyOwned ? "" : "持っているアバター"} cards={filtered.filter((card) => card.owned)} grouped={grouped} />
      {!onlyOwned && (
        <AvatarGroups title="衣装の対応先" cards={filtered.filter((card) => !card.owned)} grouped={grouped} note="所持が登録されていないアバターです。" />
      )}
    </section>
  );
}

/** One avatar card linking to its detail page. Shared with the library search results. */
export function AvatarCardLink({ card }: { card: AvatarCard }) {
  return (
    <a className="card avatar-card" href={`#/avatar/${encodeURIComponent(card.id)}`}>
      <Thumb path={card.thumbnailPath} url={card.thumbnailUrl} name={card.displayName} />
      <div>
        <h2 className="card-title">{card.displayName}</h2>
        <div className="pills">
          <span className="pill">{CATEGORY_LABEL.outfit} {card.outfitCount}</span>
          <span className="pill">{CATEGORY_LABEL.hair} {card.hairCount}</span>
          <span className="pill">{CATEGORY_LABEL.gimmick} {card.gimmickCount}</span>
          {card.genericCount > 0 && <span className="pill">汎用 {card.genericCount}</span>}
        </div>
      </div>
    </a>
  );
}

function AvatarGroups({
  title,
  note,
  cards,
  grouped,
}: {
  title: string;
  note?: string;
  cards: AvatarCard[];
  grouped: boolean;
}) {
  const groups = useMemo(() => {
    if (cards.length === 0) return [];
    if (!grouped) return [{ title: "", cards }];
    const order: string[] = [];
    const map = new Map<string, { title: string; cards: AvatarCard[] }>();
    for (const card of cards) {
      const key = card.baseBody || "solo";
      const existing = map.get(key);
      if (existing) {
        existing.cards.push(card);
      } else {
        order.push(key);
        map.set(key, {
          title: card.baseBodyName ?? "個別のアバター",
          cards: [card],
        });
      }
    }
    return order.map((key) => map.get(key)!);
  }, [cards, grouped]);

  if (cards.length === 0) return null;

  return (
    <div className="ownership">
      {title && <h2 className="ownership-title">{title}</h2>}
      {note && <p className="meta">{note}</p>}
      {groups.map((group) => (
        <div className="group" key={`${title}-${group.title || "flat"}`}>
          {group.title && <h2>{group.title}</h2>}
          <div className="cards">
            {group.cards.map((card) => (
              <AvatarCardLink key={card.id} card={card} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
