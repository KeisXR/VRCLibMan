import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import { CATEGORY_LABEL } from "../types";
import type { AvatarCard } from "../types";
import { Thumb, errorText } from "../ui";

export function AvatarsPage() {
  const { revision, fail } = useShell();
  const [cards, setCards] = useState<AvatarCard[] | null>(null);
  const [query, setQuery] = useState("");
  const [grouped, setGrouped] = useState(true);
  const [onlyOwned, setOnlyOwned] = useState(false);

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

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>アバター</h1>
          <p>持っているアバターと、衣装の対応先として出てくるだけの名前を分けています。</p>
        </div>
        <div className="toolbar">
          <input
            className="search"
            value={query}
            placeholder="名前で絞り込む"
            onChange={(event) => setQuery(event.target.value)}
          />
          <button className="button" type="button" onClick={() => setOnlyOwned((value) => !value)}>
            {onlyOwned ? "所持のみ" : "所持で絞り込み"}
          </button>
          <button className="button" type="button" onClick={() => setGrouped((value) => !value)}>
            {grouped ? "素体別" : "一覧"}
          </button>
        </div>
      </div>
      {cards && cards.length === 0 && (
        <div className="empty">
          まだ持っているアバターがありません。Booth の拡張で今見ているページを JSON
          にし、インポートから読み込み、ローカルの Ollama で分類すると、ここへ並びます。
        </div>
      )}
      {cards && cards.length > 0 && filtered.length === 0 && <div className="empty">一致するアバターがありません。</div>}
      <AvatarGroups title={onlyOwned ? "" : "持っているアバター"} cards={filtered.filter((card) => card.owned)} grouped={grouped} />
      {!onlyOwned && (
        <AvatarGroups title="衣装の対応先" cards={filtered.filter((card) => !card.owned)} grouped={grouped} note="ライブラリの衣装や小物から名前が出ているだけで、所持アバターとしては記録していません。" />
      )}
    </section>
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
              <a className="card avatar-card" key={card.id} href={`#/avatar/${encodeURIComponent(card.id)}`}>
                <Thumb path={card.thumbnailPath} url={card.thumbnailUrl} name={card.displayName} />
                <div>
                  <h2 className="card-title">{card.displayName}</h2>
                  <div className="meta">{card.owned ? "所持" : "対応先"}</div>
                  {card.isPlaceholder && <div className="meta">仮の名前です。設定で既存のアバターへ統合できます。</div>}
                  <div className="pills">
                    <span className="pill">{CATEGORY_LABEL.outfit} {card.outfitCount}</span>
                    <span className="pill">{CATEGORY_LABEL.hair} {card.hairCount}</span>
                    <span className="pill">{CATEGORY_LABEL.gimmick} {card.gimmickCount}</span>
                    {card.genericCount > 0 && <span className="pill">汎用 {card.genericCount}</span>}
                  </div>
                </div>
              </a>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}
