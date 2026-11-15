import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, listenBoothFile, revealInFolder } from "./api";
import { openSettingsSection } from "./anchorScroll";
import {
  EMPTY_DOWNLOADS,
  activeDownloads,
  addVanished,
  applyFileStatus,
  clearFinishedDownloads,
  downloadBlocker,
  downloadKey,
  finishDownload,
  isDownloadActive,
  isFileGone,
  needsLibraryReread,
  startDownload,
  updateResultMessage,
  type DownloadBoard,
  type DownloadTarget,
  type VanishedFile,
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
  /** BOOTH に無かったファイル。先頭から「このアセットを更新しますか？」で尋ねる。更新中は空。 */
  vanished: VanishedFile[];
  /** 先頭の 1 件（`all` ならたまっているすべて）を更新する。ライブラリを読み直し、新しいファイルを受け取る。 */
  updateVanished: (all: boolean) => void;
  dismissVanished: () => void;
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
  const [vanished, setVanished] = useState<VanishedFile[]>([]);
  const vanishedRef = useRef(vanished);
  vanishedRef.current = vanished;
  const [updating, setUpdating] = useState(false);
  const updatingRef = useRef(false);
  // 更新のために最後にライブラリを読み直し始めた時刻。それより前に消えていたファイルは読み直さずに探す。
  const lastReread = useRef<number | null>(null);
  // 更新で始めたファイル。これも BOOTH に無ければ、もう尋ねずに失敗として出す（尋ね続けないため）。
  const fromUpdate = useRef(new Set<string>());
  // 全取得は受け取り中には始められないので、受け取りが終わるのを待つ。
  const idleWaiters = useRef<(() => void)[]>([]);
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

  useEffect(() => {
    if (activeDownloads(board) > 0 || idleWaiters.current.length === 0) return;
    const waiters = idleWaiters.current;
    idleWaiters.current = [];
    for (const resolve of waiters) resolve();
  }, [board]);

  const reveal = useCallback((path: string) => {
    void revealInFolder(path).catch((err: unknown) =>
      handlers.current.fail(`保存先を開けませんでした: ${errorText(err)}`),
    );
  }, []);

  const receive = useCallback(
    (target: DownloadTarget) => {
      const key = downloadKey(target);
      setBoard((current) => startDownload(current, target));
      void api.downloadBoothFile(target.boothId, target.variation, target.filename).then(
        (placed) => {
          fromUpdate.current.delete(key);
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
          const afterUpdate = fromUpdate.current.delete(key);
          setBoard((current) => finishDownload(current, target, { ok: false, message }));
          if (isFileGone(message) && !afterUpdate) {
            setVanished((list) => addVanished(list, { ...target, at: Date.now() }));
          } else if (isFileGone(message)) {
            handlers.current.fail(`${target.filename}: 更新したファイルも BOOTH にありませんでした。商品ページで確かめてください`);
          } else {
            handlers.current.fail(message);
          }
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

  const dismissVanished = useCallback(() => setVanished([]), []);

  const updateVanished = useCallback(
    (all: boolean) => {
      if (updatingRef.current) return;
      const list = vanishedRef.current;
      if (list.length === 0) return;
      const chosen = all ? list : list.slice(0, 1);
      const chosenKeys = new Set(chosen.map(downloadKey));
      setVanished((current) => current.filter((item) => !chosenKeys.has(downloadKey(item))));
      updatingRef.current = true;
      setUpdating(true);
      void (async () => {
        const { notify, fail, refresh } = handlers.current;
        try {
          if (needsLibraryReread(chosen, lastReread.current)) {
            if (activeDownloads(boardRef.current) > 0) {
              notify("ほかのダウンロードが終わってから、BOOTH のライブラリを読み直します");
              await new Promise<void>((resolve) => idleWaiters.current.push(resolve));
            }
            notify("BOOTH のライブラリを読み直しています。終わったら新しいファイルをダウンロードします");
            const began = Date.now();
            const report = await api.importBoothLibrary();
            refresh();
            if (report.notice) {
              fail(report.notice);
              return;
            }
            if (report.cancelled) {
              notify("ライブラリの読み直しを中止したので、アセットは更新しませんでした");
              return;
            }
            lastReread.current = began;
          }
          const targets = await api.assetUpdateDownloads(
            chosen.map((file) => ({ boothId: file.boothId, variation: file.variation })),
          );
          for (const target of targets) {
            fromUpdate.current.add(downloadKey(target));
            start(target);
          }
          notify(updateResultMessage(chosen, targets), undefined, targets.length === 0 ? { persist: true } : undefined);
        } catch (err) {
          fail(errorText(err));
        } finally {
          updatingRef.current = false;
          setUpdating(false);
        }
      })();
    },
    [start],
  );

  const shownVanished = updating ? NO_VANISHED : vanished;

  return useMemo(
    () => ({
      board,
      active: activeDownloads(board),
      start,
      clearFinished,
      reveal,
      vanished: shownVanished,
      updateVanished,
      dismissVanished,
    }),
    [board, start, clearFinished, reveal, shownVanished, updateVanished, dismissVanished],
  );
}

const NO_VANISHED: VanishedFile[] = [];
