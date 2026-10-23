import { useEffect, useState } from "react";
import { api, openExternal, openLocal, revealInFolder } from "../api";
import { useShell } from "../App";
import { confidenceLabel, filePresence, formatBytes } from "../media";
import { CATEGORY_LABEL } from "../types";
import type { AvatarDetail, DownloadView, ItemView, Knowledge } from "../types";
import { FileMark, Thumb, errorText } from "../ui";

export function AvatarDetailPage({ id, fileBoothId }: { id: string; fileBoothId?: string }) {
  const { revision, refresh, notify, fail } = useShell();
  const [detail, setDetail] = useState<AvatarDetail | null>(null);
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [intoId, setIntoId] = useState("");

  useEffect(() => {
    let cancelled = false;
    api.avatar(id).then(
      (value) => {
        if (cancelled) return;
        setDetail(value);
        if (avatarNeedsRemoteProbe(value)) {
          api.probeRemoteFiles(id).then(
            (next) => {
              if (!cancelled) setDetail(next);
            },
            () => undefined,
          );
        }
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
            {detail.owned ? "所持アバター" : "衣装の対応先。所持アバターとしては記録していません。"}
            {detail.aliases.length > 0 ? ` ・ 別名 ${detail.aliases.join(" / ")}` : ""}
            {detail.baseBodyName ? ` ・ ${detail.baseBodyName}` : ""}
          </p>
        </div>
      </div>
      {detail.baseBodyNote && <p className="note">{detail.baseBodyNote}</p>}
      {detail.siblings.length > 0 && (
        <div className="siblings">
          {detail.siblings.map((sibling) => (
            <a key={sibling.id} href={`#/avatar/${encodeURIComponent(sibling.id)}`}>
              {sibling.displayName}
            </a>
          ))}
        </div>
      )}
      <div className={detail.isBuiltin ? "split" : "split with-side"}>
        <div>
          {detail.ownedProducts.map((owned) => (
            <article className="item owned" key={owned.boothId}>
              <Thumb path={owned.thumbnailPath} url={owned.thumbnailUrl} name={detail.displayName} small />
              <div>
                <h3>{owned.title}</h3>
                <div className="meta">{owned.shop}</div>
              </div>
              <button className="button" type="button" onClick={() => void openExternal(owned.url)}>
                Booth
              </button>
            </article>
          ))}
          {detail.sections.map((section) => (
            <div className="section" key={section.category}>
              <h2>{CATEGORY_LABEL[section.category] ?? section.category}</h2>
              <div className="stack">
                {section.items.map((item) => (
                  <ItemCard key={item.boothId} item={item} />
                ))}
              </div>
            </div>
          ))}
          {detail.genericItems.length > 0 && (
            <details className="generic">
              <summary>汎用 {detail.genericItems.length} 件</summary>
              <div className="stack">
                {detail.genericItems.map((item) => (
                  <ItemCard key={item.boothId} item={item} />
                ))}
              </div>
            </details>
          )}
          {detail.sections.length === 0 && detail.genericItems.length === 0 && (
            <p className="empty">このアバターに結び付いた衣装・髪・ギミックはまだありません。</p>
          )}
        </div>
        {!detail.isBuiltin && (
          <aside className="panel" style={{ padding: 14 }}>
            <h2 style={{ marginTop: 0 }}>既存のアバターへ統合</h2>
            <p className="meta">
              {detail.isPlaceholder
                ? "商品名から仮で作った項目です。対応表にあるアバターへまとめると、衣装の紐づけも移ります。"
                : "この項目を別のアバターへまとめます。初期データのアバター自体は統合先にだけ選べます。"}
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
          </aside>
        )}
      </div>
    </section>
  );
}

function ItemCard({ item }: { item: ItemView }) {
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
          {` ・ 確信度 ${confidenceLabel(item.confidence)}`}
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
            <p className="meta">ファイル情報はまだありません。取り込み直すか、フォルダを調べてください</p>
          )}
          {item.downloads.length > 0 && item.remoteConfigured && item.remoteChecked && !item.remoteReachable && (
            <p className="meta">リモートのフォルダを確認できません。接続してからもう一度開いてください。</p>
          )}
          {item.downloads.length > 0 && (
            <DownloadFiles item={item} busy={busy} pull={(file) => void pull(file)} push={(file) => void push(file)} />
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
      <button className="button" type="button" onClick={() => void openExternal(item.url)}>
        Booth
      </button>
    </article>
  );
}

function avatarNeedsRemoteProbe(detail: AvatarDetail): boolean {
  const items = [...detail.sections.flatMap((section) => section.items), ...detail.genericItems];
  return items.some((item) => item.remoteConfigured && !item.remoteChecked);
}

function hasRealFileName(filename: string): boolean {
  return !/^\d{4,12}$/.test(filename);
}

function DownloadFiles({
  item,
  busy,
  pull,
  push,
}: {
  item: ItemView;
  busy: string;
  pull: (file: DownloadView) => void;
  push: (file: DownloadView) => void;
}) {
  const present = item.downloads.filter((file) => file.state !== "missing");
  const missing = item.downloads.filter((file) => file.state === "missing");
  const foldMissing = missing.length >= 4;
  const unnamed = item.downloads.some((file) => !hasRealFileName(file.filename));
  const row = (file: DownloadView) => (
    <DownloadRow key={`${file.variation}:${file.filename}:${file.state}`} file={file} item={item} busy={busy} pull={pull} push={push} />
  );
  return (
    <>
      {unnamed && (
        <p className="meta">
          ファイル名がない行は番号だけ出しています。拡張を更新してライブラリを取り込み直すと、名前が付きます。
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
  busy,
  pull,
  push,
}: {
  file: DownloadView;
  item: ItemView;
  busy: string;
  pull: (file: DownloadView) => void;
  push: (file: DownloadView) => void;
}) {
  return (
    <li className={`download-row ${file.state}`}>
      <div>
        <span className="download-name">{file.filename}</span>
        {file.variation ? <span className="meta"> {file.variation}</span> : null}
        <span className={`meta state-${file.state}`}> {placeLabel(file, item)}</span>
        {file.sizeBytes != null && file.sizeBytes > 0 ? <span className="meta"> {formatBytes(file.sizeBytes)}</span> : null}
      </div>
      <div className="row-actions">
        {file.state === "local" && file.localPath && (
          <button type="button" onClick={() => void revealInFolder(file.localPath || "")}>
            保存先を開く
          </button>
        )}
        {file.state === "local" && item.remoteConfigured && item.remoteReachable && (
          <button type="button" disabled={busy === file.filename} onClick={() => push(file)}>
            {busy === file.filename ? "コピー中" : "リモートに送る"}
          </button>
        )}
        {file.state === "remote" && item.remoteChecked && (
          <button type="button" disabled={busy === file.filename} onClick={() => pull(file)}>
            {busy === file.filename ? "コピー中" : "ローカルに取ってくる"}
          </button>
        )}
      </div>
    </li>
  );
}

function placeLabel(file: DownloadView, item: ItemView): string {
  if (file.state === "local") return "ローカルにある";
  if (item.remoteConfigured && !item.remoteChecked) return "確認しています";
  if (file.state === "remote") return "リモートにだけある";
  return "未ダウンロード";
}
