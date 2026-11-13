import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, listenBoothFile, revealInFolder } from "./api";
import { openSettingsSection } from "./anchorScroll";
import {
  EMPTY_DOWNLOADS,
  activeDownloads,
  applyFileStatus,
  clearFinishedDownloads,
  downloadBlocker,
  downloadKey,
  finishDownload,
  isDownloadActive,
  startDownload,
  type DownloadBoard,
  type DownloadTarget,
} from "./downloads";
import type { NoticeAction, NotifyOptions } from "./notice";
import { invalidateRemoteProbe } from "./remoteProductProbe";
import { errorText } from "./ui";

export interface DownloadsValue {
  board: DownloadBoard;
  active: number;
  /** 1件を受け取る。別のファイルとは並行して進み、同じファイルの受け取り中は何もしない。 */
  start: (target: DownloadTarget) => void;
  clearFinished: () => void;
  reveal: (path: string) => void;
}

const DownloadsContext = createContext<DownloadsValue | null>(null);

export const DownloadsProvider = DownloadsContext.Provider;

export function useDownloads(): DownloadsValue {
  const value = useContext(DownloadsContext);
  if (!value) throw new Error("downloads missing");
  return value;
}

/** App が1つだけ持つ。画面を移っても一覧と受け取りは続く。 */
export function useDownloadBoard({
  notify,
  fail,
  refresh,
}: {
  notify: (message: string, action?: NoticeAction, options?: NotifyOptions) => void;
  fail: (message: string) => void;
  refresh: () => void;
}): DownloadsValue {
  const [board, setBoard] = useState<DownloadBoard>(EMPTY_DOWNLOADS);
  const boardRef = useRef(board);
  boardRef.current = board;
  // 設定を確かめている間の二度押しで、同じファイルを二重に始めないため。
  const checking = useRef(new Set<string>());
  const handlers = useRef({ notify, fail, refresh });
  handlers.current = { notify, fail, refresh };

  useEffect(() => {
    let alive = true;
    let stop = () => undefined as void;
    void listenBoothFile((status) => {
      if (!alive) return;
      if (status.phase === "done") invalidateRemoteProbe(status.boothId);
      setBoard((current) => applyFileStatus(current, status));
    }).then((unlisten) => {
      if (!alive) unlisten();
      else stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, []);

  const reveal = useCallback((path: string) => {
    void revealInFolder(path).catch((err: unknown) =>
      handlers.current.fail(`保存先を開けませんでした: ${errorText(err)}`),
    );
  }, []);

  const receive = useCallback(
    (target: DownloadTarget) => {
      setBoard((current) => startDownload(current, target));
      void api.downloadBoothFile(target.boothId, target.variation, target.filename).then(
        (placed) => {
          invalidateRemoteProbe(target.boothId);
          setBoard((current) => finishDownload(current, target, { ok: true, path: placed || null }));
          handlers.current.notify(
            `${target.filename} をライブラリに移しました`,
            placed ? { label: "保存先を開く", run: () => reveal(placed) } : undefined,
          );
          handlers.current.refresh();
        },
        (err: unknown) => {
          const message = errorText(err);
          setBoard((current) => finishDownload(current, target, { ok: false, message }));
          handlers.current.fail(message);
        },
      );
    },
    [reveal],
  );

  // 設定が足りないときは始めず（サイドバーに失敗を残さず）、設定の該当する節へ案内する。
  const start = useCallback(
    (target: DownloadTarget) => {
      const key = downloadKey(target);
      const running = boardRef.current.entries.find((entry) => entry.key === key);
      if ((running && isDownloadActive(running)) || checking.current.has(key)) return;
      checking.current.add(key);
      void api
        .settings()
        .then(
          (settings) => {
            const blocked = downloadBlocker(settings);
            if (!blocked) {
              receive(target);
              return;
            }
            const section = settings.builtinBrowser ? "settings-library" : "settings-booth";
            handlers.current.notify(blocked, { label: "設定を開く", run: () => openSettingsSection(section) }, { persist: true });
          },
          // 設定を読めないときは Rust の判定に任せる。
          () => receive(target),
        )
        .finally(() => checking.current.delete(key));
    },
    [receive],
  );

  const clearFinished = useCallback(() => setBoard((current) => clearFinishedDownloads(current)), []);

  return useMemo(
    () => ({ board, active: activeDownloads(board), start, clearFinished, reveal }),
    [board, start, clearFinished, reveal],
  );
}
