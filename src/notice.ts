/**
 * 画面上部の情報通知（`.banner.notice`）を時間で消すための純関数。
 * 失敗の帯（`.banner.error`）と `persist` を付けた通知は時間では消さない。
 */

export interface NoticeAction {
  label: string;
  run: () => void;
}

/** 画面上部の情報通知。`id` は出すたびに増える連番。 */
export interface NoticeEntry {
  id: number;
  message: string;
  action?: NoticeAction;
  /** 失敗や注意を含む通知。時間では消さず、×で閉じるまで残す。 */
  persist?: boolean;
}

/** 失敗の帯。時間では消さない。 */
export interface ErrorEntry {
  id: number;
  message: string;
}

export interface NotifyOptions {
  /** true のときは時間で消さない（失敗件数や途中打ち切りを含む結果など）。 */
  persist?: boolean;
}

/** 操作ボタンなしの通知を出しておく基準の長さ（ミリ秒）。 */
export const NOTICE_BASE_MS = 6000;
/** 「保存先を開く」などの操作ボタン付きの通知の基準の長さ。 */
export const NOTICE_ACTION_MS = 12000;
/** この文字数までは基準の長さだけで読める扱いにする。 */
export const NOTICE_FREE_CHARS = 20;
/** 超えた1文字ごとに足す時間。 */
export const NOTICE_PER_CHAR_MS = 80;
/** 文字数による加算の上限。 */
export const NOTICE_EXTRA_MAX_MS = 10000;
/** マウスやフォーカスが離れたあと、最低限残す時間。 */
export const NOTICE_RESUME_MIN_MS = 3000;

/** 通知を出しておく時間。文字数が多いほど長く、上限あり。 */
export function noticeDuration(message: string, hasAction: boolean): number {
  const base = hasAction ? NOTICE_ACTION_MS : NOTICE_BASE_MS;
  const length = Array.from(message.trim()).length;
  const extra = Math.min(NOTICE_EXTRA_MAX_MS, Math.max(0, length - NOTICE_FREE_CHARS) * NOTICE_PER_CHAR_MS);
  return base + extra;
}

/** タイマーを止める理由。どれか1つでもあれば止まる。 */
export type NoticeHold = "hover" | "focus" | "inactive";

export interface NoticeClock {
  /** 消えるまでの残り時間（止まっている間は減らない）。 */
  remaining: number;
  /** 進んでいるときは進み始めた時刻、止まっているときは null。 */
  runningSince: number | null;
  holds: NoticeHold[];
}

export function startNoticeClock(duration: number, now: number, holds: NoticeHold[] = []): NoticeClock {
  const unique = [...new Set(holds)];
  return { remaining: duration, runningSince: unique.length ? null : now, holds: unique };
}

/** 今の残り時間。進んでいるときは経過分を引く。 */
export function noticeRemaining(clock: NoticeClock, now: number): number {
  if (clock.runningSince === null) return clock.remaining;
  return Math.max(0, clock.remaining - Math.max(0, now - clock.runningSince));
}

/** 止める理由を足す。最初の理由で残り時間を固定する。 */
export function holdNoticeClock(clock: NoticeClock, hold: NoticeHold, now: number): NoticeClock {
  if (clock.holds.includes(hold)) return clock;
  return { remaining: noticeRemaining(clock, now), runningSince: null, holds: [...clock.holds, hold] };
}

/**
 * 止める理由を外す。理由がなくなったら再開する。
 * マウスやフォーカスが離れた直後に消えないよう、残りは最低 NOTICE_RESUME_MIN_MS にする。
 */
export function releaseNoticeClock(clock: NoticeClock, hold: NoticeHold, now: number): NoticeClock {
  if (!clock.holds.includes(hold)) return clock;
  const holds = clock.holds.filter((item) => item !== hold);
  if (holds.length) return { ...clock, holds };
  return { remaining: Math.max(clock.remaining, NOTICE_RESUME_MIN_MS), runningSince: now, holds };
}

/** 時間切れか。止まっている間は消さない。 */
export function noticeExpired(clock: NoticeClock, now: number): boolean {
  return clock.runningSince !== null && noticeRemaining(clock, now) <= 0;
}

/** 連番付きの通知。古いタイマーが新しい通知を消さないよう、閉じるときは番号で照合する。 */
export interface Numbered {
  id: number;
}

export function dismissIfCurrent<T extends Numbered>(current: T | null, id: number): T | null {
  return current && current.id === id ? null : current;
}
