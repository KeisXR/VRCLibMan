import { useEffect, useMemo, useState } from "react";
import { api, openExternal, openLocal } from "../api";
import { useShell } from "../App";
import { confidenceLabel, formatBytes } from "../media";
import { CATEGORIES, CATEGORY_LABEL } from "../types";
import type { AvatarCandidate, Knowledge, ProductOption, QueueFile, QueuePage as QueueData, QueueProduct } from "../types";
import { Thumb, errorText } from "../ui";

type Kind = "unextracted" | "review" | "files";

function queueCount(page: QueueData, kind: Kind): number {
  if (kind === "unextracted") return page.unextracted;
  if (kind === "files") return page.unlinkedFiles;
  return page.needsReview;
}

export function QueuePage() {
  const { revision, refresh, notify, fail } = useShell();
  const [kind, setKind] = useState<Kind>("unextracted");
  const [choseTab, setChoseTab] = useState(false);
  const [page, setPage] = useState<QueueData | null>(null);
  const [loadedKind, setLoadedKind] = useState<Kind | null>(null);
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [options, setOptions] = useState<ProductOption[]>([]);
  const [candidates, setCandidates] = useState<AvatarCandidate[]>([]);
  const [aliasTarget, setAliasTarget] = useState<Record<string, string>>({});

  function choose(next: Kind) {
    setChoseTab(true);
    setKind(next);
  }

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.queue(kind), api.knowledge(), api.productOptions(), api.avatarCandidates()]).then(
      ([queue, known, products, nextCandidates]) => {
        if (cancelled) return;
        setPage(queue);
        setKnowledge(known);
        setOptions(products);
        setCandidates(nextCandidates);
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

  useEffect(() => {
    if (!page || choseTab || loadedKind !== kind) return;
    if (queueCount(page, kind) > 0) return;
    const next = (["unextracted", "review", "files"] as Kind[]).find((item) => queueCount(page, item) > 0);
    if (next && next !== kind) setKind(next);
  }, [page, choseTab, kind, loadedKind]);

  const visible = loadedKind === kind ? page : null;

  async function registerCandidate(name: string) {
    try {
      await api.registerAvatarCandidate(name);
      notify(`${name} をアバターとして登録しました`);
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function aliasCandidate(name: string) {
    const avatarId = aliasTarget[name];
    if (!avatarId) {
      fail("別名を足すアバターを選んでください");
      return;
    }
    try {
      await api.aliasAvatarCandidate(name, avatarId);
      notify(`${name} を別名にしました`);
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>確認</h1>
          <p>分類できていない商品と、フォルダにあって商品と結び付いていないファイルです。</p>
        </div>
      </div>
      <div className="tabs">
        <button className={kind === "unextracted" ? "tab active" : "tab"} type="button" onClick={() => choose("unextracted")}>
          未抽出 {page ? page.unextracted : "–"}
        </button>
        <button className={kind === "review" ? "tab active" : "tab"} type="button" onClick={() => choose("review")}>
          要確認 {page ? page.needsReview : "–"}
        </button>
        <button className={kind === "files" ? "tab active" : "tab"} type="button" onClick={() => choose("files")}>
          未突合 {page ? page.unlinkedFiles : "–"}
        </button>
      </div>
      {visible && kind !== "files" && visible.products.length === 0 && <p className="empty">この一覧は空です。</p>}
      {visible && kind === "files" && visible.files.length === 0 && <p className="empty">未突合のファイルはありません。</p>}
      {kind === "review" && candidates.length > 0 && (
        <div className="panel" style={{ marginBottom: 16 }}>
          <h2>新しいアバター候補</h2>
          <p className="meta">
            商品名の【対応】から拾った、対応表にない名前です。件数はその名前が付いている商品の数です。
          </p>
          <div className="stack">
            {candidates.map((candidate) => (
              <div className="candidate" key={candidate.name}>
                <strong>{candidate.name}</strong>
                <span className="meta">{candidate.productCount} 件</span>
                <button className="button primary" type="button" onClick={() => void registerCandidate(candidate.name)}>
                  アバターとして登録
                </button>
                <select
                  value={aliasTarget[candidate.name] ?? ""}
                  onChange={(event) =>
                    setAliasTarget((current) => ({ ...current, [candidate.name]: event.target.value }))
                  }
                >
                  <option value="">既存のアバター</option>
                  {(knowledge?.avatars ?? []).map((avatar) => (
                    <option key={avatar.id} value={avatar.id}>
                      {avatar.displayName}
                    </option>
                  ))}
                </select>
                <button className="button" type="button" onClick={() => void aliasCandidate(candidate.name)}>
                  この別名にする
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
      <div className="stack">
        {visible && kind !== "files" &&
          visible.products.map((product) => (
            <ProductEditor
              key={product.boothId}
              product={product}
              knowledge={knowledge}
              onDone={(message) => {
                notify(message);
                refresh();
              }}
              onError={fail}
            />
          ))}
        {visible && kind === "files" &&
          visible.files.map((file) => (
            <FileEditor
              key={file.path}
              file={file}
              options={options}
              onDone={(message) => {
                notify(message);
                refresh();
              }}
              onError={fail}
            />
          ))}
      </div>
    </section>
  );
}

function ProductEditor({
  product,
  knowledge,
  onDone,
  onError,
}: {
  product: QueueProduct;
  knowledge: Knowledge | null;
  onDone: (message: string) => void;
  onError: (message: string) => void;
}) {
  const [category, setCategory] = useState(product.category ?? "");
  const [generic, setGeneric] = useState(product.isGeneric);
  const [selected, setSelected] = useState<string[]>(product.linkedAvatarIds);
  const [filter, setFilter] = useState("");
  const avatars = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return (knowledge?.avatars ?? []).filter((avatar) => {
      if (!needle) return true;
      return [avatar.displayName, ...avatar.aliases].join(" ").toLowerCase().includes(needle);
    });
  }, [knowledge, filter]);

  function toggle(id: string) {
    setSelected((current) => (current.includes(id) ? current.filter((item) => item !== id) : [...current, id]));
  }

  async function save(confirm: boolean) {
    if (!category) {
      onError("カテゴリを選んでから保存してください");
      return;
    }
    try {
      await api.updateProduct({
        boothId: product.boothId,
        category,
        isGeneric: generic,
        avatarIds: selected,
        confirm,
      });
      onDone(confirm ? "確認済みにしました" : "修正を保存しました");
    } catch (err) {
      onError(errorText(err));
    }
  }

  async function addAlias(name: string) {
    try {
      const id = await api.saveAvatar({
        displayName: name,
        aliases: [name],
        notes: "",
        baseBody: null,
      });
      setSelected((current) => (current.includes(id) ? current : [...current, id]));
      onDone(`「${name}」を対応表に追加しました。保存するとこの商品に結び付きます。`);
    } catch (err) {
      onError(errorText(err));
    }
  }

  return (
    <article className="queue-card">
      <div className="queue-main">
        <Thumb path={product.thumbnailPath} url={product.thumbnailUrl} name={product.title} />
        <div>
          <h2 className="card-title">{product.title}</h2>
          <div className="meta">
            {product.shop || "ショップ不明"} ・ 確信度 {confidenceLabel(product.confidence)}
          </div>
          {product.descriptionSnippet && <p className="meta">{product.descriptionSnippet.slice(0, 280)}</p>}
          <button className="button" type="button" onClick={() => void openExternal(product.url)}>
            Booth を開く
          </button>
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
          <select value={category} onChange={(event) => setCategory(event.target.value)}>
            <option value="">未分類</option>
            {CATEGORIES.map((item) => (
              <option key={item} value={item}>
                {CATEGORY_LABEL[item]}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>汎用</span>
          <input type="checkbox" checked={generic} onChange={(event) => setGeneric(event.target.checked)} />
        </label>
      </div>
      <label className="field">
        <span>対応アバター</span>
        <input className="search" value={filter} placeholder="対応表を検索" onChange={(event) => setFilter(event.target.value)} />
        <div className="checks">
          {avatars.map((avatar) => (
            <label key={avatar.id}>
              <input
                type="checkbox"
                checked={selected.includes(avatar.id)}
                onChange={() => toggle(avatar.id)}
              />
              {avatar.displayName}
            </label>
          ))}
        </div>
      </label>
      <div className="row-actions">
        {(product.category === "avatar" || /オリジナル3dモデル/i.test(product.title)) && (
          <button
            className="button primary"
            type="button"
            onClick={() => {
              if (!window.confirm("この商品を、持っているアバター本体として記録します。")) return;
              void (async () => {
                try {
                  await api.markOwnedAvatar(product.boothId);
                  onDone("所持アバターにしました");
                } catch (err) {
                  onError(errorText(err));
                }
              })();
            }}
          >
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
}

function FileEditor({
  file,
  options,
  onDone,
  onError,
}: {
  file: QueueFile;
  options: ProductOption[];
  onDone: (message: string) => void;
  onError: (message: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [productId, setProductId] = useState(file.candidateProductId ?? "");
  const visible = options.filter((option) => {
    const needle = query.trim().toLowerCase();
    if (!needle) return true;
    return `${option.title} ${option.shop} ${option.boothId}`.toLowerCase().includes(needle);
  }).slice(0, 80);

  async function link(id: string | null) {
    try {
      await api.linkFile(file.path, id);
      onDone(id ? "ファイルを商品に結び付けました" : "結びつきを外しました");
    } catch (err) {
      onError(errorText(err));
    }
  }

  return (
    <article className="queue-card">
      <div>
        <h2 className="card-title">{file.filename}</h2>
        <div className="meta">
          {file.isDir ? "フォルダ" : "ファイル"} ・ {formatBytes(file.size)} ・ スコア {file.matchScore.toFixed(2)}
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
}
