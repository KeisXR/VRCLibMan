import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import { readWithTimeout } from "../ipcRecovery";
import type { AvatarCard, GenreProducts } from "../types";
import { errorText } from "../ui";
import { ItemCard } from "./AvatarDetailPage";
import { AvatarCardLink } from "./AvatarsPage";
import { filterAvatarsByQuery, filterProductsByQuery, searchTokens } from "../searchUtils";
import { APP_SEARCH_ENTRIES, appEntryNote, appEntryTargets, filterAppEntries } from "../appSearchIndex";
import type { AppSearchContext, AppSearchEntry } from "../appSearchIndex";
import { jumpToAppLocation } from "../appSearchJump";

/** 1 種類ずつの読み込み状態。失敗した種類があっても、読めた種類は検索できる。 */
type Load<T> = { status: "loading" } | { status: "ok"; value: T } | { status: "error"; message: string };

type FullText = { query: string; ids: ReadonlySet<string> } | { query: string; error: string };

const LOADING = { status: "loading" } as const;

/** 「画面と設定」を最初に出す件数。残りは「さらに表示」で開く。 */
export const APP_RESULTS_SHOWN = 8;

/**
 * One search over the whole library: avatars, asset products and world products, plus the
 * screens and settings of the app itself (`appSearchIndex.ts`, shown first).
 * The query itself lives in the shell (the sidebar search box writes it too), so it
 * survives navigation. Matching is `searchUtils` (whitespace tokens, AND) on the cards,
 * plus `search_product_ids` in Rust, which also reads the whole stored description
 * (cards carry only its first 500 characters).
 */
export function SearchPage() {
  const { revision, searchQuery, setSearchQuery, builtinBrowser } = useShell();
  // 拡張機能の欄（ペアリングコードなど）が今の読み込み方で出るか。分からなければ注記を出さない。
  const [extension, setExtension] = useState<boolean | null>(null);
  const [expandedQuery, setExpandedQuery] = useState<string | null>(null);
  const [avatars, setAvatars] = useState<Load<AvatarCard[]>>(LOADING);
  const [assets, setAssets] = useState<Load<GenreProducts>>(LOADING);
  const [worlds, setWorlds] = useState<Load<GenreProducts>>(LOADING);
  const [attempt, setAttempt] = useState(0);
  const [fullText, setFullText] = useState<FullText | null>(null);

  useEffect(() => {
    let cancelled = false;
    // 読めている種類は表示したまま読み直す。失敗していた種類だけ「読み込み中」に戻す。
    function load<T>(label: string, read: () => Promise<T>, apply: (next: Load<T>) => void, reset: boolean) {
      if (reset) apply(LOADING);
      void readWithTimeout(read, label).then(
        (value) => { if (!cancelled) apply({ status: "ok", value }); },
        (err: unknown) => { if (!cancelled) apply({ status: "error", message: errorText(err) }); },
      );
    }
    load("アバター", api.avatars, setAvatars, avatars.status === "error");
    load("アセット", () => api.productsByGenre("asset", null), setAssets, assets.status === "error");
    load("ワールド", () => api.productsByGenre("world", null), setWorlds, worlds.status === "error");
    return () => {
      cancelled = true;
    };
    // The statuses only decide which kinds show 「読み込み中」 again; they do not trigger a load.
  }, [revision, attempt]);

  useEffect(() => {
    let cancelled = false;
    api.integrationStatus().then(
      (status) => { if (!cancelled) setExtension(status.mode !== "browser"); },
      () => { if (!cancelled) setExtension(null); },
    );
    return () => {
      cancelled = true;
    };
  }, [revision]);

  const query = searchQuery ?? "";
  const tokens = searchTokens(query);
  const hasQuery = tokens.length > 0;
  const appContext: AppSearchContext = { builtinBrowser, extension };

  useEffect(() => {
    if (!hasQuery) {
      setFullText(null);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      api.searchProductIds(query).then(
        (ids) => { if (!cancelled) setFullText({ query, ids: new Set(ids) }); },
        (err: unknown) => { if (!cancelled) setFullText({ query, error: errorText(err) }); },
      );
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query, hasQuery, revision]);

  const fullIds = fullText && fullText.query === query && "ids" in fullText ? fullText.ids : undefined;
  const fullTextFailed = fullText !== null && fullText.query === query && "error" in fullText;

  const foundAvatars = useMemo(
    () => (avatars.status === "ok" && hasQuery ? filterAvatarsByQuery(avatars.value, query) : []),
    [avatars, hasQuery, query],
  );
  const foundAssets = useMemo(
    () => (assets.status === "ok" && hasQuery ? filterProductsByQuery(assets.value.items, query, fullIds) : []),
    [assets, hasQuery, query, fullIds],
  );
  const foundWorlds = useMemo(
    () => (worlds.status === "ok" && hasQuery ? filterProductsByQuery(worlds.value.items, query, fullIds) : []),
    [worlds, hasQuery, query, fullIds],
  );
  const foundApp = useMemo(() => (hasQuery ? filterAppEntries(APP_SEARCH_ENTRIES, query) : []), [hasQuery, query]);
  const appExpanded = expandedQuery === query;
  const shownApp = appExpanded ? foundApp : foundApp.slice(0, APP_RESULTS_SHOWN);
  const total = foundApp.length + foundAvatars.length + foundAssets.length + foundWorlds.length;

  const kinds = [
    { label: "アバター", load: avatars, count: avatars.status === "ok" ? avatars.value.length : 0 },
    { label: "アセット", load: assets, count: assets.status === "ok" ? assets.value.total : 0 },
    { label: "ワールド", load: worlds, count: worlds.status === "ok" ? worlds.value.total : 0 },
  ];
  const ready = kinds.filter((kind) => kind.load.status === "ok");
  const loading = kinds.filter((kind) => kind.load.status === "loading");
  const failed = kinds.flatMap((kind) => (kind.load.status === "error" ? [{ label: kind.label, message: kind.load.message }] : []));

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>検索</h1>
        </div>
      </div>
      <input
        className="search search-page-input"
        type="search"
        aria-label="ライブラリ・設定を検索"
        placeholder="名前・ショップ名・説明・設定項目で検索"
        value={query}
        autoFocus
        onChange={(event) => setSearchQuery(event.target.value)}
      />
      {failed.length > 0 && (
        <div className="panel search-load-error" role="alert">
          <p className="note-strong">
            {ready.length === 0 && loading.length === 0
              ? "ライブラリを読み込めませんでした。アバターと商品を検索するには、もう一度読み込んでください。画面と設定の項目は検索できます。"
              : `${failed.map((kind) => kind.label).join("・")}を読み込めませんでした。読み込めた種類は検索できます。`}
          </p>
          <ul className="meta">
            {failed.map((kind) => <li key={kind.label}>{kind.label}：{kind.message}</li>)}
          </ul>
          <button className="button secondary" type="button" onClick={() => setAttempt((value) => value + 1)}>
            もう一度読み込む
          </button>
        </div>
      )}
      {!hasQuery ? (
        ready.length === 0 ? (
          loading.length > 0 && <p className="meta" role="status">読み込み中…</p>
        ) : (
          <p className="meta">
            {ready.map((kind) => `${kind.label} ${kind.count} 件`).join("、")}から探せます。
            {loading.length > 0 && `${loading.map((kind) => kind.label).join("・")}は読み込み中です。`}
            画面や設定の項目も探せます。
          </p>
        )
      ) : total === 0 ? (
        // すべての種類が読めなかったときは、上の帯が理由と再読み込みを出す。
        (ready.length > 0 || loading.length > 0) && <div className="empty">
          {loading.length > 0
            ? `いま読み込めている範囲には一致するものがありません。${loading.map((kind) => kind.label).join("・")}は読み込み中です。`
            : "一致するものがありません。"}
        </div>
      ) : (
        <>
          <p className="meta" role="status">
            {total} 件見つかりました
            {loading.length > 0 && `（${loading.map((kind) => kind.label).join("・")}は読み込み中）`}
          </p>
          {foundApp.length > 0 && (
            <div className="search-group">
              <h2>画面と設定（{foundApp.length}）</h2>
              <ul className="app-search-list">
                {shownApp.map((entry) => (
                  <li key={entry.key}>
                    <AppEntryLink entry={entry} context={appContext} />
                  </li>
                ))}
              </ul>
              {!appExpanded && foundApp.length > shownApp.length && (
                <button className="button secondary app-search-more" type="button" onClick={() => setExpandedQuery(query)}>
                  さらに表示（残り {foundApp.length - shownApp.length} 件）
                </button>
              )}
            </div>
          )}
          {foundAvatars.length > 0 && (
            <div className="search-group">
              <h2>アバター（{foundAvatars.length}）</h2>
              <div className="cards">
                {foundAvatars.map((card) => (
                  <AvatarCardLink key={card.id} card={card} />
                ))}
              </div>
            </div>
          )}
          {foundAssets.length > 0 && (
            <div className="search-group">
              <h2>アセット（{foundAssets.length}）</h2>
              <div className="stack">
                {foundAssets.map((item) => (
                  <ItemCard key={item.boothId} item={item} />
                ))}
              </div>
            </div>
          )}
          {foundWorlds.length > 0 && (
            <div className="search-group">
              <h2>ワールド（{foundWorlds.length}）</h2>
              <div className="stack">
                {foundWorlds.map((item) => (
                  <ItemCard key={item.boothId} item={item} />
                ))}
              </div>
            </div>
          )}
        </>
      )}
      {hasQuery && fullTextFailed && ready.length > 0 && (
        <p className="meta">説明の全文を検索できなかったため、説明は先頭の 500 字だけで探しています。</p>
      )}
    </section>
  );
}

function AppEntryLink({ entry, context }: { entry: AppSearchEntry; context: AppSearchContext }) {
  const note = appEntryNote(entry, context);
  const { target, fallback } = appEntryTargets(entry, context);
  return (
    <a
      className="app-search-link"
      href={entry.href}
      onClick={(event) => {
        event.preventDefault();
        jumpToAppLocation(entry.href, target, fallback);
      }}
    >
      <span className="app-search-title">{entry.title}</span>
      <span className="app-search-path">{entry.path.join(" › ")}</span>
      {note && <span className="app-search-note">{note}</span>}
    </a>
  );
}
