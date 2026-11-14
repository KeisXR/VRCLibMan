import { useCallback, useEffect, useRef, useState } from "react";
import { assetUpdates, isTauri, onAssetUpdatesConfigured } from "../api";
import { assetUpdateListSummary } from "../assetUpdatesUi";
import type { AssetUpdateStatus } from "../types";
import { errorText, ProgressTrack } from "../ui";
import type { NoticeAction, NotifyOptions } from "../notice";
import { ReclassifyIcon } from "./navIcons";

/**
 * The manual asset-update check, reachable from the sidebar above the account pill.
 * Scope follows the settings in 「アセットの更新設定」: the all-products switch and the
 * per-product opt-ins decide the targets, and the manual check re-reads the library
 * first when the built-in browser is on. This button only runs that same check.
 */
export function useSidebarAssetUpdate(
  notify: (message: string, action?: NoticeAction, options?: NotifyOptions) => void,
  fail: (message: string) => void,
) {
  const [status, setStatus] = useState<AssetUpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const readSequence = useRef(0);
  const actionSequence = useRef(0);
  const observedRunning = useRef(false);
  const refresh = useCallback(() => {
    const request = ++readSequence.current;
    void assetUpdates.status().then((s) => {
      if (request !== readSequence.current) return;
      setStatus(s);
      if (s.running) observedRunning.current = true;
      else if (observedRunning.current) setChecking(false);
    }, () => {});
  }, []);
  useEffect(() => {
    let alive = true;
    refresh();
    // A configuration change in the settings screen emits no event from Rust; the
    // settings call announces it in this window (onAssetUpdatesConfigured), so the
    // first target enabled there surfaces here at once. Changing pages does not
    // read the status again: with every product as a target it is a large read.
    // Once a minute stays as a fallback.
    const timer = window.setInterval(refresh, 60000);
    const stopConfigured = onAssetUpdatesConfigured(refresh);
    let stop = () => {};
    if (isTauri()) {
      void import("@tauri-apps/api/event")
        .then(({ listen }) => listen("asset-updates-changed", refresh))
        .then((unlisten) => { if (alive) stop = unlisten; else unlisten(); });
    }
    return () => {
      alive = false;
      readSequence.current++;
      window.clearInterval(timer);
      stopConfigured();
      stop();
    };
  }, [refresh]);
  const running = checking || Boolean(status?.running);
  async function run() {
    if (running) return;
    const request = ++actionSequence.current;
    observedRunning.current = false;
    setChecking(true);
    try {
      const next = await assetUpdates.check();
      if (request !== actionSequence.current) return;
      readSequence.current++;
      setStatus(next);
      // A result containing failures must stay on screen (the notification
      // rules in AGENTS.md); the check resolves even when some rows failed.
      // Changes raise the shared banner through the asset-updates-changed
      // event; a quiet result still needs an answer to the press.
      const failed = next.rows.filter((r) => r.error).length;
      const changedAny = next.rows.some((r) => r.changes.length > 0);
      if (failed > 0) notify(assetUpdateListSummary(next), undefined, { persist: true });
      else if (!changedAny) notify(assetUpdateListSummary(next));
    } catch (err) {
      if (request === actionSequence.current) fail(errorText(err));
    } finally {
      if (request === actionSequence.current) setChecking(false);
    }
  }
  return { status, running, run, refresh };
}

export function SidebarAssetUpdateButton({ compact, status, running, run }: {
  compact: boolean;
  status: AssetUpdateStatus | null;
  running: boolean;
  run: () => Promise<void>;
}) {
  if (!running && !status?.rows.some((row) => row.enabled)) return null;
  const label = running ? "アセットの更新を確認中…" : "アセットの更新を確認";
  const progress = status?.progress;
  const progressText = progress ? `${progress.done} / ${progress.total} 件` : "確認の準備中…";
  return (
    <div className="sidebar-asset-check">
      <button
        type="button"
        className={compact ? "rail-icon-button" : "sidebar-asset-update"}
        onClick={() => void run()}
        disabled={running}
        aria-label={compact ? label : undefined}
        title={running ? `${label} ${progressText}` : compact ? label : "確認対象は設定の「アセットの更新設定」に従います"}
      >
        <ReclassifyIcon />
        {!compact && <span className="sidebar-asset-update-label">{label}</span>}
      </button>
      {running && <div className="sidebar-asset-progress" title={progressText}>
        {!compact && <span>{progressText}</span>}
        <ProgressTrack current={progress?.done ?? 0} total={progress?.total ?? 0} label={`アセットの更新を確認中：${progressText}`} />
      </div>}
    </div>
  );
}
