import { useEffect, useState } from "react";
import { api, openExternal, openLocal } from "../api";
import { useShell } from "../App";
import { confidenceLabel, formatBytes } from "../media";
import { CATEGORY_LABEL } from "../types";
import type { AvatarDetail, ItemView, Knowledge } from "../types";
import { Thumb, errorText } from "../ui";

export function AvatarDetailPage({ id }: { id: string }) {
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
  return (
    <article className="item">
      <Thumb path={item.thumbnailPath} url={item.thumbnailUrl} name={item.title} small />
      <div>
        <h3>{item.title}</h3>
        <div className="meta">
          {item.shop}
          {item.viaBaseBody ? " ・ 同じ素体" : ""}
          {item.isGeneric ? " ・ 汎用" : ""}
          {` ・ 確信度 ${confidenceLabel(item.confidence)}`}
        </div>
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
      <button className="button" type="button" onClick={() => void openExternal(item.url)}>
        Booth
      </button>
    </article>
  );
}
