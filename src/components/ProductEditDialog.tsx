import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api, openBoothLink } from "../api";
import { useShell } from "../App";
import { avatarHit, rankAvatars } from "../avatarSearch";
import type { AvatarHit } from "../avatarSearch";
import { CATEGORIES, CATEGORY_LABEL, GENRE_LABEL, genresForCategory } from "../types";
import type { ProductEditView, ProductUpdate, PurchasedEditions } from "../types";
import { Thumb, errorText } from "../ui";

function sameSet(left: ReadonlySet<string>, right: readonly string[]): boolean {
  return left.size === right.length && right.every((id) => left.has(id));
}

/** 開いたときから、保存すると送る変更があるか（`save` が送る項目と同じ判定）。 */
export function editIsDirty(
  product: Pick<ProductEditView, "category" | "genre" | "genreLocked" | "isGeneric" | "linkedAvatarIds">,
  form: { category: string; genre: string; generic: boolean; selected: ReadonlySet<string> },
): boolean {
  return (
    Boolean(form.category && form.category !== product.category) ||
    Object.keys(genreChange(product, form.category, form.genre)).length > 0 ||
    form.generic !== product.isGeneric ||
    !sameSet(form.selected, product.linkedAvatarIds)
  );
}

/**
 * 「個別の対応版を指定」の候補。買った対応版は商品の対応範囲の中にあるものだけが意味を持つ
 * （範囲の外の版はアバター詳細にも集計にも使われない）ので、対応範囲で選んだアバターに絞る。
 * 範囲から外したあとも、すでに選んでいた版は外せるように残す（`outside` で示す）。
 */
export function editionCandidates<T extends { id: string }>(
  avatarIndex: readonly T[],
  scope: ReadonlySet<string>,
  chosen: readonly string[],
): { avatar: T; outside: boolean }[] {
  return avatarIndex
    .filter((avatar) => scope.has(avatar.id) || chosen.includes(avatar.id))
    .map((avatar) => ({ avatar, outside: !scope.has(avatar.id) }));
}

/**
 * ジャンル欄の値を API の 3 通りに分ける。空文字は「自動」。
 * 何も送らない＝変更しない、`resetGenre`＝自動に戻す、`genre`＝その値で固定する。
 */
export function genreChange(
  product: Pick<ProductEditView, "category" | "genre" | "genreLocked">,
  category: string,
  genre: string,
): Pick<ProductUpdate, "genre" | "resetGenre"> {
  const categoryChanged = Boolean(category) && category !== product.category;
  const initial = product.genreLocked ? product.genre ?? "" : "";
  if (genre) return genre !== initial || categoryChanged ? { genre } : {};
  // 「自動」: 固定を外すとき、またはカテゴリを変えて新しいカテゴリで選び直すとき。
  return product.genreLocked || categoryChanged ? { resetGenre: true } : {};
}

function EditForm({
  product,
  avatarIndex,
  onClose,
  onCancel,
  dirtyRef,
}: {
  product: ProductEditView;
  avatarIndex: readonly AvatarHit[];
  onClose: () => void;
  /** 保存せずに閉じる。変更があれば確認してから閉じる。 */
  onCancel: () => void;
  /** 保存していない変更があるか。Escape で閉じる前に親が読む。 */
  dirtyRef: { current: boolean };
}) {
  const { refresh, notify, fail } = useShell();
  const [category, setCategory] = useState(product.category ?? "");
  // 自動で決まったジャンルは「自動」として見せる。固定したジャンルだけを値として選ぶ。
  const [genre, setGenre] = useState(product.genreLocked ? product.genre ?? "" : "");
  const [generic, setGeneric] = useState(product.isGeneric);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(product.linkedAvatarIds));
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const initialEditions = product.purchasedEditions ?? { mode: "auto", avatarIds: [] };
  const [editions, setEditions] = useState<PurchasedEditions>(initialEditions);
  const [ownedOnly, setOwnedOnly] = useState(false);
  const [selectedOnly, setSelectedOnly] = useState(false);
  const deferred = useDeferredValue(filter);
  const ranked = useMemo(() => rankAvatars(avatarIndex, deferred, avatarIndex.length), [avatarIndex, deferred]);
  const searching = deferred.trim().length > 0;
  const visible = (searching ? [...ranked.close, ...ranked.loose] : avatarIndex).filter(a => (!ownedOnly || a.owned) && (!selectedOnly || selected.has(a.id)));
  const autoLabel =
    !product.genreLocked && product.genre && (category || null) === product.category
      ? `自動（いまは${GENRE_LABEL[product.genre] ?? product.genre}）`
      : "自動";
  dirtyRef.current = editIsDirty(product, { category, genre, generic, selected }) || JSON.stringify(editions) !== JSON.stringify(initialEditions);

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
        purchasedEditions: JSON.stringify(editions) !== JSON.stringify(initialEditions) ? editions : undefined,
        category: category && category !== product.category ? category : null,
        ...genreChange(product, category, genre),
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
            <option value="">{autoLabel}</option>
            {category && genresForCategory(category).map((item) => <option key={item} value={item}>{GENRE_LABEL[item]}</option>)}
          </select>
        </label>
        <label className="check-row">
          <input type="checkbox" checked={generic} onChange={(event) => setGeneric(event.target.checked)} />
          <span>
            <strong>汎用</strong>
            <span className="meta">特定のアバターに依存せず、どのアバターにも使える商品</span>
          </span>
        </label>
      </div>
      <div className="field">
        <span>商品の対応範囲（選択中：{selected.size}件）</span>
        <p className="meta">この商品がどのアバターに対応しているか（商品の説明の対応アバター）。アバター詳細の一覧と件数はこれで決まります。</p>
        <div className="row-actions">
          <label><input type="checkbox" checked={ownedOnly} onChange={e => setOwnedOnly(e.target.checked)} />所持アバターのみ</label>
          <label><input type="checkbox" checked={selectedOnly} onChange={e => setSelectedOnly(e.target.checked)} />選択済みのみ</label>
        </div>
        <input
          className="search"
          value={filter}
          aria-label="対応アバターを名前で探す"
          placeholder="名前・別名で絞り込み"
          onChange={(event) => setFilter(event.target.value)}
        />
        <div className="picker edition-avatar-list">
          {visible.map(avatar => <label key={avatar.id} className={searching && ranked.loose.some(a => a.id === avatar.id) ? "picker-loose" : undefined}>
            <input type="checkbox" checked={selected.has(avatar.id)} onChange={() => toggle(avatar.id)} />
            {avatar.displayName}
            {searching && ranked.loose.some(a => a.id === avatar.id) && <span className="meta">（名前が似ている候補・一致は不確か）</span>}
          </label>)}
          {visible.length === 0 && <p className="meta" role="status">一致するアバターがありません。</p>}
        </div>
        <p className="meta">対応アバターを変えると固定され、自動の分類では変わらなくなります。</p>
      </div>
      <div className="field">
        <span>購入した対応版</span>
        <p className="meta">アバターごとに分けて売られている商品で、あなたが買った版です。対応範囲の中から選びます。アバター詳細での振り分けと、購入履歴のアバター別の集計に使います。</p>
        <select aria-label="購入した対応版" value={editions.mode} onChange={e => setEditions({ mode: e.target.value as PurchasedEditions["mode"], avatarIds: [] })}>
          <option value="auto">購入情報から判定</option>
          <option value="specific">個別の対応版を指定</option>
          <option value="all">全対応版セット・1版で全アバター対応（商品の対応範囲に従う）</option>
          <option value="unknown">対応版未設定</option>
        </select>
        {editions.mode === "specific" && (() => {
          const candidates = editionCandidates(avatarIndex, selected, editions.avatarIds);
          if (candidates.length === 0) {
            return <p className="note-strong" role="status">先に上の「商品の対応範囲」でアバターを選んでください。買った版はその中から選びます。</p>;
          }
          return <div className="picker edition-avatar-list" aria-label="購入した対応版のアバター">
            {candidates.map(({ avatar: a, outside }) => <label key={a.id}>
              <input type="checkbox" checked={editions.avatarIds.includes(a.id)} onChange={e => setEditions(current => ({ ...current, avatarIds: e.target.checked ? [...current.avatarIds, a.id] : current.avatarIds.filter(id => id !== a.id) }))} />{a.displayName}
              {outside && <span className="meta">（対応範囲の外。外すことをおすすめします）</span>}
            </label>)}
          </div>;
        })()}
        <p className="meta">商品の対応範囲とは別に保存します。アバター本体の所持は変わりません。</p>
      </div>
      <div className="row-actions">
        {showOwn && (
          <button className="button" type="button" disabled={busy} onClick={() => void markOwned()}>
            所持アバターにする
          </button>
        )}
        <button className="button" type="button" disabled={busy} onClick={onCancel}>
          キャンセル
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
  const dirtyRef = useRef(false);

  // 変更があるときだけ、捨ててよいかを尋ねる。誤って Escape を押しても、選び直した対応アバターを失わないため。
  const confirmDiscard = useCallback(
    () => !dirtyRef.current || window.confirm("保存していない変更があります。変更を捨てて閉じますか？"),
    [],
  );
  const cancel = useCallback(() => {
    if (confirmDiscard()) onClose();
  }, [confirmDiscard, onClose]);

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
        setAvatarIndex(known.avatars.map(avatarHit));
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
      onKeyDown={(event) => {
        event.stopPropagation();
        // Escape はキー入力の段階で受け止める。ダイアログ標準の cancel は、直前にクリックや入力が無いと
        // ブラウザが取り消しを許さず、確認で「いいえ」と答えても閉じてしまうため。
        if (event.key !== "Escape" || event.defaultPrevented || event.nativeEvent.isComposing) return;
        event.preventDefault();
        cancel();
      }}
      onCancel={(event) => {
        // キー以外の閉じる要求。取り消せるときだけ確かめ、やめたら開いたままにする。
        if (event.cancelable && !confirmDiscard()) event.preventDefault();
      }}
      onClose={onClose}
    >
      {product ? (
        <EditForm product={product} avatarIndex={avatarIndex} onClose={onClose} onCancel={cancel} dirtyRef={dirtyRef} />
      ) : (
        <p className="meta">読み込み中</p>
      )}
    </dialog>
  );
}
