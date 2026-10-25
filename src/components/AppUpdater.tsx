import { useEffect, useState } from "react";
import { appUpdater, openExternal } from "../api";
import { formatBytes } from "../media";
import type { AppUpdateStatus } from "../types";
import { errorText } from "../ui";

export function AppUpdater({
  beforeInstall,
  workInProgress,
}: {
  beforeInstall: () => Promise<unknown>;
  workInProgress: boolean;
}) {
  const [status, setStatus] = useState<AppUpdateStatus | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");

  function accept(next: AppUpdateStatus) {
    setStatus((current) =>
      !current || next.revision >= current.revision ? next : current,
    );
  }

  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await appUpdater.status();
        if (alive) accept(next);
      } catch (err) {
        if (alive) setError(errorText(err));
      }
      if (alive) timer = setTimeout(() => void poll(), 1000);
    };
    void poll();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, []);

  async function run(operation: () => Promise<AppUpdateStatus>) {
    if (pending) return;
    setPending(true);
    setError("");
    try {
      accept(await operation());
    } catch (err) {
      setError(errorText(err));
    } finally {
      setPending(false);
    }
  }

  async function install() {
    if (
      !window.confirm(
        `v${status?.version} に更新して再起動します。作業を終えてから実行してください。ライブラリと設定は引き継がれます。`,
      )
    )
      return;
    await run(async () => {
      await beforeInstall();
      return appUpdater.install();
    });
  }

  const phase = status?.phase;
  const busy =
    pending ||
    !status ||
    phase === "checking" ||
    phase === "downloading" ||
    phase === "installing";
  const total = status?.total;
  const percent =
    total && status
      ? Math.min(100, Math.floor((status.downloaded / total) * 100))
      : null;
  const message =
    phase === "unavailable"
      ? status?.message
      : phase === "checking"
        ? "更新を確認中…"
        : phase === "current"
          ? "最新バージョンです"
          : phase === "available"
            ? `v${status?.version} に更新できます`
            : phase === "downloading"
              ? `ダウンロード中 ${status ? formatBytes(status.downloaded) : ""}${total ? ` / ${formatBytes(total)}` : ""}`
              : phase === "ready"
                ? `v${status?.version} の準備ができました`
                : phase === "installing"
                  ? "更新を適用中…"
                  : "操作したときだけ GitHub Releases に更新を確認します。";

  return (
    <div className="app-updater">
      <p role="status" aria-live="polite">
        {message}
      </p>
      {phase === "downloading" && (
        <progress
          aria-label="更新のダウンロード"
          max={100}
          value={percent ?? undefined}
        />
      )}
      <div className="inline-control">
        {phase !== "ready" && (
          <button
            type="button"
            className="button"
            disabled={busy || phase === "unavailable"}
            onClick={() => void run(appUpdater.check)}
          >
            更新を確認
          </button>
        )}
        {phase === "available" && (
          <button
            type="button"
            className="button primary"
            disabled={busy}
            onClick={() => void run(appUpdater.download)}
          >
            更新をダウンロード
          </button>
        )}
        {phase === "ready" && (
          <button
            type="button"
            className="button primary"
            disabled={busy || workInProgress}
            onClick={() => void install()}
          >
            再起動して更新
          </button>
        )}
        <button
          type="button"
          className="button"
          onClick={() =>
            void openExternal(
              "https://github.com/KeisXR/VRCLibMan/releases",
            ).catch((err) => setError(errorText(err)))
          }
        >
          リリース一覧
        </button>
      </div>
      {phase === "ready" && workInProgress && (
        <p className="meta">実行中の作業が終わると更新できます。</p>
      )}
      {(error || status?.error) && (
        <p className="warn-text" role="alert">
          {error || status?.error}
        </p>
      )}
      {status?.notes && (
        <details className="settings-details">
          <summary>更新内容</summary>
          <div className="update-notes">{status.notes}</div>
        </details>
      )}
      <p className="meta">
        ダウンロード後に署名を検証します。アプリと拡張機能の更新は別です。
      </p>
    </div>
  );
}
