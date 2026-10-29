import { useEffect, useMemo, useRef, useState } from "react";
import { ItemCard } from "../pages/AvatarDetailPage";
import { GENRE_FREE, GENRE_LABEL, GENRE_UNSET, genresForKind } from "../types";
import type { GenreKind } from "../types";
import { useGenreBrowse } from "../genreBrowseContext";
import { filterItems, genreHref, selectedGenre } from "../genreBrowseUtils";
import { useRemoteProductProbe } from "../remoteProductProbe";
import { isPreviewMoveKey, nextPreviewId, previewKeysBlocked } from "../previewPane";
import { GenreTarget } from "./GenreColumn";
import { PreviewOverlay, PreviewPane, useLargePreview, usePreviewOpen, usePreviewPaneWidth } from "./PreviewPane";
import { ResizeSeparator } from "./ResizeSeparator";

const NOUN: Record<string, string> = { asset: "アセット", world: "ワールド" };

export { filterItems, genreHref, selectedGenre } from "../genreBrowseUtils";

export function GenreBrowser({ title, lede }: { title: string; lede?: string }) {
  const { kind, basePath, routeGenre, selected, data, error, narrowLayout, reload, draggingId, setDraggingId, moveProduct } = useGenreBrowse();
  const [filter, setFilter] = useState("");
  const [previewOpen, togglePreview] = usePreviewOpen();
  const [windowWidth, setWindowWidth] = useState(() => (typeof window === "undefined" ? 1280 : window.innerWidth));
  const [splitWidth, setSplitWidth] = useState<number | null>(null);
  const splitRef = useRef<HTMLDivElement>(null);
  const previewWidth = usePreviewPaneWidth(windowWidth, splitWidth);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const keyboardMove = useRef(false);
  const current = data;
  const shown = useMemo(() => filterItems(current?.items ?? [], filter), [current, filter]);
  const shownIds = useMemo(() => shown.map((item) => item.boothId), [shown]);
  const { items: probedItems, statusFor, retryProbe } = useRemoteProductProbe(shown);
  const probedMap = useMemo(
    () => new Map(probedItems.map((item) => [item.boothId, item])),
    [probedItems],
  );
  const noun = NOUN[kind];
  const selectedLabel = selected ? GENRE_LABEL[selected] ?? selected : "すべて";
  const unknownGenre = routeGenre != null && routeGenre !== "" && selectedGenre(kind, routeGenre) === null;
  const selectedItem = selectedId ? probedMap.get(selectedId) ?? shown.find((item) => item.boothId === selectedId) ?? null : null;
  const preview = useLargePreview(previewOpen ? selectedItem : null, previewOpen && selectedItem != null);
  const tabStopId = selectedId ?? shownIds[0] ?? null;
  const closePreview = () => setSelectedId(null);

  useEffect(() => {
    if (selectedId && !shownIds.includes(selectedId)) setSelectedId(null);
  }, [shownIds, selectedId]);

  useEffect(() => {
    const onResize = () => setWindowWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    const node = splitRef.current;
    if (!node) return;
    const measure = () => setSplitWidth(node.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [previewOpen, narrowLayout, shown.length]);

  useEffect(() => {
    if (!keyboardMove.current || !selectedId) return;
    keyboardMove.current = false;
    const node = document.getElementById(`product-${selectedId}`);
    if (node instanceof HTMLElement) {
      node.focus();
      node.scrollIntoView({ block: "nearest" });
    }
  }, [selectedId]);

  useEffect(() => {
    if (!previewOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.altKey || event.ctrlKey || event.metaKey || event.defaultPrevented) return;
      if (!isPreviewMoveKey(event.key)) return;
      const target = event.target;
      if (!(target instanceof Element)) return;
      const editable = target instanceof HTMLElement && target.isContentEditable;
      const inMenu = Boolean(target.closest(".product-actions-panel, [role='menu'], [role='separator']"));
      if (previewKeysBlocked(target.tagName, editable, inMenu)) return;
      if (shownIds.length === 0) return;
      event.preventDefault();
      keyboardMove.current = true;
      setSelectedId((currentId) => nextPreviewId(shownIds, currentId, event.key));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [previewOpen, shownIds]);

  const list = (
    <div className="stack">
      {shown.map((item) => {
        const resolved = probedMap.get(item.boothId) ?? item;
        const isSelected = previewOpen && item.boothId === selectedId;
        return (
          <ItemCard
            key={item.boothId}
            item={resolved}
            movable
            onDragChange={setDraggingId}
            remoteProbeStatus={statusFor(item.boothId)}
            onRetryRemoteProbe={() => retryProbe([item.boothId])}
            selectable={previewOpen}
            selected={isSelected}
            tabStop={previewOpen && item.boothId === tabStopId}
            onSelect={previewOpen ? () => setSelectedId(item.boothId) : undefined}
          />
        );
      })}
    </div>
  );

  const pane = (
    <PreviewPane
      item={selectedItem}
      largePath={preview.largePath}
      phase={preview.phase}
      onReveal={preview.reveal}
      onClose={narrowLayout ? closePreview : undefined}
    />
  );

  return (
    <section className="genre-browser">
      {narrowLayout && (
        <details className="genre-mobile-picker">
          <summary>ジャンル：{selectedLabel}</summary>
          <nav className="genre-mobile-nav" aria-label="ジャンル（狭い画面）">
            <GenreTarget href={genreHref(basePath, null)} label="すべて" active={selected === null} onDropId={() => undefined} />
            {genresForKind(kind).map((value) => (
              <GenreTarget
                key={value}
                href={genreHref(basePath, value)}
                label={GENRE_LABEL[value] ?? value}
                count={data?.counts[value] ?? 0}
                showCount={false}
                active={selected === value}
                dropGenre={value}
                onDropId={(boothId) => {
                  setDraggingId(null);
                  void moveProduct(boothId, kind, value);
                }}
              />
            ))}
            <GenreTarget
              href={genreHref(basePath, GENRE_FREE)}
              label={GENRE_LABEL[GENRE_FREE]}
              count={data?.counts[GENRE_FREE] ?? 0}
              showCount={false}
              active={selected === GENRE_FREE}
              onDropId={() => undefined}
            />
            {(data?.counts[GENRE_UNSET] ?? 0) > 0 || selected === GENRE_UNSET ? (
              <GenreTarget
                href={genreHref(basePath, GENRE_UNSET)}
                label={GENRE_LABEL[GENRE_UNSET]}
                active={selected === GENRE_UNSET}
                onDropId={() => undefined}
              />
            ) : null}
            {draggingId && (
              <div className="genre-rail-other">
                <p className="genre-rail-kicker">{kind === "world" ? "アセット" : "ワールド"}</p>
                {genresForKind(kind === "world" ? "asset" : "world").map((value) => {
                  const across: GenreKind = kind === "world" ? "asset" : "world";
                  return (
                    <GenreTarget
                      key={value}
                      label={GENRE_LABEL[value] ?? value}
                      dropGenre={value}
                      onDropId={(boothId) => {
                        setDraggingId(null);
                        void moveProduct(boothId, across, value);
                      }}
                    />
                  );
                })}
              </div>
            )}
          </nav>
        </details>
      )}
      <div className="page-head genre-list-head">
        <div>
          <h1>{title}</h1>
          {lede && <p>{lede}</p>}
        </div>
        <div className="toolbar">
          <button
            className="button"
            type="button"
            aria-pressed={previewOpen}
            aria-controls="asset-preview"
            onClick={togglePreview}
          >
            プレビュー
          </button>
        </div>
      </div>
      <input
        className="search genre-list-search"
        type="search"
        aria-label={`${title}を絞り込む`}
        placeholder="商品名・ショップ名で絞り込み"
        value={filter}
        onChange={(event) => setFilter(event.target.value)}
      />
      {unknownGenre && <p className="note genre-notice">指定されたジャンルが見つからないため、すべてを表示しています。</p>}
      {selected === GENRE_UNSET && (
        <p className="note genre-notice">
          ジャンルが決まっていない{noun}の商品です。確認画面や再分類でジャンルが付くと、各ジャンルに移ります。
        </p>
      )}
      {selected === GENRE_FREE && (
        <p className="note genre-notice">
          ダウンロードされていない無料の{noun}です。ファイルが手元に来ると、各ジャンルに移ります。手動で移動した商品は、移動先のジャンルに残ります。
        </p>
      )}
      {error ? (
        <div className="empty" role="alert">
          <p>{noun}の一覧を読み込めませんでした。{error}</p>
          <button type="button" onClick={reload}>
            もう一度読み込む
          </button>
        </div>
      ) : current === null ? (
        <p className="meta">読み込み中</p>
      ) : current.total === 0 ? (
        <div className="empty">
          <p>{noun}の商品はまだありません。インポートして分類すると、ジャンル別に表示されます。</p>
          <a href="#/import">インポートへ</a>
        </div>
      ) : current.items.length === 0 ? (
        <p className="empty">「{selectedLabel}」の商品はまだありません。</p>
      ) : shown.length === 0 ? (
        <p className="empty">条件に合う商品はありません。</p>
      ) : previewOpen && !narrowLayout ? (
        <div
          ref={splitRef}
          className="split with-side"
          style={{ ["--preview-width" as string]: `${previewWidth.width}px` }}
        >
          <div>
            <p className="meta genre-result" role="status">
              {selectedLabel} {shown.length} 件
            </p>
            {list}
          </div>
          <ResizeSeparator
            label="プレビューの幅"
            value={previewWidth.width}
            min={previewWidth.min}
            max={previewWidth.max}
            grows="left"
            onChange={previewWidth.resize}
            onCommit={previewWidth.commit}
            onReset={previewWidth.reset}
          />
          <div className="preview-column">{pane}</div>
        </div>
      ) : (
        <>
          <p className="meta genre-result" role="status">
            {selectedLabel} {shown.length} 件
          </p>
          {list}
          {previewOpen && narrowLayout && selectedItem && (
            <PreviewOverlay onClose={closePreview}>{pane}</PreviewOverlay>
          )}
        </>
      )}
    </section>
  );
}
