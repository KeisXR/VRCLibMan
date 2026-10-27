import { useEffect, useState } from "react";
import { api } from "../api";
import { formatBytes } from "../media";
import { useAppUpdater } from "../useAppUpdater";
export function SidebarUpdaterBadge({
  workInProgress,
  onOpenSettings,
}: {
  workInProgress: boolean;
  onOpenSettings: () => void;
}) {
  const { status, pending, check } = useAppUpdater();
  const [dismissedVersion, setDismissedVersion] = useState("");
  const phase = status?.phase;
  const version = status?.version;
  const hidden =
    phase === "available" && version != null && version !== "" && dismissedVersion === version;

  useEffect(() => {
    void api.settings().then((settings) => setDismissedVersion(settings.uiUpdateDismissedVersion));
  }, []);

  if (!status || status.phase === "unavailable" || hidden) return null;

  const total = status.total;
  const percent =
    total && status.phase === "downloading"
      ? Math.min(100, Math.floor((status.downloaded / total) * 100))
      : null;
  const faint = phase === "checking" || (phase === "idle" && status.error);

  let label = "";
  if (phase === "checking") label = "更新を確認中…";
  else if (phase === "idle" && status.error) label = "更新を確認できませんでした";
  else if (phase === "available" && version) label = `v${version} に更新できます`;
  else if (phase === "downloading")
    label = `ダウンロード中 ${formatBytes(status.downloaded)}${total ? ` / ${formatBytes(total)}` : ""}`;
  else if (phase === "ready" && version) label = "再起動して更新";
  else return null;

  async function dismiss() {
    if (!version) return;
    const next = await api.updateSettings({ uiUpdateDismissedVersion: version });
    setDismissedVersion(next.uiUpdateDismissedVersion);
  }

  return (
    <div className={`sidebar-updater${faint ? " faint" : ""}`}>
      <button type="button" className="sidebar-updater-main" onClick={onOpenSettings} disabled={pending}>
        {label}
      </button>
      {phase === "downloading" && (
        <progress className="sidebar-updater-progress" aria-label="更新のダウンロード" max={100} value={percent ?? undefined} />
      )}
      {phase === "ready" && (
        <button
          type="button"
          className="button"
          disabled={pending || workInProgress}
          onClick={onOpenSettings}
        >
          設定で確認
        </button>
      )}
      {phase === "available" && (
        <details className="sidebar-updater-menu">
          <summary aria-label="更新通知のメニュー">⋯</summary>
          <button type="button" onClick={() => void dismiss()}>この版は通知しない</button>
          <button type="button" onClick={() => void check(true)} disabled={pending || workInProgress}>
            更新を確認
          </button>
        </details>
      )}
    </div>
  );
}
