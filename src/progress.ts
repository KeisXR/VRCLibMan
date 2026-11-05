import type { JobProgress, JobProgressState } from "./types";

/** 終わった仕事を帯に残す時間（ミリ秒）。 */
export const PROGRESS_LINGER_MS = 2500;

export interface ProgressEntry {
  job: string;
  current: number;
  total: number;
  message: string;
  state: JobProgressState;
  background: boolean;
  /** 届いた順番。大きいほど新しい。 */
  seq: number;
  /** 終わりの状態が届いた時刻。走っている間は null。 */
  endedAt: number | null;
}

/** 仕事ごとの最新の状態。 */
export interface ProgressBoard {
  jobs: Record<string, ProgressEntry>;
  seq: number;
}

export interface ProgressStrip {
  entry: ProgressEntry;
  /** ほかに走っている前面の仕事の数。 */
  others: number;
}

export const EMPTY_PROGRESS: ProgressBoard = { jobs: {}, seq: 0 };

/** 状態を明示しない古い送り方は、`total > 0` かつ `current >= total` を終わりとみなす。 */
export function progressState(progress: JobProgress): JobProgressState {
  if (progress.state) return progress.state;
  return progress.total > 0 && progress.current >= progress.total ? "done" : "running";
}

export function isProgressEnded(entry: { state: JobProgressState }): boolean {
  return entry.state !== "running";
}

/** 届いた1件を反映する。同じ仕事は最新で置き換える。 */
export function applyProgress(board: ProgressBoard, progress: JobProgress, now: number): ProgressBoard {
  const state = progressState(progress);
  const seq = board.seq + 1;
  const entry: ProgressEntry = {
    job: progress.job,
    current: Math.max(0, progress.current),
    total: Math.max(0, progress.total),
    message: progress.message,
    state,
    background: progress.background === true,
    seq,
    endedAt: state === "running" ? null : now,
  };
  return { jobs: { ...board.jobs, [progress.job]: entry }, seq };
}

/** 終わってから `linger` 経った仕事を消す。変わらなければ同じ board を返す。 */
export function pruneProgress(board: ProgressBoard, now: number, linger = PROGRESS_LINGER_MS): ProgressBoard {
  let changed = false;
  const jobs: Record<string, ProgressEntry> = {};
  for (const [job, entry] of Object.entries(board.jobs)) {
    if (entry.endedAt !== null && now - entry.endedAt >= linger) {
      changed = true;
      continue;
    }
    jobs[job] = entry;
  }
  return changed ? { jobs, seq: board.seq } : board;
}

/** 次に `pruneProgress` を呼ぶまでの時間。消すものが無ければ null。 */
export function nextPruneDelay(board: ProgressBoard, now: number, linger = PROGRESS_LINGER_MS): number | null {
  let next: number | null = null;
  for (const entry of Object.values(board.jobs)) {
    if (entry.endedAt === null) continue;
    const wait = Math.max(0, entry.endedAt + linger - now);
    next = next === null ? wait : Math.min(next, wait);
  }
  return next;
}

/** 上部の帯。前面の仕事のうち最後に更新されたものと、ほかに走っている前面の仕事の数。 */
export function progressStrip(board: ProgressBoard): ProgressStrip | null {
  const foreground = Object.values(board.jobs).filter((entry) => !entry.background);
  if (foreground.length === 0) return null;
  const latest = foreground.reduce((a, b) => (b.seq > a.seq ? b : a));
  const others = foreground.filter((entry) => entry !== latest && !isProgressEnded(entry)).length;
  return { entry: latest, others };
}

/** 前面の仕事が1つでも走っているか（アプリ更新の再起動を待つ判定に使う）。 */
export function workInProgress(board: ProgressBoard): boolean {
  return Object.values(board.jobs).some((entry) => !entry.background && !isProgressEnded(entry));
}

/** 「ほか N 件」。0 件なら空文字。 */
export function othersLabel(count: number): string {
  return count > 0 ? `ほか ${count} 件` : "";
}

/** バーの幅（0〜100）。走っている total = 0 は不定なので null。done は 0 件でも満たし、失敗と中止の不定は 0 で止める。 */
export function progressPercent(current: number, total: number, state: JobProgressState = "running"): number | null {
  if (state === "done") return 100;
  if (total <= 0) return state === "running" ? null : 0;
  return Math.min(100, Math.max(0, (current / total) * 100));
}
