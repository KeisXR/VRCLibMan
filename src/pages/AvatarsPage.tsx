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
      if (!needle) return true;
      const haystack = [card.displayName, card.baseBodyName ?? "", ...card.aliases]
        .join(" ")
        .toLowerCase();
      return haystack.includes(needle);
    });
  }, [cards, query]);

  const groups = useMemo(() => {
    if (!grouped) return [{ title: "", cards: filtered }];
    const order: string[] = [];
    const map = new Map<string, { title: string; cards: AvatarCard[] }>();
    for (const card of filtered) {
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
  }, [filtered, grouped]);

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>アバター</h1>
          <p>持っている素体と、同じ素体で着られる衣装です。</p>
        </div>
        <div className="toolbar">
          <input
            className="search"
            value={query}
            placeholder="名前で絞り込む"
            onChange={(event) => setQuery(event.target.value)}
          />
          <button className="button" type="button" onClick={() => setGrouped((value) => !value)}>
            {grouped ? "素体でまとめている" : "一覧で表示"}
          </button>
        </div>
      </div>
      {cards && cards.length === 0 && (
        <div className="empty">
          まだ持っているアバターがありません。Booth の拡張で今見ているページを JSON
          にし、取り込みから読み込み、ローカルの Ollama で分類すると、ここへ並びます。
        </div>
      )}
      {cards && cards.length > 0 && filtered.length === 0 && <div className="empty">一致するアバターがありません。</div>}
      {groups.map((group) => (
        <div className="group" key={group.title || "flat"}>
          {group.title && <h2>{group.title}</h2>}
          <div className="cards">
            {group.cards.map((card) => (
              <a className="card avatar-card" key={card.id} href={`#/avatar/${encodeURIComponent(card.id)}`}>
                <Thumb path={card.thumbnailPath} url={card.thumbnailUrl} name={card.displayName} />
                <div>
                  <h2 className="card-title">{card.displayName}</h2>
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
    </section>
  );
}
