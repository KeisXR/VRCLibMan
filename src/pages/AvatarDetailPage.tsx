import { useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import { api, listenBoothFile, openExternal, openLocal, revealInFolder } from "../api";
import { useShell } from "../App";
import { filePresence, formatBytes } from "../media";
import { CATEGORY_LABEL } from "../types";
import type { AvatarDetail, BoothFileStatus, DownloadView, ItemView, Knowledge } from "../types";
import { FileMark, Thumb, errorText } from "../ui";
import { ExcludeProductButton } from "../components/ProductExclusions";
import { type RemoteProbeStatus, placeLabelForFile, useRemoteProductProbe } from "../remoteProductProbe";

export function AvatarDetailPage({ id, fileBoothId }: { id: string; fileBoothId?: string }) {
  const { revision, refresh, notify, fail } = useShell();
  const [detail, setDetail] = useState<AvatarDetail | null>(null);
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
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

  if (!detail) return <p className="meta">読み込み中</p>;
  const targets = (knowledge?.avatars ?? []).filter((avatar) => avatar.id !== detail.id);

  return (
    <section>
      <p><a href="#/">アバター一覧へ</a></p>
      <div className="page-head">
        <div>
          <h1>{detail.displayName}</h1>
          <p>
            {detail.owned ? "所持アバター" : "所持未登録"}
            {detail.baseBodyName ? ` ・ ${detail.baseBodyName}` : ""}
          </p>
        </div>
      </div>
      {(detail.aliases.length > 0 || detail.baseBodyNote) && (
        <details className="generic">
          <summary>アバターの情報</summary>
          {detail.aliases.length > 0 && <p className="meta">別名: {detail.aliases.join(" / ")}</p>}
          {detail.baseBodyNote && <p className="meta">{detail.baseBodyNote}</p>}
        </details>
      )}
      {detail.siblings.length > 0 && (
        <div className="siblings">
          {detail.siblings.map((sibling) => (
            <a key={sibling.id} href={`#/avatar/${encodeURIComponent(sibling.id)}`}>
              {sibling.displayName}
            </a>
          ))}
        </div>
      )}
      <div className="split">
        <div>
          {detail.ownedProducts.map((owned) => (
            <article className="item owned" key={owned.boothId}>
              <Thumb path={owned.thumbnailPath} url={owned.thumbnailUrl} name={detail.displayName} small />
              <div>
                <h3>{owned.title}</h3>
                <div className="meta">{owned.shop}</div>
              </div>
              <button className="button" type="button" onClick={() => void openExternal(owned.url)}>
                BOOTH
              </button>
            </article>
          ))}
          {detail.sections.map((section) => (
            <div className="section" key={section.category}>
              <h2>{CATEGORY_LABEL[section.category] ?? section.category}</h2>
              <div className="stack">
                {section.items.map((item) => (
                  <ItemCard key={item.boothId} {...itemCardProps(item)} />
                ))}
              </div>
            </div>
          ))}
          {detail.genericItems.length > 0 && (
            <details className="generic">
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
  remoteProbeStatus = "ok",
  onRetryRemoteProbe,
}: {
  item: ItemView;
  remoteProbeStatus?: RemoteProbeStatus;
  onRetryRemoteProbe?: () => void;
}) {
  const { refresh, notify, fail } = useShell();
  const [busy, setBusy] = useState("");

  async function pull(file: DownloadView) {
    setBusy(file.filename);
    try {
      await api.pullRemoteFile(item.boothId, file.variation, file.filename);
      notify(`${file.filename} を手元のフォルダへコピーしました`);
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
      await api.pushRemoteFile(item.boothId, file.variation, file.filename);
      notify(`${file.filename} をリモートへコピーしました`);
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy("");
    }
  }

  return (
    <article className="item">
      <Thumb path={item.thumbnailPath} url={item.thumbnailUrl} name={item.title} small />
      <div>
        <h3>
          {item.title}
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
          {item.downloads.length === 0 && item.localFileCount === 0 && (
            <p className="meta">ファイル情報はまだありません。再インポートするか、フォルダを調べてください</p>
          )}
          {item.downloads.length > 0 &&
            item.remoteConfigured &&
            ((item.remoteChecked && !item.remoteReachable) || remoteProbeStatus === "failed") && (
              <p className="meta">リモートのフォルダを確認できません。接続してからもう一度開いてください。</p>
            )}
          {remoteProbeStatus === "failed" && onRetryRemoteProbe && (
            <button className="button" type="button" onClick={onRetryRemoteProbe}>
              もう一度確認
            </button>
          )}
          {item.downloads.length > 0 && (
            <DownloadFiles
              item={item}
              remoteProbeStatus={remoteProbeStatus}
              busy={busy}
              pull={(file) => void pull(file)}
              push={(file) => void push(file)}
            />
          )}
          {item.localFileCount > 0 && (
            <ul className="file-list">
              {item.localFiles.map((file) => (
                <li key={file.path}>
                  <button type="button" onClick={() => void openLocal(file.path)}>
                    {file.isDir ? "フォルダ" : "ファイル"} {file.filename}
                  </button>
                  <span>{formatBytes(file.size)}</span>
                </li>
              ))}
              {item.localFileCount > item.localFiles.length && (
                <li>ほか {item.localFileCount - item.localFiles.length} 件</li>
              )}
            </ul>
          )}
        </div>
      </div>
      <div className="row-actions"><button className="button" type="button" onClick={() => void openExternal(item.url)}>BOOTH</button><ExcludeProductButton boothId={item.boothId} title={item.title} /></div>
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
  const { builtinBrowser, refresh, notify, fail } = useShell();
  const [live, setLive] = useState<BoothFileStatus | null>(null);
  const [receiving, setReceiving] = useState("");
  const running = useRef(false);
  useEffect(() => {
    let stop = () => undefined as void;
    let alive = true;
    void listenBoothFile((status) => {
      if (!alive || status.boothId !== item.boothId) return;
      setLive(status);
    }).then((unlisten) => {
      stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, [item.boothId]);

  async function receive(file: DownloadView) {
    if (running.current) return;
    running.current = true;
    const key = `${file.variation}:${file.filename}`;
    setReceiving(key);
    setLive({
      boothId: item.boothId,
      variation: file.variation,
      filename: file.filename,
      phase: "receiving",
      message: "ダウンロード中",
    });
    try {
      await api.downloadBoothFile(item.boothId, file.variation, file.filename);
      setLive({
        boothId: item.boothId,
        variation: file.variation,
        filename: file.filename,
        phase: "done",
        message: "ライブラリに移しました",
      });
      notify("ファイルをライブラリに移しました");
      refresh();
    } catch (err) {
      const message = errorText(err);
      setLive({
        boothId: item.boothId,
        variation: file.variation,
        filename: file.filename,
        phase: "error",
        message,
      });
      fail(message);
    } finally {
      running.current = false;
      setReceiving("");
    }
  }

  const present = item.downloads.filter((file) => file.state !== "missing");
  const missing = item.downloads.filter((file) => file.state === "missing");
  const foldMissing = missing.length >= 4;
  const unnamed = item.downloads.some((file) => !hasRealFileName(file.filename));
  const row = (file: DownloadView) => (
    <DownloadRow
      key={`${file.variation}:${file.filename}:${file.state}`}
      file={file}
      item={item}
      remoteProbeStatus={remoteProbeStatus}
      busy={busy}
      receiving={receiving}
      live={live}
      canReceive={builtinBrowser}
      pull={pull}
      push={push}
      receive={receive}
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
      </ul>
    </>
  );
}

function DownloadRow({
  file,
  item,
  remoteProbeStatus,
  busy,
  receiving,
  live,
  canReceive,
  pull,
  push,
  receive,
}: {
  file: DownloadView;
  item: ItemView;
  remoteProbeStatus: RemoteProbeStatus;
  busy: string;
  receiving: string;
  live: BoothFileStatus | null;
  canReceive: boolean;
  pull: (file: DownloadView) => void;
  push: (file: DownloadView) => void;
  receive: (file: DownloadView) => void;
}) {
  const key = `${file.variation}:${file.filename}`;
  const status =
    live &&
    live.variation === file.variation &&
    live.filename === file.filename &&
    (file.state === "missing" || live.phase === "error")
      ? live
      : null;
  const offer = file.state === "missing" && canReceive;
  return (
    <li
      className={`download-row ${file.state}${offer ? " can-receive" : ""}`}
      onClick={() => {
        if (offer) receive(file);
      }}
    >
      <div>
        <span className="download-name">{file.filename}</span>
        {file.variation ? <span className="meta"> {file.variation}</span> : null}
        <span className={status?.phase === "error" ? "meta warn-text" : `meta state-${file.state}`}>
          {" "}
          {status ? status.message : placeLabelForFile(file, item, remoteProbeStatus)}
        </span>
        {file.sizeBytes != null && file.sizeBytes > 0 ? <span className="meta"> {formatBytes(file.sizeBytes)}</span> : null}
      </div>
      <div className="row-actions">
        {offer && (
          <button
            type="button"
            disabled={receiving === key}
            onClick={(event) => {
              event.stopPropagation();
              receive(file);
            }}
          >
            {receiving === key ? "ダウンロード中" : "ダウンロード"}
          </button>
        )}
        {file.state === "local" && file.localPath && (
          <button type="button" onClick={() => void revealInFolder(file.localPath || "")}>
            保存先を開く
          </button>
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

