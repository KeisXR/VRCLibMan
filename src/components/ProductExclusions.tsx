import { useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import { CATEGORY_LABEL } from "../types";
import type { ManagedProduct } from "../types";
import { errorText } from "../ui";

export function ExcludeProductButton({ boothId, title }: { boothId: string; title: string }) {
  const { refresh, notify, fail } = useShell();
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!container.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        trigger.current?.focus();
      }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  return (
    <div className="product-actions" ref={container} onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false);
    }}>
      <button
        ref={trigger}
        className="icon-button product-actions-trigger"
        type="button"
        aria-label={`${title}のその他の操作`}
        title="その他の操作"
        aria-expanded={open}
        disabled={busy}
        onClick={() => setOpen((value) => !value)}
      >
        <span aria-hidden="true">⋯</span>
      </button>
      {open && (
        <div className="product-actions-panel">
          <button
            className="button danger"
            type="button"
            disabled={busy}
            onClick={() => {
              setOpen(false);
              setBusy(true);
              void api
                .setProductExcluded(boothId, true)
                .then(
                  () => {
                    notify("除外しました。インポートの「商品を管理・除外」から元に戻せます");
                    refresh();
                  },
                  (err: unknown) => fail(errorText(err)),
                )
                .finally(() => setBusy(false));
            }}
          >
            一覧から除外
          </button>
        </div>
      )}
    </div>
  );
}

export function ProductExclusions() {
  const { revision, refresh, notify, fail } = useShell();
  const [products, setProducts] = useState<ManagedProduct[] | null>(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [pending, setPending] = useState<Set<string>>(new Set());
  const needle = useDeferredValue(query.trim().toLocaleLowerCase("ja"));

  useEffect(() => {
    let alive = true;
    void api.managedProducts().then(
      (value) => {
        if (alive) setProducts(value);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [revision, fail]);

  const matches = useMemo(
    () =>
      (products ?? []).filter((product) => {
        if (filter === "excluded" && !product.excluded) return false;
        if (filter === "active" && product.excluded) return false;
        return `${product.title} ${product.shop} ${product.boothId}`
          .toLocaleLowerCase("ja")
          .includes(needle);
      }),
    [products, filter, needle],
  );

  async function toggle(product: ManagedProduct) {
    setPending((current) => new Set(current).add(product.boothId));
    try {
      await api.setProductExcluded(product.boothId, !product.excluded);
      notify(
        product.excluded
          ? "除外を解除しました"
          : "一覧と分類の対象から除外しました",
      );
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setPending((current) => {
        const next = new Set(current);
        next.delete(product.boothId);
        return next;
      });
    }
  }

  return (
    <div className="product-exclusions">
      <p>
        VRChat・Unity
        と関係のない商品は除外できます。除外は再インポート後も維持され、いつでも元に戻せます。手元のファイルは削除しません。
      </p>
      <div className="exclusion-filters">
        <label className="field">
          <span>商品を検索</span>
          <input
            value={query}
            placeholder="商品名・ショップ名・商品ID"
            onChange={(event) => setQuery(event.target.value)}
          />
        </label>
        <label className="field">
          <span>表示する商品</span>
          <select
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          >
            <option value="all">すべて</option>
            <option value="active">対象の商品</option>
            <option value="excluded">除外中</option>
          </select>
        </label>
      </div>
      {products == null ? (
        <p className="meta">読み込み中</p>
      ) : (
        <>
          <p className="meta">
            合計 {products.length} 件・除外中{" "}
            {products.filter((product) => product.excluded).length} 件
            {matches.length > 100
              ? `・検索結果 ${matches.length} 件のうち先頭 100 件を表示`
              : ""}
          </p>
          {matches.length === 0 && (
            <p className="meta">該当する商品はありません</p>
          )}
          <ul className="exclusion-list">
            {matches.slice(0, 100).map((product) => (
              <li key={product.boothId}>
                <div>
                  <strong>{product.title}</strong>
                  <span className="meta">
                    {product.shop || "ショップ不明"} ・{product.boothId} ・
                    {product.excluded
                      ? "除外中"
                      : CATEGORY_LABEL[product.category ?? "uncategorized"]}
                  </span>
                </div>
                <button
                  className="button"
                  type="button"
                  disabled={pending.has(product.boothId)}
                  onClick={() => void toggle(product)}
                  aria-label={`${product.title}を${product.excluded ? "元に戻す" : "除外"}`}
                >
                  {product.excluded ? "元に戻す" : "除外"}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
