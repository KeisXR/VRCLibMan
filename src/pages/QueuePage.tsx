import { memo, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api, openExternal, openLocal } from "../api";
import { useShell } from "../App";
import { formatBytes } from "../media";
import { CATEGORIES, CATEGORY_LABEL, GENRE_LABEL, genresForCategory } from "../types";
import type { AvatarCandidate, ProductOption, QueueFile, QueueProduct } from "../types";
import { FileMark, Thumb, errorText } from "../ui";
import { ExcludeProductButton } from "../components/ProductExclusions";

type Kind = "unextracted" | "review" | "files";

type AvatarHit = {
  id: string;
  displayName: string;
  haystack: string;
};

type OptionHit = ProductOption & { haystack: string };

type Counts = {
  unextracted: number;
  needsReview: number;
  unlinkedFiles: number;
};

const EMPTY_COUNTS: Counts = { unextracted: 0, needsReview: 0, unlinkedFiles: 0 };

function initialKind(counts: Counts): Kind {
  if (counts.unextracted > 0) return "unextracted";
  if (counts.needsReview > 0) return "review";
  if (counts.unlinkedFiles > 0) return "files";
  return "unextracted";
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

function bigrams(text: string): string[] {
  const chars = [...text];
  const grams: string[] = [];
  for (let index = 0; index < chars.length - 1; index += 1) grams.push(chars[index] + chars[index + 1]);
  return grams;
}

function matchingAvatars(index: readonly AvatarHit[], query: string): AvatarHit[] {
  const needle = query.trim().toLowerCase();
  const ranked = index.map((hit) => {
    if (!needle) return { hit, score: 1 };
    const name = hit.displayName.toLowerCase();
    const included = name.includes(needle) || hit.haystack.includes(needle);
    const score = Math.max(bigramScore(needle, name), bigramScore(needle, hit.haystack), included ? 0.8 : 0);
    return { hit, score };
  });
  const visible = needle ? ranked.filter((row) => row.score > 0) : ranked;
  visible.sort(
    (left, right) =>
      right.score - left.score || left.hit.displayName.localeCompare(right.hit.displayName, "ja"),
  );
  return visible.slice(0, 80).map((row) => row.hit);
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

export function QueuePage() {
  const { revision, refresh, notify, fail } = useShell();
  const [kind, setKind] = useState<Kind | null>(null);
  const [choseTab, setChoseTab] = useState(false);
  const [counts, setCounts] = useState<Counts>(EMPTY_COUNTS);
  const [products, setProducts] = useState<QueueProduct[]>([]);
  const [files, setFiles] = useState<QueueFile[]>([]);
  const [loadedKind, setLoadedKind] = useState<Kind | null>(null);
  const [avatarIndex, setAvatarIndex] = useState<AvatarHit[]>([]);
  const [nameById, setNameById] = useState<ReadonlyMap<string, string>>(() => new Map());
  const [options, setOptions] = useState<OptionHit[]>([]);
  const [candidates, setCandidates] = useState<AvatarCandidate[]>([]);

  const choose = useCallback((next: Kind) => {
    setChoseTab(true);
    setKind(next);
  }, []);

  const onDone = useCallback(
    (message: string) => {
      notify(message);
      refresh();
    },
    [notify, refresh],
  );

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.knowledge(), api.productOptions(), api.avatarCandidates()]).then(
      ([known, productOptions, nextCandidates]) => {
        if (cancelled) return;
        const index = known.avatars.map((avatar) => ({
          id: avatar.id,
          displayName: avatar.displayName,
          haystack: [avatar.displayName, ...avatar.aliases].join(" ").toLowerCase(),
        }));
        const names = new Map<string, string>();
        for (const avatar of index) names.set(avatar.id, avatar.displayName);
        setAvatarIndex(index);
        setNameById(names);
        setOptions(
          productOptions.map((option) => ({
            ...option,
            haystack: `${option.title} ${option.shop} ${option.boothId}`.toLowerCase(),
          })),
        );
        setCandidates(nextCandidates);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision, fail]);

  useEffect(() => {
    if (choseTab) return;
    let cancelled = false;
    api.stats().then(
      (stats) => {
        if (cancelled) return;
        const nextCounts = {
          unextracted: stats.unextracted,
          needsReview: stats.needsReview,
          unlinkedFiles: stats.unlinkedFiles,
        };
        setCounts(nextCounts);
        setKind(initialKind(nextCounts));
      },
      (err: unknown) => {
        if (cancelled) return;
        fail(errorText(err));
        setKind("unextracted");
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision, choseTab, fail]);

  useEffect(() => {
    if (!kind) return;
    let cancelled = false;
    api.queue(kind).then(
      (queue) => {
        if (cancelled) return;
        setCounts({
          unextracted: queue.unextracted,
          needsReview: queue.needsReview,
          unlinkedFiles: queue.unlinkedFiles,
        });
        setProducts(queue.products);
        setFiles(queue.files);
        setLoadedKind(kind);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [kind, revision, fail]);

  const visible = loadedKind === kind;

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>確認</h1>
        </div>
      </div>
      <div className="tabs">
        <button className={kind === "unextracted" ? "tab active" : "tab"} type="button" onClick={() => choose("unextracted")}>
          未分類 {counts.unextracted}
        </button>
        <button className={kind === "review" ? "tab active" : "tab"} type="button" onClick={() => choose("review")}>
          要確認 {counts.needsReview}
        </button>
        <button className={kind === "files" ? "tab active" : "tab"} type="button" onClick={() => choose("files")}>
          商品未設定のファイル {counts.unlinkedFiles}
        </button>
      </div>
      {visible && kind !== "files" && products.length === 0 && <p className="empty">この一覧は空です。</p>}
      {visible && kind === "files" && files.length === 0 && <p className="empty">商品未設定のファイルはありません。</p>}
      {kind === "review" && (
        <CandidateList candidates={candidates} avatarIndex={avatarIndex} onDone={onDone} onError={fail} />
      )}
      <div className="stack">
        {visible && kind !== "files" &&
          products.map((product) => (
            <ProductEditor
              key={product.boothId}
              product={product}
              avatarIndex={avatarIndex}
              nameById={nameById}
              onDone={onDone}
              onError={fail}
            />
          ))}
        {visible && kind === "files" &&
          files.map((file) => (
            <FileEditor key={file.path} file={file} options={options} onDone={onDone} onError={fail} />
          ))}
      </div>
    </section>
  );
}

const CandidateList = memo(function CandidateList({
  candidates,
  avatarIndex,
  onDone,
  onError,
}: {
  candidates: AvatarCandidate[];
  avatarIndex: readonly AvatarHit[];
  onDone: (message: string) => void;
  onError: (message: string) => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const selectedRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    selectedRef.current?.scrollIntoView({ block: "nearest" });
  }, [editing]);

  const register = useCallback(
    async (name: string) => {
      try {
        await api.registerAvatarCandidate(name);
        onDone(`${name} をアバターとして登録しました`);
      } catch (err) {
        onError(errorText(err));
      }
    },
    [onDone, onError],
  );

  const alias = useCallback(
    async (name: string, avatarId: string) => {
      try {
        await api.aliasAvatarCandidate(name, avatarId);
        setEditing((current) => (current === name ? null : current));
        onDone(`${name} を別名にしました`);
      } catch (err) {
        onError(errorText(err));
      }
    },
    [onDone, onError],
  );

  if (candidates.length === 0) return null;

  return (
    <div className="panel" style={{ marginBottom: 16 }}>
      <h2>新しいアバター候補</h2>
      <p className="meta">
        対応表に未登録のアバター名です。
      </p>
      <div className="stack queue-candidate-list">
        {candidates.map((candidate) => {
          const selected = editing === candidate.name;
          return (
            <div
              className={selected ? "candidate-block is-selected" : "candidate-block"}
              key={candidate.name}
              ref={selected ? selectedRef : undefined}
            >
              <div className="candidate">
                <strong>{candidate.name}</strong>
                <span className="meta">関連商品 {candidate.productCount} 件</span>
                <button className="button primary" type="button" onClick={() => void register(candidate.name)}>
                  アバターとして登録
                </button>
                <button
                  className="button"
                  type="button"
                  aria-expanded={selected}
                  onClick={() => setEditing(selected ? null : candidate.name)}
                >
                  既存の別名にする
                </button>
              </div>
              {selected && (
                <AliasCombo
                  suggestion={candidate.name}
                  avatarIndex={avatarIndex}
                  onApply={(avatarId) => void alias(candidate.name, avatarId)}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
});

function AliasCombo({
  suggestion,
  avatarIndex,
  onApply,
}: {
  suggestion: string;
  avatarIndex: readonly AvatarHit[];
  onApply: (avatarId: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const activeRef = useRef<HTMLButtonElement>(null);
  const hits = useMemo(() => matchingAvatars(avatarIndex, query), [avatarIndex, query]);
  const safeCursor = hits.length === 0 ? 0 : Math.min(cursor, hits.length - 1);
  const pending = hits[safeCursor];

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    setCursor(0);
  }, [query]);

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [safeCursor, query]);

  const apply = useCallback(
    (avatarId: string | undefined) => {
      if (avatarId) onApply(avatarId);
    },
    [onApply],
  );

  return (
    <div className="alias-combo">
      <input
        ref={inputRef}
        className="search"
        value={query}
        placeholder="登録先のアバターを検索"
        role="combobox"
        aria-expanded="true"
        aria-autocomplete="list"
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setCursor((current) => Math.min(current + 1, Math.max(hits.length - 1, 0)));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setCursor((current) => Math.max(current - 1, 0));
          } else if (event.key === "Enter") {
            event.preventDefault();
            apply(hits[safeCursor]?.id);
          } else if (event.key === "Escape") {
            event.preventDefault();
            setQuery("");
          }
        }}
      />
      <p className="meta alias-preview">
        {pending ? `${suggestion} → ${pending.displayName}` : `${suggestion} → 一致するアバターがありません`}
      </p>
      <div className="alias-options" role="listbox">
        {hits.map((avatar, index) => (
          <button
            key={avatar.id}
            className={index === safeCursor ? "alias-option is-active" : "alias-option"}
            type="button"
            role="option"
            aria-selected={index === safeCursor}
            ref={index === safeCursor ? activeRef : undefined}
            onMouseEnter={() => setCursor(index)}
            onClick={() => onApply(avatar.id)}
          >
            {avatar.displayName}
          </button>
        ))}
      </div>
    </div>
  );
}

const ProductEditor = memo(function ProductEditor({
  product,
  avatarIndex,
  nameById,
  onDone,
  onError,
}: {
  product: QueueProduct;
  avatarIndex: readonly AvatarHit[];
  nameById: ReadonlyMap<string, string>;
  onDone: (message: string) => void;
  onError: (message: string) => void;
}) {
  const [category, setCategory] = useState(product.category ?? "");
  const [genre, setGenre] = useState(product.genre ?? "");
  const [generic, setGeneric] = useState(product.isGeneric);
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set(product.linkedAvatarIds));
  const [filter, setFilter] = useState("");
  const deferred = useDeferredValue(filter);
  const hits = useMemo(() => rankAvatars(avatarIndex, deferred, 30), [avatarIndex, deferred]);

  const toggle = useCallback((id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const save = useCallback(
    async (confirm: boolean) => {
      if (!category) {
        onError("カテゴリを選んでから保存してください");
        return;
      }
      try {
        await api.updateProduct({
          boothId: product.boothId,
          category,
          genre: genre || null,
          isGeneric: generic,
          avatarIds: [...selected],
          confirm,
        });
        onDone(confirm ? "確認済みにしました" : "修正を保存しました");
      } catch (err) {
        onError(errorText(err));
      }
    },
    [category, genre, generic, onDone, onError, product.boothId, selected],
  );

  const addAlias = useCallback(
    async (name: string) => {
      try {
        const id = await api.saveAvatar({
          displayName: name,
          aliases: [name],
          notes: "",
          baseBody: null,
        });
        setSelected((current) => {
          if (current.has(id)) return current;
          const next = new Set(current);
          next.add(id);
          return next;
        });
        onDone(`「${name}」を対応表に追加しました。保存するとこの商品に結び付きます。`);
      } catch (err) {
        onError(errorText(err));
      }
    },
    [onDone, onError],
  );

  const markOwned = useCallback(async () => {
    if (!window.confirm("この商品を、持っているアバター本体として記録します。")) return;
    try {
      await api.markOwnedAvatar(product.boothId);
      onDone("所持アバターにしました");
    } catch (err) {
      onError(errorText(err));
    }
  }, [onDone, onError, product.boothId]);

  const showOwn = product.category === "avatar" || /オリジナル3dモデル/i.test(product.title);

  return (
    <article className="queue-card">
      <div className="queue-main">
        <Thumb path={product.thumbnailPath} url={product.thumbnailUrl} name={product.title} />
        <div>
          <h2 className="card-title">
            {product.title}
            <FileMark
              presence={product.filePresence}
              href={
                product.category !== "avatar" && product.linkedAvatarIds[0]
                  ? `#/avatar/${encodeURIComponent(product.linkedAvatarIds[0])}/files/${encodeURIComponent(product.boothId)}`
                  : undefined
              }
            />
          </h2>
          <div className="meta">
            {product.shop || "ショップ不明"}
          </div>
          {product.descriptionSnippet && <p className="meta">{product.descriptionSnippet.slice(0, 280)}</p>}
          <button className="button" type="button" onClick={() => void openExternal(product.url)}>
            BOOTH を開く
          </button>
          <ExcludeProductButton boothId={product.boothId} title={product.title} />
        </div>
      </div>
      {product.unresolvedNames.length > 0 && (
        <div className="pills">
          {product.unresolvedNames.map((name) => (
            <button key={name} className="chip warn" type="button" onClick={() => void addAlias(name)}>
              未登録: {name} を対応表へ
            </button>
          ))}
        </div>
      )}
      <div className="form-grid">
        <label className="field">
          <span>カテゴリ</span>
          <select value={category} onChange={(event) => { const next = event.target.value; setCategory(next); setGenre((current) => (genresForCategory(next).includes(current) ? current : "")); }}>
            <option value="">未分類</option>
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
      </div>
      <div className="row-actions">
        {showOwn && (
          <button className="button primary" type="button" onClick={() => void markOwned()}>
            所持アバターにする
          </button>
        )}
        <button className="button" type="button" onClick={() => void save(false)}>
          保存
        </button>
        <button className="button primary" type="button" onClick={() => void save(true)}>
          確認済みにする
        </button>
      </div>
    </article>
  );
});

const FileEditor = memo(function FileEditor({
  file,
  options,
  onDone,
  onError,
}: {
  file: QueueFile;
  options: readonly OptionHit[];
  onDone: (message: string) => void;
  onError: (message: string) => void;
}) {
  const [query, setQuery] = useState("");
  const deferred = useDeferredValue(query);
  const [productId, setProductId] = useState(file.candidateProductId ?? "");
  const visible = useMemo(() => {
    const needle = deferred.trim().toLowerCase();
    if (!needle) return options.slice(0, 30);
    const matched: OptionHit[] = [];
    for (const option of options) {
      if (!option.haystack.includes(needle)) continue;
      matched.push(option);
      if (matched.length >= 30) break;
    }
    return matched;
  }, [deferred, options]);

  const link = useCallback(
    async (id: string | null) => {
      try {
        await api.linkFile(file.path, id);
        onDone(id ? "ファイルを商品に結び付けました" : "結びつきを外しました");
      } catch (err) {
        onError(errorText(err));
      }
    },
    [file.path, onDone, onError],
  );

  return (
    <article className="queue-card">
      <div>
        <h2 className="card-title">{file.filename}</h2>
        <div className="meta">
          {file.isDir ? "フォルダ" : "ファイル"} ・ {formatBytes(file.size)}
        </div>
        <button className="button" type="button" onClick={() => void openLocal(file.path)}>
          開く
        </button>
      </div>
      {file.candidateProductId && (
        <button className="button" type="button" onClick={() => void link(file.candidateProductId)}>
          候補「{file.candidateTitle ?? file.candidateProductId}」に結び付ける
        </button>
      )}
      <div className="form-grid">
        <input className="search" value={query} placeholder="商品を検索" onChange={(event) => setQuery(event.target.value)} />
        <select value={productId} onChange={(event) => setProductId(event.target.value)}>
          <option value="">商品を選ぶ</option>
          {visible.map((option) => (
            <option key={option.boothId} value={option.boothId}>
              {option.title}
            </option>
          ))}
        </select>
      </div>
      <button className="button primary" type="button" disabled={!productId} onClick={() => void link(productId)}>
        選択した商品に結び付ける
      </button>
    </article>
  );
});
