import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api, openBoothLink } from "../api";
import { useShell } from "../App";
import { avatarHit, rankAvatars } from "../avatarSearch";
import type { AvatarHit } from "../avatarSearch";
import { CATEGORIES, CATEGORY_LABEL, GENRE_LABEL, genresForCategory } from "../types";
import type { ProductEditView, ProductUpdate } from "../types";
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
  const deferred = useDeferredValue(filter);
  const ranked = useMemo(() => rankAvatars(avatarIndex, deferred, 30), [avatarIndex, deferred]);
  const searching = deferred.trim().length > 0;
  const nameById = useMemo(() => new Map(avatarIndex.map((avatar) => [avatar.id, avatar.displayName])), [avatarIndex]);
  const autoLabel =
    !product.genreLocked && product.genre && (category || null) === product.category
      ? `自動（いまは${GENRE_LABEL[product.genre] ?? product.genre}）`
      : "自動";
  dirtyRef.current = editIsDirty(product, { category, genre, generic, selected });

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
          aria-label="対応アバターを名前で探す"
          placeholder="名前を入力すると候補が出ます"
          onChange={(event) => setFilter(event.target.value)}
        />
        {searching && ranked.close.length === 0 && ranked.loose.length === 0 && (
          <p className="meta note-strong" role="status">一致するアバターがありません。名前か別名の一部を入力してください。</p>
        )}
        {ranked.close.length > 0 && (
          <div className="picker">
            {ranked.close.map((avatar) => (
              <label key={avatar.id}>
                <input type="checkbox" checked={selected.has(avatar.id)} onChange={() => toggle(avatar.id)} />
                {avatar.displayName}
              </label>
            ))}
          </div>
        )}
        {ranked.loose.length > 0 && (
          <>
            <p className="meta picker-heading">名前が似ている候補（一致は不確かです。名前を確かめてから選んでください）</p>
            <div className="picker picker-loose">
              {ranked.loose.map((avatar) => (
                <label key={avatar.id}>
                  <input type="checkbox" checked={selected.has(avatar.id)} onChange={() => toggle(avatar.id)} />
                  {avatar.displayName}
                </label>
              ))}
            </div>
          </>
        )}
        <p className="meta">対応アバターを変えると固定され、自動の分類では変わらなくなります。</p>
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
