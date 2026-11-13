import type { BoothFileStatus, Settings } from "./types";
import { formatBytes } from "./media";

/**
 * 商品カードの「ダウンロード」を始める前の確認。足りない設定があれば、その案内を返す。
 * 判定の正本は Rust（`download_booth_file` も同じ条件で断る）。ここは、始めてから失敗させて
 * サイドバーに失敗を残す前に、設定へ案内するためだけに見る。
 */
export function downloadBlocker(settings: Pick<Settings, "builtinBrowser" | "routeDownloads" | "libraryFolder">): string | null {
  if (!settings.builtinBrowser) {
    return "商品カードからのダウンロードには内蔵ブラウザを使います。設定の「BOOTH の読み込み方」で内蔵ブラウザを選んでください";
  }
  if (!settings.libraryFolder.trim()) {
    return "ダウンロードするには、ライブラリのフォルダを選んで「ダウンロードを自動整理」をオンにしてください";
  }
  if (!settings.routeDownloads) {
    return "ダウンロードするには、設定の「ダウンロードを自動整理」をオンにしてください";
  }
  return null;
}

/**
 * 内蔵ブラウザで受け取るファイルの一覧（サイドバーの「ダウンロード」）。
 * Rust の `booth-file-download` と、画面から呼んだ結果の両方で更新する。決まりは docs/design/README.md#ダウンロード。
 */
export type DownloadPhase = "starting" | "queued" | "receiving" | "moving" | "done" | "error";

export interface DownloadEntry {
  key: string;
  boothId: string;
  variation: string;
  filename: string;
  phase: DownloadPhase;
  message: string;
  received: number | null;
  total: number | null;
  /** 完了時だけ、ライブラリへ移した先。 */
  path: string | null;
  /** 押した順番。大きいほど新しい。 */
  seq: number;
}

export interface DownloadBoard {
  entries: DownloadEntry[];
  seq: number;
}

export interface DownloadTarget {
  boothId: string;
  variation: string;
  filename: string;
}

export const EMPTY_DOWNLOADS: DownloadBoard = { entries: [], seq: 0 };

/** 終わった項目を残す上限。古いものから落とす。 */
export const KEPT_FINISHED = 30;

export function downloadKey(target: DownloadTarget): string {
  return [target.boothId.trim(), target.variation, target.filename].join("\u001f");
}

export function isDownloadActive(entry: { phase: DownloadPhase }): boolean {
  return entry.phase !== "done" && entry.phase !== "error";
}

function trim(entries: DownloadEntry[]): DownloadEntry[] {
  let finished = 0;
  return entries.filter((entry) => isDownloadActive(entry) || ++finished <= KEPT_FINISHED);
}

function put(board: DownloadBoard, entry: Omit<DownloadEntry, "seq">, fresh: boolean): DownloadBoard {
  const seq = board.seq + 1;
  const previous = board.entries.find((item) => item.key === entry.key);
  const next: DownloadEntry = { ...entry, seq: fresh || !previous ? seq : previous.seq };
  const rest = board.entries.filter((item) => item.key !== entry.key);
  const entries = [...rest, next].sort((a, b) => b.seq - a.seq);
  return { entries: trim(entries), seq };
}

/** 押した直後。同じファイルの前の結果は置き換えて先頭に出す。 */
export function startDownload(board: DownloadBoard, target: DownloadTarget): DownloadBoard {
  return put(
    board,
    {
      key: downloadKey(target),
      boothId: target.boothId,
      variation: target.variation,
      filename: target.filename,
      phase: "starting",
      message: "ダウンロード中",
      received: null,
      total: null,
      path: null,
    },
    true,
  );
}

/** Rust からの状態。終わった項目に、遅れて届いた途中の状態は反映しない。 */
export function applyFileStatus(board: DownloadBoard, status: BoothFileStatus): DownloadBoard {
  const key = downloadKey(status);
  const previous = board.entries.find((item) => item.key === key);
  const phase: DownloadPhase = status.phase;
  if (previous && !isDownloadActive(previous) && isDownloadActive({ phase })) return board;
  return put(
    board,
    {
      key,
      boothId: status.boothId,
      variation: status.variation,
      filename: status.filename,
      phase,
      message: status.message,
      received: status.received ?? (phase === "receiving" ? previous?.received ?? null : null),
      total: status.total ?? (phase === "receiving" ? previous?.total ?? null : null),
      path: status.path ?? null,
    },
    false,
  );
}

/** 画面から呼んだ結果。イベントで先に終わっていれば、その内容を残す。 */
export function finishDownload(
  board: DownloadBoard,
  target: DownloadTarget,
  result: { ok: true; path: string | null } | { ok: false; message: string },
): DownloadBoard {
  const key = downloadKey(target);
  const previous = board.entries.find((item) => item.key === key);
  if (previous && !isDownloadActive(previous)) return board;
  return put(
    board,
    {
      key,
      boothId: target.boothId,
      variation: target.variation,
      filename: target.filename,
      phase: result.ok ? "done" : "error",
      message: result.ok ? "ライブラリに移しました" : result.message,
      received: null,
      total: null,
      path: result.ok ? result.path : null,
    },
    false,
  );
}

export function clearFinishedDownloads(board: DownloadBoard): DownloadBoard {
  const entries = board.entries.filter(isDownloadActive);
  return entries.length === board.entries.length ? board : { entries, seq: board.seq };
}

export function activeDownloads(board: DownloadBoard): number {
  return board.entries.filter(isDownloadActive).length;
}

/** バーの幅（0〜100）。大きさが分からない受け取りと準備中は null（不定）。 */
export function downloadPercent(entry: DownloadEntry): number | null {
  if (entry.phase === "done") return 100;
  if (entry.phase !== "receiving" || !entry.total || entry.received === null) return null;
  return Math.min(100, Math.max(0, (entry.received / entry.total) * 100));
}

/** 一覧と行に出す状態の文言。 */
export function downloadDetail(entry: DownloadEntry): string {
  if (entry.phase === "queued") return "順番待ち";
  if (entry.phase === "receiving" && entry.received !== null && entry.received > 0) {
    if (entry.total) {
      const percent = Math.floor(downloadPercent(entry) ?? 0);
      return `${formatBytes(entry.received)} / ${formatBytes(entry.total)}（${percent}%）`;
    }
    return `${formatBytes(entry.received)} 受信`;
  }
  return entry.message;
}

/** サイドバーのボタンの文言。 */
export function downloadsLabel(board: DownloadBoard): string {
  const active = activeDownloads(board);
  if (active > 0) return `ダウンロード中 ${active}件`;
  const failed = board.entries.filter((entry) => entry.phase === "error").length;
  if (failed > 0) return `ダウンロード ${failed}件失敗`;
  return `ダウンロード ${board.entries.length}件完了`;
}

/** サイドバーのボタンの下に出す、受け取り中の合計。大きさが一つでも分からなければ null（不定）。 */
export function overallDownload(board: DownloadBoard): { current: number; total: number } {
  const active = board.entries.filter(isDownloadActive);
  if (active.length === 0 || active.some((entry) => !entry.total)) return { current: 0, total: 0 };
  return {
    current: active.reduce((sum, entry) => sum + (entry.received ?? 0), 0),
    total: active.reduce((sum, entry) => sum + (entry.total ?? 0), 0),
  };
}
