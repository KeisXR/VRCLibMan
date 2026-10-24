import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import type { ItemView } from "../types";
import { errorText } from "../ui";
import { ItemCard } from "./AvatarDetailPage";

export function WorldsPage() {
  const { revision, fail } = useShell();
  const [items, setItems] = useState<ItemView[] | null>(null);
  const [filter, setFilter] = useState("");

  useEffect(() => {
    let cancelled = false;
    api.productsByCategory("world").then(
      (value) => {
        if (!cancelled) setItems(value);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision, fail]);

  const shown = useMemo(() => {
    const query = filter.trim().toLocaleLowerCase("ja");
    if (!query) return items ?? [];
    return (items ?? []).filter((item) =>
      `${item.title}\n${item.shop}`.toLocaleLowerCase("ja").includes(query),
    );
  }, [items, filter]);

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>ワールド</h1>
          <p>取り込んだワールド商品を、アバターとの結び付きに関係なく表示します。</p>
        </div>
        <input
          className="search"
          type="search"
          aria-label="ワールドを絞り込む"
          placeholder="商品名・ショップ名で絞り込み"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </div>
      {items === null ? (
        <p className="meta">読み込み中</p>
      ) : shown.length > 0 ? (
        <div className="stack">
          {shown.map((item) => <ItemCard key={item.boothId} item={item} />)}
        </div>
      ) : (
        <p className="empty">
          {filter.trim() ? "条件に合うワールドはありません。" : "ワールド商品はまだありません。"}
        </p>
      )}
    </section>
  );
}
