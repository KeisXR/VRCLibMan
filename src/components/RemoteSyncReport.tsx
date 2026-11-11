import { formatBytes } from "../media";
import type { RemoteSyncStatus } from "../types";

function more(count: number, listed: number): string {
  return count > listed ? `ほか ${count - listed} 件` : "";
}

function completedAt(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("ja-JP");
}

/** 自動同期の要約。確認全体の問題と、1 件ずつの問題（競合・手元に無い・失敗・リモートで消えた）を分けて数える。 */
export function remoteSyncSummary(status: RemoteSyncStatus | null): string {
  if (!status) return "同期を待っています";
  if (status.running) return `同期中（${status.copied} 件コピー済み）`;
  if (status.error) return `同期を再試行します: ${status.error}`;
  if (!status.lastCompletedAt) return "同期を待っています";
  const attention = status.conflicts + status.sourceMissing + status.failed + status.missing;
  return `最終確認: ${completedAt(status.lastCompletedAt)}。コピー ${status.copied} 件、リモートにあり ${status.skipped} 件${attention > 0 ? `、確認が必要 ${attention} 件` : ""}`;
}

/** 設定の「リモートへ自動同期」の下に出す内訳と再試行の入口。 */
export function RemoteSyncReport({ status, onRetry }: { status: RemoteSyncStatus | null; onRetry: () => void }) {
  return (
    <div className="section-body remote-sync-report">
      <p className="meta" role="status">{remoteSyncSummary(status)}</p>
      {status && status.conflicts > 0 && (
        <details className="sync-problem" open>
          <summary className="note-strong">同じ名前の別のファイルがリモートにあるため、コピーしなかった: {status.conflicts} 件</summary>
          <ul>
            {status.conflictFiles.map((item) => (
              <li key={item.file}>{item.file}（手元 {formatBytes(item.localSize)}、リモート {formatBytes(item.remoteSize)}）</li>
            ))}
          </ul>
          {more(status.conflicts, status.conflictFiles.length) && <p className="meta">{more(status.conflicts, status.conflictFiles.length)}</p>}
          <p className="meta">リモートのファイルは変更していません。手元のファイルで置き換えるときは、アバター詳細のファイル一覧で「リモートへコピー」を押し、上書きを確認してください。リモートのファイルを移すか消すと、次の確認でコピーします。</p>
        </details>
      )}
      {status && status.failed > 0 && (
        <details className="sync-problem" open>
          <summary className="note-strong">コピーに失敗: {status.failed} 件</summary>
          <ul>
            {status.failures.map((item) => (
              <li key={item.file}>{item.file}: {item.error}</li>
            ))}
          </ul>
          {more(status.failed, status.failures.length) && <p className="meta">{more(status.failed, status.failures.length)}</p>}
        </details>
      )}
      {status && status.sourceMissing > 0 && (
        <details className="sync-problem">
          <summary className="note-strong">手元のファイルが見つからないため、コピーできなかった: {status.sourceMissing} 件</summary>
          <ul>
            {status.sourceMissingFiles.map((file) => <li key={file}>{file}</li>)}
          </ul>
          {more(status.sourceMissing, status.sourceMissingFiles.length) && <p className="meta">{more(status.sourceMissing, status.sourceMissingFiles.length)}</p>}
          <p className="meta">移動や名前の変更をした場合は、ライブラリを調べ直すか、インポート画面の「手元のファイル」で登録し直してください。</p>
        </details>
      )}
      {status && status.missing > 0 && (
        <details className="sync-problem">
          <summary className="note-strong">リモートの元の場所に見つからない: {status.missing} 件</summary>
          <ul>
            {status.missingFiles.map((file) => <li key={file}>{file}</li>)}
          </ul>
          {more(status.missing, status.missingFiles.length) && <p className="meta">{more(status.missing, status.missingFiles.length)}</p>}
          <p className="meta">名前の変更や削除の可能性があるため、自動ではコピーし直しません。戻すときはアバター詳細のファイル一覧で「リモートへコピー」を押してください。</p>
        </details>
      )}
      <div className="row-actions">
        <button className="button secondary" type="button" disabled={status?.running} onClick={onRetry}>
          今すぐ再試行
        </button>
      </div>
    </div>
  );
}

