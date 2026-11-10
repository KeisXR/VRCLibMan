import { useEffect, useRef, useState } from "react";
import { downloadDetail, downloadPercent, downloadsLabel, isDownloadActive, overallDownload } from "../downloads";
import { useDownloads } from "../useDownloads";
import { ProgressTrack } from "../ui";
import { DownloadIcon } from "./navIcons";

/**
 * サイドバー下部の「ダウンロード」。押すと、受け取り中と終わったファイルの一覧を上に開く。
 * 一覧はアプリを閉じるまで残り、「完了した項目を消す」で片付ける。
 */
export function SidebarDownloads({ compact }: { compact: boolean }) {
  const { board, active, clearFinished, reveal } = useDownloads();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (board.entries.length === 0) setOpen(false);
  }, [board.entries.length]);

  if (board.entries.length === 0) return null;
  const label = downloadsLabel(board);
  const failed = board.entries.some((entry) => entry.phase === "error");
  const overall = overallDownload(board);
  const finished = board.entries.length - active;

  return (
    <div className="sidebar-downloads account-menu" ref={rootRef}>
      <button
        type="button"
        className={compact ? `rail-icon-button${active > 0 || failed ? " attention" : ""}` : "sidebar-asset-update sidebar-downloads-button"}
        aria-expanded={open}
        aria-haspopup="dialog"
        aria-label={compact ? label : undefined}
        title={compact ? label : "ダウンロードの一覧を開く"}
        onClick={() => setOpen((value) => !value)}
      >
        <DownloadIcon />
        {!compact && <span className="sidebar-asset-update-label">{label}</span>}
        {compact && active > 0 && <span className="sidebar-downloads-badge" aria-hidden="true">{active}</span>}
      </button>
      {!compact && active > 0 && (
        <div className="sidebar-asset-progress">
          <ProgressTrack current={overall.current} total={overall.total} label={label} />
        </div>
      )}
      {open && (
        <div className={compact ? "account-popover download-popover compact" : "account-popover download-popover"} role="dialog" aria-label="ダウンロード">
          <div className="download-popover-head">
            <strong>ダウンロード</strong>
            <button type="button" disabled={finished === 0} onClick={clearFinished}>
              完了した項目を消す
            </button>
          </div>
          <ul className="download-popover-list">
            {board.entries.map((entry) => (
              <li key={entry.key} className={`download-popover-item ${entry.phase}`}>
                <span className="download-popover-name" title={entry.filename}>{entry.filename}</span>
                {entry.variation && <span className="meta download-popover-variation">{entry.variation}</span>}
                <span className={entry.phase === "error" ? "meta warn-text" : "meta"}>{downloadDetail(entry)}</span>
                {isDownloadActive(entry) && (
                  <ProgressTrack
                    current={downloadPercent(entry) ?? 0}
                    total={downloadPercent(entry) === null ? 0 : 100}
                    label={`${entry.filename}: ${downloadDetail(entry)}`}
                  />
                )}
                {entry.phase === "done" && entry.path && (
                  <button type="button" className="download-popover-reveal" onClick={() => reveal(entry.path!)}>
                    保存先を開く
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
