import { useEffect, useState } from "react";
import { api, isTauri, openBoothLink } from "../api";
import { filePresence, mediaSrc } from "../media";
import { PREVIEW_SETTLE_MS, shouldRequestLargePreview } from "../previewPane";
import { GENRE_LABEL } from "../types";
import type { ItemView } from "../types";
import { FileMark } from "../ui";

type Phase = "idle" | "adult" | "settling" | "loading" | "ready" | "fallback";

export function useLargePreview(item: ItemView | null, enabled: boolean) {
  const boothId = item?.boothId ?? null;
  const isAdult = item?.isAdult ?? false;
  const [revealedFor, setRevealedFor] = useState<string | null>(null);
  const revealed = boothId != null && revealedFor === boothId;
  const [picture, setPicture] = useState<{ id: string; path: string | null; phase: Phase } | null>(null);

  useEffect(() => {
    if (!enabled || !boothId) return;
    if (!shouldRequestLargePreview(isAdult, revealed)) {
      setPicture({ id: boothId, path: null, phase: "adult" });
      return;
    }
    const handle = { cancelled: false, timer: 0 };
    setPicture({ id: boothId, path: null, phase: "settling" });
    void api.cacheProductPreview(boothId, false, true).then(
      (cached) => {
        if (handle.cancelled) return;
        if (cached) {
          setPicture({ id: boothId, path: cached, phase: "ready" });
          return;
        }
        handle.timer = window.setTimeout(() => {
          if (handle.cancelled) return;
          setPicture({ id: boothId, path: null, phase: "loading" });
          void api.cacheProductPreview(boothId, true, true).then(
            (path) => {
              if (handle.cancelled) return;
              setPicture({ id: boothId, path, phase: path ? "ready" : "fallback" });
            },
            () => {
              if (!handle.cancelled) setPicture({ id: boothId, path: null, phase: "fallback" });
            },
          );
        }, PREVIEW_SETTLE_MS);
      },
      () => {
        if (!handle.cancelled) setPicture({ id: boothId, path: null, phase: "fallback" });
      },
    );
    return () => {
      handle.cancelled = true;
      window.clearTimeout(handle.timer);
    };
  }, [enabled, boothId, isAdult, revealed]);

  const current = picture && picture.id === boothId ? picture : null;
  const phase: Phase = !enabled || !boothId ? "idle" : (current?.phase ?? (shouldRequestLargePreview(isAdult, revealed) ? "settling" : "adult"));
  return {
    largePath: current?.path ?? null,
    phase,
    reveal: () => {
      if (boothId) setRevealedFor(boothId);
    },
  };
}

function located(stored: string | null): string | null {
  if (!stored) return null;
  if (!isTauri()) return stored;
  return mediaSrc(stored, null);
}

function PreviewVisual({
  name,
  largePath,
  thumbPath,
  thumbUrl,
}: {
  name: string;
  largePath: string | null;
  thumbPath: string | null;
  thumbUrl: string | null;
}) {
  const [largeFailed, setLargeFailed] = useState(false);
  const [thumbFailed, setThumbFailed] = useState(false);
  useEffect(() => {
    setLargeFailed(false);
    setThumbFailed(false);
  }, [largePath, thumbPath, thumbUrl, name]);
  const large = largeFailed ? null : located(largePath);
  const thumb = thumbFailed ? null : mediaSrc(thumbPath, thumbUrl);
  const src = large ?? thumb;
  const usingLarge = src != null && src === large;
  if (!src) {
    const letter = name.trim().slice(0, 1) || "?";
    return (
      <div className="preview-frame" aria-hidden="true">
        <span className="preview-letter">{letter}</span>
      </div>
    );
  }
  return (
    <div className="preview-frame">
      <img
        src={src}
        alt=""
        draggable={false}
        referrerPolicy="no-referrer"
        onError={() => {
          if (usingLarge) setLargeFailed(true);
          else setThumbFailed(true);
        }}
      />
    </div>
  );
}

function genreLabel(genre: string | null): string {
  if (!genre) return "未設定";
  return GENRE_LABEL[genre] ?? genre;
}

export function PreviewPane({
  item,
  largePath,
  phase,
  onReveal,
  emptyText = "商品を選ぶと、ここに画像を表示します。",
}: {
  item: ItemView | null;
  largePath: string | null;
  phase: Phase;
  onReveal: () => void;
  emptyText?: string;
}) {
  return (
    <section className="panel preview-pane" id="asset-preview" aria-label="プレビュー">
      {item == null ? (
        <p className="meta">{emptyText}</p>
      ) : (
        <>
          <PreviewVisual
            name={item.title}
            largePath={phase === "adult" ? null : largePath}
            thumbPath={item.thumbnailPath}
            thumbUrl={item.thumbnailUrl}
          />
          {phase === "adult" && (
            <div className="preview-adult">
              <p className="meta">表示するまで、大きい画像は出ません。</p>
              <button className="button" type="button" onClick={onReveal}>
                表示する
              </button>
            </div>
          )}
          {phase === "loading" && <p className="meta">読み込み中</p>}
          {phase === "fallback" && (item.thumbnailPath || item.thumbnailUrl) && (
            <p className="meta">大きい画像は表示できないため、小さい画像を出しています。</p>
          )}
          <h2>
            {item.title}
            {item.unpublished && <span className="badge">非公開</span>}
            {item.isFree && <span className="badge">無料</span>}
            {item.isAdult && <span className="badge adult">R-18</span>}
          </h2>
          <dl className="preview-facts">
            <div>
              <dt>ショップ</dt>
              <dd>{item.shop || "未設定"}</dd>
            </div>
            <div>
              <dt>ジャンル</dt>
              <dd>{genreLabel(item.genre)}</dd>
            </div>
            <div>
              <dt>ファイル</dt>
              <dd>
                <FileMark presence={filePresence(item.downloads, item.localFileCount)} />
              </dd>
            </div>
          </dl>
          <div className="row-actions">
            <button className="button" type="button" onClick={() => void openBoothLink(item.url)}>
              BOOTH
            </button>
          </div>
        </>
      )}
    </section>
  );
}
