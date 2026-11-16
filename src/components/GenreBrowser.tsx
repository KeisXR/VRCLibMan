import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ItemCard } from "../pages/AvatarDetailPage";
import { GENRE_FREE, GENRE_LABEL, GENRE_UNSET } from "../types";
import type { ItemView } from "../types";
import { useGenreBrowse } from "../genreBrowseContext";
import { filterItems } from "../genreBrowseUtils";
import { genreSectionId, groupGenreSections } from "../genreSections";
import { useRemoteProductProbe } from "../remoteProductProbe";
import type { RemoteProbeStatus } from "../remoteProductProbe";
import { LIST_STEP, chunked, chunksFor, useProgressiveCount } from "../progressiveList";
import { isPreviewMoveKey, nextPreviewId, previewKeysBlocked } from "../previewPane";
import { scrollToGenreSection } from "../useGenreSpy";
import { GenreChipBar } from "./GenreChipBar";
import { GenreIcon } from "./genreIcons";
import { useRightPanelPreview } from "./RightPanel";
import { useReplayRise } from "../useReplayRise";

const NOUN: Record<string, string> = { asset: "アセット", world: "ワールド" };

export { filterItems, genreHref, selectedGenre } from "../genreBrowseUtils";

/** What every card of the list shares. One object, so a group of cards redraws only when it changes. */
interface CardContext {
  probed: Map<string, ItemView>;
  statusFor: (boothId: string) => RemoteProbeStatus;
  retryProbe: (boothIds: string[]) => void;
  previewOpen: boolean;
  selectedId: string | null;
  focusId: string | null;
  tabStopId: string | null;
  select: (boothId: string) => void;
  setDraggingId: (boothId: string | null) => void;
}

/**
 * One group of cards (see progressiveList.ts). Groups already on screen are not drawn again
 * while the next ones are added. Renders no element of its own, so the list stays flat.
 */
const CardGroup = memo(function CardGroup({ items, context }: { items: ItemView[]; context: CardContext }) {
  const { probed, statusFor, retryProbe, previewOpen, selectedId, focusId, tabStopId, select, setDraggingId } = context;
  return (
    <>
      {items.map((item) => (
        <ItemCard
          key={item.boothId}
          item={probed.get(item.boothId) ?? item}
          movable
          onDragChange={setDraggingId}
          remoteProbeStatus={statusFor(item.boothId)}
          onRetryRemoteProbe={() => retryProbe([item.boothId])}
          selectable={previewOpen}
          selected={previewOpen && item.boothId === selectedId}
          focused={item.boothId === focusId}
          tabStop={previewOpen && item.boothId === tabStopId}
          onSelect={previewOpen ? () => select(item.boothId) : undefined}
        />
      ))}
    </>
  );
});

/**
 * The asset or world list. Every product is shown, under one heading per genre (the genres,
 * then 無料, then 未設定). The genre column and the chip bar jump between the headings.
 */
export function GenreBrowser({ title, lede }: { title: string; lede?: string }) {
  const {
    kind,
    focusId,
    data,
    dataKey,
    loading,
    error,
    genreBar,
    reload,
    setDraggingId,
    order,
    jump,
    resync,
  } = useGenreBrowse();
  const [filter, setFilter] = useState("");
  // Only the preview part of the panel: selecting a row or changing tabs does not draw the list again.
  const panel = useRightPanelPreview();
  const { publishPreview } = panel;
  // Rows are selectable while the right panel shows its preview tab.
  const previewOpen = panel.previewActive;
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const keyboardMove = useRef(false);
  const current = data;
  const filtered = useMemo(() => filterItems(current?.items ?? [], filter), [current, filter]);
  const sections = useMemo(
    () => groupGenreSections(filtered, (item) => current?.buckets?.[item.boothId] ?? GENRE_UNSET, order),
    [filtered, current, order],
  );
  // The list in the order it is drawn: heading by heading.
  const shown = useMemo(() => sections.flatMap((section) => section.items), [sections]);
  const shownIds = useMemo(() => shown.map((item) => item.boothId), [shown]);
  const { items: probedItems, statusFor, retryProbe } = useRemoteProductProbe(shown);
  const probedMap = useMemo(
    () => new Map(probedItems.map((item) => [item.boothId, item])),
    [probedItems],
  );
  const noun = NOUN[kind];
  const selectedItem = selectedId ? probedMap.get(selectedId) ?? shown.find((item) => item.boothId === selectedId) ?? null : null;
  const tabStopId = selectedId ?? shownIds[0] ?? null;
  // Another list or filter starts drawing over; a refresh of the same one keeps what is drawn.
  const { count: drawnCount, reveal } = useProgressiveCount(`${dataKey ?? ""}\u0000${filter}`, shown.length);
  const clearSelection = useCallback(() => setSelectedId(null), []);
  const publishedItem = previewOpen ? selectedItem : null;

  useEffect(() => {
    publishPreview({ item: publishedItem, clear: clearSelection });
  }, [publishPreview, publishedItem, clearSelection]);

  useEffect(() => () => publishPreview(null), [publishPreview]);

  useEffect(() => {
    if (selectedId && !shownIds.includes(selectedId)) setSelectedId(null);
  }, [shownIds, selectedId]);

  // More headings drawn can change which one the scroll position has reached.
  useEffect(() => resync(), [drawnCount, sections, resync]);

  // A genre picked in the column or the chip bar: draw up to its heading, then scroll to it.
  const jumped = useRef(0);
  useEffect(() => {
    if (!jump || jumped.current === jump.nonce || !current) return;
    const section = sections.find((row) => row.key === jump.key);
    if (!section) {
      // Nothing under that genre (or hidden by the filter): there is no heading to go to.
      jumped.current = jump.nonce;
      return;
    }
    if (section.start >= drawnCount) {
      reveal(section.start + Math.min(section.items.length, LIST_STEP) - 1);
      return;
    }
    jumped.current = jump.nonce;
    const { key, smooth } = jump;
    // After the heading is laid out (a timer where there is no animation frame, as in tests).
    if (typeof window.requestAnimationFrame === "function") window.requestAnimationFrame(() => scrollToGenreSection(key, smooth));
    else window.setTimeout(() => scrollToGenreSection(key, smooth), 0);
  }, [jump, sections, drawnCount, reveal, current]);

  // 購入履歴などから開いた商品へ、一覧を読み終えたら一度だけ送る。プレビューが開いていれば選ぶ。
  const focusedOnce = useRef<string | null>(null);
  useEffect(() => {
    if (!focusId) {
      focusedOnce.current = null;
      return;
    }
    if (loading || focusedOnce.current === focusId) return;
    const index = shownIds.indexOf(focusId);
    if (index < 0) return;
    if (index >= drawnCount) {
      reveal(index);
      return;
    }
    focusedOnce.current = focusId;
    if (previewOpen) setSelectedId(focusId);
    window.requestAnimationFrame(() => {
      document.getElementById(`product-${focusId}`)?.scrollIntoView({ block: "center" });
    });
  }, [focusId, shownIds, previewOpen, drawnCount, reveal, loading]);
  const focusMissing = Boolean(
    focusId && current && !loading && !current.items.some((item) => item.boothId === focusId),
  );

  useEffect(() => {
    if (!keyboardMove.current || !selectedId) return;
    const index = shownIds.indexOf(selectedId);
    if (index >= drawnCount) {
      reveal(index);
      return;
    }
    keyboardMove.current = false;
    const node = document.getElementById(`product-${selectedId}`);
    if (node instanceof HTMLElement) {
      node.focus();
      node.scrollIntoView({ block: "nearest" });
    }
  }, [selectedId, shownIds, drawnCount, reveal]);

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

  const listRef = useRef<HTMLDivElement>(null);
  // 新しい一覧が届いて差し替えたときに動かす。
  useReplayRise(listRef, dataKey);
  const groups = useMemo(() => new Map(sections.map((section) => [section.key, chunked(section.items)])), [sections]);
  const cardContext = useMemo<CardContext>(
    () => ({
      probed: probedMap,
      statusFor,
      retryProbe,
      previewOpen,
      selectedId,
      focusId,
      tabStopId,
      select: setSelectedId,
      setDraggingId,
    }),
    [probedMap, statusFor, retryProbe, previewOpen, selectedId, focusId, tabStopId, setDraggingId],
  );
  const list = (
    <div className="genre-list" ref={listRef} aria-busy={loading || undefined}>
      {sections.map((section) => {
        // Headings below what is drawn come with their cards (progressiveList.ts).
        if (section.start >= drawnCount) return null;
        const drawn = Math.min(section.items.length, drawnCount - section.start);
        const label = GENRE_LABEL[section.key] ?? section.key;
        return (
          <section key={section.key} className="genre-section" id={genreSectionId(section.key)} aria-label={label}>
            <h2 className="genre-section-head">
              <GenreIcon name={section.key} />
              <span className="genre-section-label">{label}</span>
              <span className="genre-section-count">{section.items.length}</span>
            </h2>
            {section.key === GENRE_UNSET && (
              <p className="note genre-notice">
                ジャンルが決まっていない{noun}の商品です。確認画面や再分類でジャンルが付くと、各ジャンルに移ります。
              </p>
            )}
            {section.key === GENRE_FREE && (
              <p className="note genre-notice">
                ダウンロードされていない無料の{noun}です。ファイルが手元に来ると、各ジャンルに移ります。手動で移動した商品は、移動先のジャンルに残ります。
              </p>
            )}
            <div className="stack">
              {(groups.get(section.key) ?? []).slice(0, chunksFor(drawn)).map((items, index) => (
                <CardGroup key={index} items={items} context={cardContext} />
              ))}
            </div>
          </section>
        );
      })}
    </div>
  );

  return (
    <section className="genre-browser">
      {genreBar && <GenreChipBar />}
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
            aria-controls="right-panel"
            onClick={panel.togglePreview}
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
      {focusMissing && <p className="note genre-notice">開こうとした商品は、この一覧にありません。分類が変わったか、除外した商品です。</p>}
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
      ) : shown.length === 0 ? (
        <p className="empty">条件に合う商品はありません。</p>
      ) : (
        <>
          <p className="meta genre-result" role="status">
            {filter.trim() ? `${shown.length} 件（全 ${current.items.length} 件）` : `${current.items.length} 件`}
          </p>
          {list}
        </>
      )}
    </section>
  );
}

