import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, openBoothLink, openLocal, revealInFolder } from "../api";
import { useShell } from "../App";
import { downloadDetail, downloadKey, isDownloadActive, type DownloadEntry } from "../downloads";
import { useDownloads } from "../useDownloads";
import { filePresence, formatBytes } from "../media";
import { CATEGORY_LABEL } from "../types";
import type { AvatarDetail, BoothFileStatus, DownloadView, ItemView, Knowledge, ProductOption } from "../types";
import { FileMark, Thumb, errorText } from "../ui";
import { AvatarHero } from "../components/AvatarHero";
import { AvatarHeroMenu } from "../components/AvatarHeroMenu";
import {
  avatarHeroSource,
  normalizeAvatarHero,
  readAssetsOpenPref,
  writeAssetsOpenPref,
  type AvatarHeroSize,
} from "../avatarHero";
import { ExcludeProductButton } from "../components/ProductExclusions";
import { UnityOpenButton } from "../components/UnityOpenDialog";
import { isUnityOpenableName } from "../unityOpen";
import { copyWithConfirm } from "../remoteCopy";
import { FREE_FILE_CHECK_BUSY_LABEL, FREE_FILE_CHECK_LABEL, freeFilesFoundMessage, offerFreeFileCheck } from "../freeFiles";
import {
  type RemoteProbeStatus,
  placeLabelForFile,
  useRemoteProductProbe,
} from "../remoteProductProbe";

const prefersReducedMotion = () =>
  typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

function totalDownloadBytes(files: ReadonlyArray<{ sizeBytes?: number | null; size?: number | null }>): number {
  return files.reduce((sum, file) => {
    const n = file.sizeBytes ?? file.size ?? 0;
    return sum + (n > 0 ? n : 0);
  }, 0);
}

function formatAssetSummary(count: number, bytes: number): string {
  if (bytes > 0) return `${count} ファイル・${formatBytes(bytes)}`;
  return `${count} ファイル`;
}

export function AvatarDetailPage({ id, fileBoothId }: { id: string; fileBoothId?: string }) {
  const { revision, refresh, notify, fail } = useShell();
  const [detail, setDetail] = useState<AvatarDetail | null>(null);
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [heroSize, setHeroSize] = useState<AvatarHeroSize>("small");
  const [intoId, setIntoId] = useState("");

  useEffect(() => {
    let cancelled = false;
    api.avatar(id).then(
      (value) => {
        if (!cancelled) setDetail(value);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    api.knowledge().then(
      (value) => {
        if (!cancelled) setKnowledge(value);
      },
      () => undefined,
    );
    api.settings().then(
      (value) => {
        if (!cancelled) setHeroSize(normalizeAvatarHero(value.avatarHero));
      },
      () => undefined,
    );
    return () => {
      cancelled = true;
    };
  }, [id, revision, fail]);

  useEffect(() => {
    if (!detail || !fileBoothId) return;
    document.getElementById(`files-${fileBoothId}`)?.scrollIntoView({ block: "start" });
  }, [detail, fileBoothId]);

  const catalogItems = useMemo(() => {
    if (!detail) return [];
    return [
      ...detail.ownedProducts,
      ...detail.sections.flatMap((section) => section.items),
      ...detail.genericItems,
      ...detail.unsupportedOutfits,
    ];
  }, [detail]);
  const { items: probedCatalog, statusFor, retryProbe } = useRemoteProductProbe(catalogItems);
  const probedMap = useMemo(
    () => new Map(probedCatalog.map((item) => [item.boothId, item])),
    [probedCatalog],
  );
  const itemCardProps = useCallback(
    (item: ItemView) => ({
      item: probedMap.get(item.boothId) ?? item,
      remoteProbeStatus: statusFor(item.boothId),
      onRetryRemoteProbe: () => retryProbe([item.boothId]),
    }),
    [probedMap, statusFor, retryProbe],
  );

  async function merge() {
    if (!detail || !intoId) return;
    try {
      await api.mergeAvatars(detail.id, intoId);
      notify("アバターを統合し、対応づけをやり直しました");
      window.location.hash = `#/avatar/${encodeURIComponent(intoId)}`;
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  const linkedIds = useMemo(() => {
    const ids = new Set<string>();
    for (const section of detail?.sections ?? []) {
      for (const item of section.items) {
        if (!item.viaBaseBody) ids.add(item.boothId);
      }
    }
    return ids;
  }, [detail]);

  if (!detail) return <p className="meta">読み込み中</p>;
  const targets = (knowledge?.avatars ?? []).filter((avatar) => avatar.id !== detail.id);

  const subtitle = `${detail.owned ? "所持アバター" : "所持未登録"}${detail.baseBodyName ? ` ・ ${detail.baseBodyName}` : ""}`;
  const addAction = <AddAvatarItem avatarId={detail.id} linkedIds={linkedIds} />;
  const primaryOwned = detail.ownedProducts[0] ?? null;
  const backLink = <a className="button avatar-back-link" href="#/">アバター一覧へ</a>;
  const heroImage = avatarHeroSource(detail);
  const itemCount = detail.sections.reduce((sum, section) => sum + section.items.length, 0) + detail.genericItems.length;
  // ヒーローと本体ファイルの下にある対応アイテムへ、1回で移る。ハッシュは画面の切り替えに使うので、スクロールだけにする。
  const jumpToItems = itemCount > 0 && (
    <button
      className="button secondary avatar-jump"
      type="button"
      onClick={() => document.getElementById("avatar-items")?.scrollIntoView?.({ block: "start", behavior: prefersReducedMotion() ? "auto" : "smooth" })}
    >
      対応アイテムへ（{itemCount}）
    </button>
  );

  return (
    <section className={heroSize === "off" ? undefined : "avatar-detail"}>
      {heroSize === "off" ? (
        <>
          <p>{backLink}</p>
          <div className="page-head">
            <div>
              <h1>{detail.displayName}</h1>
              <p>{subtitle}</p>
            </div>
            {addAction}
          </div>
        </>
      ) : (
        <>
          <AvatarHero
            size={heroSize}
            path={heroImage.path}
            url={heroImage.url}
            version={heroImage.version}
            name={detail.displayName}
            kicker="アバター"
            subtitle={subtitle}
            nav={backLink}
            menu={
              <AvatarHeroMenu
                avatarId={detail.id}
                avatarName={detail.displayName}
                hasCustomImage={detail.heroImagePath != null}
              />
            }
          />
          <div className="avatar-hero-toolbar">{jumpToItems}{addAction}</div>
          {primaryOwned ? <AvatarAssetsPanel item={primaryOwned} itemCardProps={itemCardProps} /> : null}
        </>
      )}
      {(detail.aliases.length > 0 || detail.baseBodyNote) && (
        <details className="generic">
          <summary>アバターの情報</summary>
          {detail.aliases.length > 0 && <p className="meta">別名: {detail.aliases.join(" / ")}</p>}
          {detail.baseBodyNote && <p className="meta">{detail.baseBodyNote}</p>}
        </details>
      )}
      <div className="split">
        <div>
          {detail.ownedProducts.length > 0 && (
            <div className="stack">
              {detail.ownedProducts.map((item) => (
                <ItemCard
                  key={item.boothId}
                  {...itemCardProps(item)}
                  hideFiles={heroSize !== "off" && primaryOwned?.boothId === item.boothId}
                />
              ))}
            </div>
          )}
          {detail.sections.map((section) => (
            <div className="section" key={section.category} id={section === detail.sections[0] ? "avatar-items" : undefined}>
              <h2>{CATEGORY_LABEL[section.category] ?? section.category}</h2>
              <div className="stack">
                {section.items.map((item) => (
                  <ItemCard key={item.boothId} {...itemCardProps(item)} />
                ))}
              </div>
            </div>
          ))}
          {detail.genericItems.length > 0 && (
            <details className="generic" id={detail.sections.length === 0 ? "avatar-items" : undefined}>
              <summary>汎用 {detail.genericItems.length} 件</summary>
              <div className="stack">
                {detail.genericItems.map((item) => (
                  <ItemCard key={item.boothId} {...itemCardProps(item)} />
                ))}
              </div>
            </details>
          )}
          {detail.sections.length === 0 && detail.genericItems.length === 0 && (
            <p className="empty">このアバターに結び付いた衣装・髪・ギミックはまだありません。</p>
          )}
          {detail.unsupportedOutfits.length > 0 && (
            <UnsupportedOutfits key={detail.id} items={detail.unsupportedOutfits} itemCardProps={itemCardProps} />
          )}
        </div>
        {!detail.isBuiltin && (
          <details className="generic">
            <summary>アバターを編集</summary>
            <div className="panel" style={{ padding: 14 }}>
              <h2 style={{ marginTop: 0 }}>既存のアバターへ統合</h2>
              <p className="meta">
                商品の対応先と別名を統合先へまとめ、この項目を削除します。
              </p>
              <label className="field">
                <span>統合先</span>
                <select value={intoId} onChange={(event) => setIntoId(event.target.value)}>
                  <option value="">選ぶ</option>
                  {targets.map((avatar) => (
                    <option key={avatar.id} value={avatar.id}>
                      {avatar.displayName}
                    </option>
                  ))}
                </select>
              </label>
              <button className="button primary" type="button" disabled={!intoId} onClick={() => void merge()}>
                統合する
              </button>
            </div>
          </details>
        )}
      </div>
    </section>
  );
}


function AvatarAssetsPanel({
  item,
  itemCardProps,
}: {
  item: ItemView;
  itemCardProps: (item: ItemView) => {
    item: ItemView;
    remoteProbeStatus: RemoteProbeStatus;
    onRetryRemoteProbe: () => void;
  };
}) {
  const { builtinBrowser, refresh, notify, fail } = useShell();
  const probed = itemCardProps(item);
  const view = probed.item;
  const [open, setOpen] = useState(() =>
    readAssetsOpenPref(typeof localStorage === "undefined" ? null : localStorage),
  );
  const [busy, setBusy] = useState("");
  const fileCount = view.downloads.length + view.localFiles.length;
  const bytes = totalDownloadBytes(view.downloads) + totalDownloadBytes(view.localFiles);
  if (fileCount === 0 && !offerFreeFileCheck(view, builtinBrowser)) return null;

  async function pull(file: DownloadView) {
    setBusy(file.filename);
    try {
      notify(await copyWithConfirm("pull", file.filename, (overwrite) => api.pullRemoteFile(view.boothId, file.variation, file.filename, overwrite)));
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy("");
    }
  }

  async function push(file: DownloadView) {
    setBusy(file.filename);
    try {
      notify(await copyWithConfirm("push", file.filename, (overwrite) => api.pushRemoteFile(view.boothId, file.variation, file.filename, overwrite)));
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy("");
    }
  }

  return (
    <details
      className="avatar-assets"
      open={open}
      onToggle={(event) => {
        const next = event.currentTarget.open;
        setOpen(next);
        writeAssetsOpenPref(next, typeof localStorage === "undefined" ? null : localStorage);
      }}
    >
      <summary>
        <span className="avatar-assets-chevron" aria-hidden="true" />
        <span>アセットファイル</span>
        <span className="avatar-assets-summary-meta">{formatAssetSummary(fileCount, bytes)}</span>
      </summary>
      <div className="avatar-assets-body">
        {offerFreeFileCheck(view, builtinBrowser) ? (
          <FreeFileCheckButton boothId={view.boothId} />
        ) : null}
        {view.downloads.length > 0 ? (
          <DownloadFiles
            item={view}
            remoteProbeStatus={probed.remoteProbeStatus}
            busy={busy}
            pull={(file) => void pull(file)}
            push={(file) => void push(file)}
          />
        ) : null}
        {view.localFileCount > 0 ? (
          <ul className="file-list">
            {view.localFiles.map((file) => (
              <li key={file.path}>
                <button type="button" onClick={() => void openLocal(file.path).catch((err: unknown) => fail(errorText(err)))}>
                  {file.isDir ? "フォルダ" : "ファイル"} {file.filename}
                </button>
                <span className="row-actions">
                  {!file.isDir && isUnityOpenableName(file.filename) ? (
                    <UnityOpenButton target={{ path: file.path, boothId: view.boothId }} notify={notify} fail={fail} />
                  ) : null}
                  <span>{formatBytes(file.size)}</span>
                </span>
              </li>
            ))}
            {view.localFileCount > view.localFiles.length ? (
              <li>ほか {view.localFileCount - view.localFiles.length} 件</li>
            ) : null}
          </ul>
        ) : null}
      </div>
    </details>
  );
}

function FreeFileCheckButton({ boothId }: { boothId: string }) {
  const { refresh, notify, fail } = useShell();
  const [checkingFree, setCheckingFree] = useState(false);
  async function runFreeFileCheck() {
    if (checkingFree) return;
    setCheckingFree(true);
    try {
      const report = await api.fetchFreeFiles(boothId);
      notify(freeFilesFoundMessage(report.files));
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setCheckingFree(false);
    }
  }
  return (
    <div className="free-file-check">
      <button className="button" type="button" disabled={checkingFree} onClick={() => void runFreeFileCheck()}>
        {checkingFree ? FREE_FILE_CHECK_BUSY_LABEL : FREE_FILE_CHECK_LABEL}
      </button>
    </div>
  );
}

function AddAvatarItem({ avatarId, linkedIds }: { avatarId: string; linkedIds: ReadonlySet<string> }) {
  const { refresh, notify, fail } = useShell();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [options, setOptions] = useState<ProductOption[]>([]);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    api.productOptions().then(
      (rows) => {
        if (!cancelled) setOptions(rows);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [open, fail]);

  useLayoutEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return options
      .filter((option) => option.category !== "avatar" && option.category !== "world")
      .filter((option) => !linkedIds.has(option.boothId))
      .filter((option) => {
        if (!needle) return true;
        const haystack = `${option.title} ${option.shop} ${option.boothId}`.toLowerCase();
        return haystack.includes(needle);
      })
      .slice(0, 40);
  }, [options, linkedIds, query]);

  async function add(boothId: string) {
    setBusy(true);
    try {
      await api.attachProductToAvatar(avatarId, boothId);
      setOpen(false);
      setQuery("");
      notify("アイテムを追加しました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="avatar-add">
      <button
        className="button"
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
      >
        アイテムを追加
      </button>
      {open && (
        <div className="avatar-add-panel">
          <input
            ref={inputRef}
            className="search"
            value={query}
            placeholder="商品を検索"
            onChange={(event) => setQuery(event.target.value)}
          />
          <div className="picker">
            {visible.length === 0 && <p className="meta">追加できる商品がありません。</p>}
            {visible.map((option) => (
              <button
                key={option.boothId}
                className="button"
                type="button"
                disabled={busy}
                onClick={() => void add(option.boothId)}
              >
                {option.title}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function UnsupportedOutfits({
  items,
  itemCardProps,
}: {
  items: ItemView[];
  itemCardProps: (item: ItemView) => {
    item: ItemView;
    remoteProbeStatus: RemoteProbeStatus;
    onRetryRemoteProbe: () => void;
  };
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState(30);
  const needle = useDeferredValue(query.trim().toLocaleLowerCase("ja"));
  const matches = useMemo(
    () =>
      items.filter((item) =>
        `${item.title} ${item.shop} ${item.boothId}`
          .toLocaleLowerCase("ja")
          .includes(needle),
      ),
    [items, needle],
  );

  return (
    <details
      className="generic unsupported-outfits"
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>ほかの衣装 {items.length} 件</summary>
      {open && (
        <>
          <p className="meta">
            このアバターや共通素体への対応が登録されていない衣装です。対応の有無は商品ページで確認してください。もちふぃった～などで調整する衣装の候補としても使えます。
          </p>
          <input
            className="search"
            aria-label="ほかの衣装を検索"
            placeholder="商品名・ショップ名・商品IDで検索"
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setLimit(30);
            }}
          />
          <p className="meta">
            {matches.length} 件中 {Math.min(limit, matches.length)} 件を表示
          </p>
          {matches.length === 0 && (
            <p className="empty">該当する衣装はありません。</p>
          )}
          <div className="stack">
            {matches.slice(0, limit).map((item) => (
              <ItemCard key={item.boothId} {...itemCardProps(item)} />
            ))}
          </div>
          {matches.length > limit && (
            <button
              className="button"
              type="button"
              onClick={() => setLimit((value) => value + 30)}
            >
              さらに表示
            </button>
          )}
        </>
      )}
    </details>
  );
}

export function ItemCard({
  item,
  movable = false,
  onDragChange,
  remoteProbeStatus = "ok",
  onRetryRemoteProbe,
  selectable = false,
  selected = false,
  tabStop = false,
  onSelect,
  hideFiles = false,
  focused = false,
}: {
  item: ItemView;
  movable?: boolean;
  onDragChange?: (boothId: string | null) => void;
  remoteProbeStatus?: RemoteProbeStatus;
  onRetryRemoteProbe?: () => void;
  selectable?: boolean;
  selected?: boolean;
  tabStop?: boolean;
  onSelect?: () => void;
  /** ヒーロー直下のアセット一覧へ移したとき、カード内のファイル行を隠す */
  hideFiles?: boolean;
  /** ほかの画面（購入履歴など）から開いた商品。枠で示す。 */
  focused?: boolean;
}) {
  const { builtinBrowser, refresh, notify, fail } = useShell();
  const [busy, setBusy] = useState("");
  const [checkingFree, setCheckingFree] = useState(false);

  async function checkFreeFiles() {
    if (checkingFree) return;
    setCheckingFree(true);
    try {
      const report = await api.fetchFreeFiles(item.boothId);
      notify(freeFilesFoundMessage(report.files));
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setCheckingFree(false);
    }
  }

  async function pull(file: DownloadView) {
    setBusy(file.filename);
    try {
      notify(await copyWithConfirm("pull", file.filename, (overwrite) => api.pullRemoteFile(item.boothId, file.variation, file.filename, overwrite)));
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy("");
    }
  }

  async function push(file: DownloadView) {
    setBusy(file.filename);
    try {
      notify(await copyWithConfirm("push", file.filename, (overwrite) => api.pushRemoteFile(item.boothId, file.variation, file.filename, overwrite)));
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy("");
    }
  }

  return (
    <article
      id={selectable || focused ? `product-${item.boothId}` : undefined}
      className={`${item.unpublished ? "item item-unpublished" : "item"}${selected ? " selected" : ""}${focused ? " focused" : ""}`}
      aria-current={selected ? "true" : undefined}
      tabIndex={selectable ? (tabStop ? 0 : -1) : undefined}
      draggable={movable}
      onClick={() => onSelect?.()}
      onKeyDown={(event) => {
        if (!onSelect) return;
        if (event.key !== " " && event.key !== "Spacebar") return;
        const target = event.target;
        if (!(target instanceof Element)) return;
        if (target.closest("button, a, input, textarea, select, summary, label")) return;
        event.preventDefault();
        onSelect();
      }}
      onDragStart={(event) => {
        if (!movable) return;
        const target = event.target as HTMLElement | null;
        if (target?.closest("button, a, input, summary, label")) {
          event.preventDefault();
          return;
        }
        event.dataTransfer.effectAllowed = "move";
        event.dataTransfer.setData("text/plain", item.boothId);
        event.currentTarget.classList.add("dragging");
        onDragChange?.(item.boothId);
      }}
      onDragEnd={(event) => {
        event.currentTarget.classList.remove("dragging");
        onDragChange?.(null);
      }}
    >
      <Thumb path={item.thumbnailPath} url={item.thumbnailUrl} name={item.title} small />
      <div>
        <h3>
          {item.title}
          {item.classificationPinned && <span className="badge">手動</span>}
          {item.unpublished && <span className="badge">非公開</span>}
          {item.isFree && <span className="badge">無料</span>}
          {item.isAdult && <span className="badge adult">R-18</span>}
          <FileMark
            presence={filePresence(item.downloads, item.localFileCount)}
            onClick={() => document.getElementById(`files-${item.boothId}`)?.scrollIntoView({ block: "nearest" })}
          />
        </h3>
        <div className="meta">
          {item.shop}
          {item.viaBaseBody ? " ・ 同じ素体" : ""}
          {item.isGeneric ? " ・ 汎用" : ""}
        </div>
        {item.isAdult && item.descriptionSnippet && (
          <details className="adult-note">
            <summary>説明を表示</summary>
            <p>{item.descriptionSnippet}</p>
          </details>
        )}
        {item.variations.length > 0 && (
          <div className="variations">
            {item.variations.map((variation) => (
              <span key={variation.name} className={variation.owned ? "variation-chip" : "variation-chip off"}>
                {variation.owned ? variation.name : `${variation.name}（未購入）`}
              </span>
            ))}
          </div>
        )}
        <div id={`files-${item.boothId}`}>
          {!hideFiles && item.downloads.length === 0 && item.localFileCount === 0 && (
            <p className="meta">ファイル情報はまだありません。再インポートするか、フォルダを調べてください</p>
          )}
          {!hideFiles && offerFreeFileCheck(item, builtinBrowser) && (
            <div className="free-file-check">
              <button className="button" type="button" disabled={checkingFree} onClick={() => void checkFreeFiles()}>
                {checkingFree ? FREE_FILE_CHECK_BUSY_LABEL : FREE_FILE_CHECK_LABEL}
              </button>
            </div>
          )}
          {!hideFiles && item.downloads.length > 0 &&
            item.remoteConfigured &&
            ((item.remoteChecked && !item.remoteReachable) || remoteProbeStatus === "failed") && (
              <p className="meta">リモートのフォルダを確認できません。接続してからもう一度開いてください。</p>
            )}
          {!hideFiles && remoteProbeStatus === "failed" && onRetryRemoteProbe && (
            <button className="button" type="button" onClick={onRetryRemoteProbe}>
              もう一度確認
            </button>
          )}
          {!hideFiles && item.downloads.length > 0 && (
            <DownloadFiles
              item={item}
              remoteProbeStatus={remoteProbeStatus}
              busy={busy}
              pull={(file) => void pull(file)}
              push={(file) => void push(file)}
            />
          )}
          {!hideFiles && item.localFileCount > 0 && (
            <ul className="file-list">
              {item.localFiles.map((file) => (
                <li key={file.path}>
                  <button type="button" onClick={() => void openLocal(file.path).catch((err: unknown) => fail(errorText(err)))}>
                    {file.isDir ? "フォルダ" : "ファイル"} {file.filename}
                  </button>
                  <span className="row-actions">
                    {!file.isDir && isUnityOpenableName(file.filename) && (
                      <UnityOpenButton target={{ path: file.path, boothId: item.boothId }} notify={notify} fail={fail} />
                    )}
                    <span>{formatBytes(file.size)}</span>
                  </span>
                </li>
              ))}
              {item.localFileCount > item.localFiles.length && (
                <li>ほか {item.localFileCount - item.localFiles.length} 件</li>
              )}
            </ul>
          )}
        </div>
      </div>
      <div className="row-actions"><button className="button" type="button" onClick={() => void openBoothLink(item.url)}>BOOTH</button><ExcludeProductButton boothId={item.boothId} title={item.title} category={item.category} genre={item.genre} classificationPinned={item.classificationPinned} /></div>
    </article>
  );
}

function hasRealFileName(filename: string): boolean {
  return !/^\d{4,12}$/.test(filename);
}

function DownloadFiles({
  item,
  remoteProbeStatus,
  busy,
  pull,
  push,
}: {
  item: ItemView;
  remoteProbeStatus: RemoteProbeStatus;
  busy: string;
  pull: (file: DownloadView) => void;
  push: (file: DownloadView) => void;
}) {
  const { builtinBrowser, fail } = useShell();
  // 受け取りは App の一覧が持つ。別のファイルと並行して進み、画面を移っても続く。
  const downloads = useDownloads();
  const receive = (file: DownloadView) =>
    downloads.start({ boothId: item.boothId, variation: file.variation, filename: file.filename });

  const present = item.downloads.filter((file) => file.state !== "missing");
  const missing = item.downloads.filter((file) => file.state === "missing" && !file.superseded);
  const superseded = item.downloads.filter((file) => file.state === "missing" && Boolean(file.superseded));
  const foldMissing = missing.length >= 4;
  const unnamed = item.downloads.some((file) => !hasRealFileName(file.filename));
  const row = (file: DownloadView) => (
    <DownloadRow
      key={`${file.variation}:${file.filename}:${file.state}`}
      file={file}
      item={item}
      remoteProbeStatus={remoteProbeStatus}
      busy={busy}
      entry={
        downloads.board.entries.find(
          (entry) =>
            entry.key === downloadKey({ boothId: item.boothId, variation: file.variation, filename: file.filename }),
        ) ?? null
      }
      finished={finishedPathIn(downloads.board.entries, item.boothId, file)}
      canReceive={builtinBrowser}
      pull={pull}
      push={push}
      receive={receive}
      reveal={(path) => revealSaved(path, fail)}
    />
  );
  return (
    <>
      {unnamed && (
        <p className="meta">
          ファイル名を取得できていません。BOOTH から商品情報を読み込み直してください。
        </p>
      )}
      <ul className="file-list">
        {present.map(row)}
        {foldMissing ? (
          <li className="download-fold">
            <details>
              <summary>未ダウンロード {missing.length}件</summary>
              <ul className="file-list">{missing.map(row)}</ul>
            </details>
          </li>
        ) : (
          missing.map(row)
        )}
        {superseded.length > 0 && (
          <li className="download-fold superseded-fold">
            <details>
              <summary>以前のファイル {superseded.length}件</summary>
              <ul className="file-list">{superseded.map(row)}</ul>
            </details>
          </li>
        )}
      </ul>
    </>
  );
}

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? "";
}

/** 完了イベントが持つ移動先のパス。行の名前が実際のファイル名に変わった場合も拾う。 */
export function finishedPath(live: BoothFileStatus | null, file: DownloadView): string | null {
  if (!live || live.phase !== "done" || !live.path) return null;
  if (live.variation !== file.variation) return null;
  if (live.filename !== file.filename && baseName(live.path) !== file.filename) return null;
  return live.path;
}

/** 一覧の終わった項目のうち、この行のファイルを移した先。行の名前が実際のファイル名に変わった場合も拾う。 */
export function finishedPathIn(entries: DownloadEntry[], boothId: string, file: DownloadView): string | null {
  for (const entry of entries) {
    if (entry.boothId !== boothId || entry.phase !== "done") continue;
    const path = finishedPath(
      { boothId, variation: entry.variation, filename: entry.filename, phase: "done", message: entry.message, path: entry.path },
      file,
    );
    if (path) return path;
  }
  return null;
}

function revealSaved(path: string, fail: (message: string) => void) {
  void revealInFolder(path).catch((err: unknown) => fail(`保存先を開けませんでした: ${errorText(err)}`));
}

function DownloadRow({
  file,
  item,
  remoteProbeStatus,
  busy,
  entry,
  finished,
  canReceive,
  pull,
  push,
  receive,
  reveal,
}: {
  file: DownloadView;
  item: ItemView;
  remoteProbeStatus: RemoteProbeStatus;
  busy: string;
  entry: DownloadEntry | null;
  finished: string | null;
  canReceive: boolean;
  pull: (file: DownloadView) => void;
  push: (file: DownloadView) => void;
  receive: (file: DownloadView) => void;
  reveal: (path: string) => void;
}) {
  const { notify, fail } = useShell();
  const status = entry && (file.state === "missing" || entry.phase === "error") ? entry : null;
  const running = entry !== null && isDownloadActive(entry);
  const savedPath = file.state === "local" && file.localPath ? file.localPath : finished;
  const offer = file.state === "missing" && canReceive && !finished;
  return (
    <li
      className={`download-row ${file.state}${offer && !running ? " can-receive" : ""}`}
      onClick={() => {
        if (offer && !running) receive(file);
      }}
    >
      <div>
        <span className="download-name">{file.filename}</span>
        {file.isNew ? <span className="badge new-file">新しいファイル</span> : null}
        {file.variation ? <span className="meta"> {file.variation}</span> : null}
        <span className={status?.phase === "error" ? "meta warn-text" : `meta state-${file.state}`}>
          {" "}
          {status ? downloadDetail(status) : placeLabelForFile(file, item, remoteProbeStatus)}
        </span>
        {file.sizeBytes != null && file.sizeBytes > 0 ? <span className="meta"> {formatBytes(file.sizeBytes)}</span> : null}
      </div>
      <div className="row-actions">
        {offer && (
          <button
            type="button"
            disabled={running}
            onClick={(event) => {
              event.stopPropagation();
              receive(file);
            }}
          >
            {running ? (entry?.phase === "queued" ? "順番待ち" : "ダウンロード中") : "ダウンロード"}
          </button>
        )}
        {savedPath && (
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              reveal(savedPath);
            }}
          >
            保存先を開く
          </button>
        )}
        {file.state === "local" && file.localPath && isUnityOpenableName(file.localPath) && (
          <UnityOpenButton target={{ path: file.localPath, boothId: item.boothId }} notify={notify} fail={fail} />
        )}
        {file.state === "local" && item.remoteConfigured && item.remoteReachable && (
          <button type="button" disabled={busy === file.filename} onClick={() => push(file)}>
            {busy === file.filename ? "コピー中" : "リモートへコピー"}
          </button>
        )}
        {file.state === "remote" && item.remoteChecked && (
          <button type="button" disabled={busy === file.filename} onClick={() => pull(file)}>
            {busy === file.filename ? "コピー中" : "ローカルへコピー"}
          </button>
        )}
      </div>
    </li>
  );
}
