import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api, openBoothLink } from "../api";
import { useShell } from "../App";
import { CATEGORIES, CATEGORY_LABEL, GENRE_LABEL, genresForCategory } from "../types";
import type { ProductEditView } from "../types";
import { Thumb, errorText } from "../ui";

type AvatarHit = {
  id: string;
  displayName: string;
  haystack: string;
};

function bigrams(text: string): string[] {
  const chars = [...text];
  const grams: string[] = [];
  for (let index = 0; index < chars.length - 1; index += 1) grams.push(chars[index] + chars[index + 1]);
  return grams;
}

function bigramScore(query: string, name: string): number {
  if (!query || !name) return 0;
  if (name === query) return 1;
  if (name.startsWith(query) || query.startsWith(name)) return 0.92;
  if (name.includes(query) || query.includes(name)) return 0.75;
  const left = bigrams(query);
  const right = bigrams(name);
  if (left.length === 0 || right.length === 0) return 0;
  const counts = new Map<string, number>();
  for (const gram of right) counts.set(gram, (counts.get(gram) ?? 0) + 1);
  let overlap = 0;
  for (const gram of left) {
    const have = counts.get(gram) ?? 0;
    if (have > 0) {
      overlap += 1;
      counts.set(gram, have - 1);
    }
  }
  return (2 * overlap) / (left.length + right.length);
}

function rankAvatars(index: readonly AvatarHit[], query: string, limit: number): AvatarHit[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return [];
  const ranked = index.map((hit) => ({
    hit,
    score: Math.max(bigramScore(needle, hit.displayName.toLowerCase()), bigramScore(needle, hit.haystack)),
  }));
  ranked.sort((left, right) => right.score - left.score || left.hit.displayName.localeCompare(right.hit.displayName, "ja"));
  return ranked.slice(0, limit).map((item) => item.hit);
}

function sameSet(left: ReadonlySet<string>, right: readonly string[]): boolean {
  return left.size === right.length && right.every((id) => left.has(id));
}

function EditForm({
  product,
  avatarIndex,
  onClose,
}: {
  product: ProductEditView;
  avatarIndex: readonly AvatarHit[];
  onClose: () => void;
}) {
  const { refresh, notify, fail } = useShell();
  const [category, setCategory] = useState(product.category ?? "");
  const [genre, setGenre] = useState(product.genre ?? "");
  const [generic, setGeneric] = useState(product.isGeneric);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(product.linkedAvatarIds));
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const deferred = useDeferredValue(filter);
  const hits = useMemo(() => rankAvatars(avatarIndex, deferred, 30), [avatarIndex, deferred]);
  const nameById = useMemo(() => new Map(avatarIndex.map((avatar) => [avatar.id, avatar.displayName])), [avatarIndex]);

  const toggle = useCallback((id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // 変えた項目だけ送る。送った項目は手動の固定になるので、触っていない項目を固定しない。
  async function save() {
    setBusy(true);
    try {
      await api.updateProduct({
        boothId: product.boothId,
        category: category && category !== product.category ? category : null,
        genre: genre && genre !== product.genre ? genre : null,
        isGeneric: generic !== product.isGeneric ? generic : null,
        avatarIds: sameSet(selected, product.linkedAvatarIds) ? null : [...selected],
      });
      notify("修正を保存しました");
      refresh();
      onClose();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function markOwned() {
    if (!window.confirm("この商品を、持っているアバター本体として記録します。")) return;
    setBusy(true);
    try {
      await api.markOwnedAvatar(product.boothId);
      notify("所持アバターにしました");
      refresh();
      onClose();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  const showOwn = product.category === "avatar" || /オリジナル3dモデル/i.test(product.title);

  return (
    <div className="edit-card">
      <div className="edit-main">
        <Thumb path={product.thumbnailPath} url={product.thumbnailUrl} name={product.title} />
        <div>
          <h2 className="card-title">{product.title}</h2>
          <div className="meta">{product.shop || "ショップ不明"}</div>
          <button className="button" type="button" onClick={() => void openBoothLink(product.url)}>
            BOOTH を開く
          </button>
        </div>
      </div>
      <div className="form-grid">
        <label className="field">
          <span>カテゴリ</span>
          <select
            value={category}
            onChange={(event) => {
              const next = event.target.value;
              setCategory(next);
              setGenre((current) => (genresForCategory(next).includes(current) ? current : ""));
            }}
          >
            {!product.category && <option value="">未分類</option>}
            {CATEGORIES.map((item) => (
              <option key={item} value={item}>
                {CATEGORY_LABEL[item]}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>ジャンル</span>
          <select aria-label="ジャンル" value={genre} onChange={(event) => setGenre(event.target.value)}>
            <option value="">自動</option>
            {category && genresForCategory(category).map((item) => <option key={item} value={item}>{GENRE_LABEL[item]}</option>)}
          </select>
        </label>
        <label className="field">
          <span>汎用</span>
          <input type="checkbox" checked={generic} onChange={(event) => setGeneric(event.target.checked)} />
        </label>
      </div>
      <div className="field">
        <span>対応アバター</span>
        <div className="pills">
          {[...selected].map((id) => (
            <button key={id} className="chip" type="button" onClick={() => toggle(id)}>
              {nameById.get(id) ?? id} ×
            </button>
          ))}
        </div>
        <input
          className="search"
          value={filter}
          placeholder="名前を入力すると候補が出ます"
          onChange={(event) => setFilter(event.target.value)}
        />
        {hits.length > 0 && (
          <div className="picker">
            {hits.map((avatar) => (
              <label key={avatar.id}>
                <input type="checkbox" checked={selected.has(avatar.id)} onChange={() => toggle(avatar.id)} />
                {avatar.displayName}
              </label>
            ))}
          </div>
        )}
        <p className="meta">対応アバターを変えると固定され、自動の分類では変わらなくなります。</p>
      </div>
      <div className="row-actions">
        {showOwn && (
          <button className="button" type="button" disabled={busy} onClick={() => void markOwned()}>
            所持アバターにする
          </button>
        )}
        <button className="button" type="button" disabled={busy} onClick={onClose}>
          閉じる
        </button>
        <button className="button primary" type="button" disabled={busy} onClick={() => void save()}>
          保存
        </button>
      </div>
    </div>
  );
}

/** 商品のカテゴリ、ジャンル、対応アバターの手動修正。カードの中に置くので、イベントは外へ漏らさない。 */
export function ProductEditDialog({ boothId, onClose }: { boothId: string; onClose: () => void }) {
  const { fail } = useShell();
  const dialog = useRef<HTMLDialogElement>(null);
  const [product, setProduct] = useState<ProductEditView | null>(null);
  const [avatarIndex, setAvatarIndex] = useState<AvatarHit[]>([]);

  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) element.showModal?.();
  }, []);

  useEffect(() => {
    let alive = true;
    void Promise.all([api.productForEdit(boothId), api.knowledge()]).then(
      ([next, known]) => {
        if (!alive) return;
        setProduct(next);
        setAvatarIndex(
          known.avatars.map((avatar) => ({
            id: avatar.id,
            displayName: avatar.displayName,
            haystack: [avatar.displayName, ...avatar.aliases].join(" ").toLowerCase(),
          })),
        );
      },
      (err: unknown) => {
        if (!alive) return;
        fail(errorText(err));
        onClose();
      },
    );
    return () => {
      alive = false;
    };
  }, [boothId, fail, onClose]);

  return (
    <dialog
      ref={dialog}
      className="product-edit-dialog"
      aria-label="分類を編集"
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => event.stopPropagation()}
      onClose={onClose}
    >
      {product ? (
        <EditForm product={product} avatarIndex={avatarIndex} onClose={onClose} />
      ) : (
        <p className="meta">読み込み中</p>
      )}
    </dialog>
  );
}
