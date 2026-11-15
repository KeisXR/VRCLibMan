import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useModalFocus } from "../keyboard";
import { useDownloads } from "../useDownloads";

/**
 * BOOTH に無かったファイル（作者の差し替えで古いダウンロード番号が無くなった）を、
 * 「このアセットを更新しますか？」で尋ねる。更新はライブラリを読み直して、新しいファイルを受け取る。
 * 振る舞いは docs/asset-update-log.md の「ダウンロードしたいファイルが BOOTH に無いとき」。
 */
export function AssetUpdatePrompt() {
  const { vanished, updateVanished, dismissVanished } = useDownloads();
  const current = vanished[0];
  if (!current || typeof document === "undefined") return null;
  return (
    <PromptBox
      title={current.title}
      filename={current.filename}
      variation={current.variation}
      others={vanished.length - 1}
      onUpdate={() => updateVanished(false)}
      onUpdateAll={() => updateVanished(true)}
      onClose={dismissVanished}
    />
  );
}

function PromptBox({
  title,
  filename,
  variation,
  others,
  onUpdate,
  onUpdateAll,
  onClose,
}: {
  title?: string;
  filename: string;
  variation: string;
  others: number;
  onUpdate: () => void;
  onUpdateAll: () => void;
  onClose: () => void;
}) {
  const panel = useRef<HTMLElement>(null);
  // Tab で背後の画面へ出ない。閉じたら押したボタンへフォーカスを戻す。
  useModalFocus(panel);
  useEffect(() => {
    panel.current?.querySelector<HTMLElement>("button")?.focus();
  }, [filename]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  return createPortal(
    <div
      className="preview-overlay unity-open-overlay"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        ref={panel}
        className="panel unity-open-dialog asset-update-prompt"
        role="dialog"
        aria-modal="true"
        aria-labelledby="asset-update-prompt-title"
        tabIndex={-1}
      >
        <h2 id="asset-update-prompt-title">このアセットを更新しますか？</h2>
        <p className="asset-update-prompt-file">
          {title && <strong>{title}</strong>}
          <span className="download-name">{filename}</span>
          {variation && <span className="meta">{variation}</span>}
        </p>
        <p className="meta">
          このファイルは BOOTH にありません。商品が更新されて、ファイルが差し替えられた可能性があります。
          更新すると BOOTH のライブラリを読み直し、新しいファイルが見つかればダウンロードします。
        </p>
        {others > 0 && <p className="meta">ほかに {others} 件のアセットも、ファイルが BOOTH にありませんでした。</p>}
        <div className="row-actions">
          <button className="button primary" type="button" onClick={onUpdate}>
            更新する
          </button>
          {others > 0 && (
            <button className="button" type="button" onClick={onUpdateAll}>
              すべて更新する（{others + 1}件）
            </button>
          )}
          <button className="button" type="button" onClick={onClose}>
            閉じる
          </button>
        </div>
      </section>
    </div>,
    document.body,
  );
}
