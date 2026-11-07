import { useEffect, useRef, useState } from "react";
import { assetUpdates, isTauri } from "../api";
import { useShell } from "../App";
import { assetUpdateListSummary } from "../assetUpdatesUi";
import type { AssetUpdateStatus } from "../types";
import { errorText } from "../ui";
import { ReclassifyIcon } from "./navIcons";

/**
 * The manual asset-update check, reachable from the sidebar above the account pill.
 * Scope follows the settings in 「アセットの更新設定」: the all-products switch and the
 * per-product opt-ins decide the targets, and the manual check re-reads the library
 * first when the built-in browser is on. This button only runs that same check.
 */
export function SidebarAssetUpdateButton({ compact = false }: { compact?: boolean }) {
  const { notify, fail } = useShell();
  const [status, setStatus] = useState<AssetUpdateStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const readSequence = useRef(0);
  const actionSequence = useRef(0);
  const observedRunning = useRef(false);
  useEffect(() => {
    let alive = true;
    const refresh = () => {
      const request = ++readSequence.current;
      void assetUpdates.status().then((s) => {
        if (!alive || request !== readSequence.current) return;
        setStatus(s);
        if (s.running) observedRunning.current = true;
        else if (observedRunning.current) setChecking(false);
      }, () => {});
    };
    refresh();
    // Configuration changes in the settings screen do not emit an event, so the
    // mounted button also refreshes when the screen changes and once a minute;
    // otherwise the first target enabled in settings would not surface here
    // until some check happened to run.
    const timer = window.setInterval(refresh, 60000);
    window.addEventListener("hashchange", refresh);
    let stop = () => {};
    if (isTauri()) {
      void import("@tauri-apps/api/event")
        .then(({ listen }) => listen("asset-updates-changed", refresh))
        .then((unlisten) => { if (alive) stop = unlisten; else unlisten(); });
    }
    return () => {
      alive = false;
      window.clearInterval(timer);
      window.removeEventListener("hashchange", refresh);
      stop();
    };
  }, []);
  const enabled = status?.rows.filter((r) => r.enabled).length ?? 0;
  if (!status || enabled === 0) return null;
  const running = checking || status.running;
  const changed = status.rows.filter((r) => r.changes.length > 0).length;
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
  const label = running ? `アセットの更新を確認中…${status.progress ? ` ${status.progress.done} / ${status.progress.total} 件` : ""}` : "アセットの更新を確認";
  if (compact) {
    return (
      <button
        type="button"
        className="rail-icon-button"
        onClick={() => void run()}
        disabled={running}
        aria-label={label}
        title={label}
      >
        <ReclassifyIcon />
      </button>
    );
  }
  return (
    <button
      type="button"
      className="sidebar-asset-update"
      onClick={() => void run()}
      disabled={running}
      title="確認対象は設定の「アセットの更新設定」に従います"
    >
      <ReclassifyIcon />
      <span className="sidebar-asset-update-label">{label}</span>
      {changed > 0 && <span className="count">{changed}</span>}
    </button>
  );
}
