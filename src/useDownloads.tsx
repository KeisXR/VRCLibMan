import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, listenBoothFile, revealInFolder } from "./api";
import {
  EMPTY_DOWNLOADS,
  activeDownloads,
  applyFileStatus,
  clearFinishedDownloads,
  downloadKey,
  finishDownload,
  isDownloadActive,
  startDownload,
  type DownloadBoard,
  type DownloadTarget,
} from "./downloads";
import type { NoticeAction } from "./notice";
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
  notify: (message: string, action?: NoticeAction) => void;
  fail: (message: string) => void;
  refresh: () => void;
}): DownloadsValue {
  const [board, setBoard] = useState<DownloadBoard>(EMPTY_DOWNLOADS);
  const boardRef = useRef(board);
  boardRef.current = board;
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

  const start = useCallback(
    (target: DownloadTarget) => {
      const key = downloadKey(target);
      const running = boardRef.current.entries.find((entry) => entry.key === key);
      if (running && isDownloadActive(running)) return;
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

  const clearFinished = useCallback(() => setBoard((current) => clearFinishedDownloads(current)), []);

  return useMemo(
    () => ({ board, active: activeDownloads(board), start, clearFinished, reveal }),
    [board, start, clearFinished, reveal],
  );
}
