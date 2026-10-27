import { useEffect, useState } from "react";
import { api } from "../api";
import { formatBytes } from "../media";
import { useAppUpdater } from "../useAppUpdater";
export function SidebarUpdaterBadge({ onOpenSettings }: { onOpenSettings: () => void }) {
  const { status, pending } = useAppUpdater();
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
  else if (phase === "ready" && version) label = `v${version} の準備ができました（設定で再起動）`;
  else return null;

  async function dismiss() {
    if (!version) return;
    const next = await api.updateSettings({ uiUpdateDismissedVersion: version });
    setDismissedVersion(next.uiUpdateDismissedVersion);
  }

  return (
    <div className={`sidebar-updater${faint ? " faint" : ""}`}>
      <div className="sidebar-updater-row">
        <button
          type="button"
          className="sidebar-updater-main"
          onClick={onOpenSettings}
          disabled={pending}
          title="設定の「アプリの更新」を開きます"
        >
          {label}
        </button>
        {phase === "available" && (
          <button
            type="button"
            className="sidebar-updater-dismiss"
            onClick={() => void dismiss()}
            aria-label="この版は通知しない"
            title="この版は通知しない"
          >
            ×
          </button>
        )}
      </div>
      {phase === "downloading" && (
        <progress className="sidebar-updater-progress" aria-label="更新のダウンロード" max={100} value={percent ?? undefined} />
      )}
    </div>
  );
}
