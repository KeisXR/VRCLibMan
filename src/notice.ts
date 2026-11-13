/**
 * 画面上部の情報通知（`.banner.notice`）を時間で消すための純関数。
 * 失敗の帯（`.banner.error`）と `persist` を付けた通知は時間では消さない。
 * マウスが一定時間乗り続けた通知も、消去を取り消して×で閉じるまで残す。
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
  /** 閉じる動きの途中。動きが終わったら取り除く。 */
  leaving?: boolean;
}

/** 失敗の帯。時間では消さない。 */
export interface ErrorEntry {
  id: number;
  message: string;
  /** 閉じる動きの途中。動きが終わったら取り除く。 */
  leaving?: boolean;
}

export interface NotifyOptions {
  /** true のときは時間で消さない（失敗件数や途中打ち切りを含む結果など）。 */
  persist?: boolean;
}

/** 通知を出しておく時間（ミリ秒）。文字数や操作ボタンの有無によらず同じ。 */
export const NOTICE_DURATION_MS = 3000;
/** マウスがこの時間乗り続けたら、時間で消すのをやめる。 */
export const NOTICE_PIN_HOVER_MS = 1000;
/** マウスやフォーカスが離れたあと、最低限残す時間。 */
export const NOTICE_RESUME_MIN_MS = 1500;

/** 通知を出しておく時間。 */
export function noticeDuration(): number {
  return NOTICE_DURATION_MS;
}

/** タイマーを止める理由。どれか1つでもあれば止まる。 */
export type NoticeHold = "hover" | "focus" | "inactive";

export interface NoticeClock {
  /** 消えるまでの残り時間（止まっている間は減らない）。 */
  remaining: number;
  /** 進んでいるときは進み始めた時刻、止まっているときは null。 */
  runningSince: number | null;
  holds: NoticeHold[];
  /** 消去を取り消した（マウスが乗り続けた）通知。時間では消えない。 */
  pinned: boolean;
}

export function startNoticeClock(duration: number, now: number, holds: NoticeHold[] = []): NoticeClock {
  const unique = [...new Set(holds)];
  return { remaining: duration, runningSince: unique.length ? null : now, holds: unique, pinned: false };
}

/** 今の残り時間。進んでいるときは経過分を引く。 */
export function noticeRemaining(clock: NoticeClock, now: number): number {
  if (clock.runningSince === null) return clock.remaining;
  return Math.max(0, clock.remaining - Math.max(0, now - clock.runningSince));
}

/** 止める理由を足す。最初の理由で残り時間を固定する。 */
export function holdNoticeClock(clock: NoticeClock, hold: NoticeHold, now: number): NoticeClock {
  if (clock.pinned || clock.holds.includes(hold)) return clock;
  return { ...clock, remaining: noticeRemaining(clock, now), runningSince: null, holds: [...clock.holds, hold] };
}

/**
 * 止める理由を外す。理由がなくなったら再開する。
 * マウスやフォーカスが離れた直後に消えないよう、残りは最低 NOTICE_RESUME_MIN_MS にする。
 */
export function releaseNoticeClock(clock: NoticeClock, hold: NoticeHold, now: number): NoticeClock {
  if (clock.pinned || !clock.holds.includes(hold)) return clock;
  const holds = clock.holds.filter((item) => item !== hold);
  if (holds.length) return { ...clock, holds };
  return { ...clock, remaining: Math.max(clock.remaining, NOTICE_RESUME_MIN_MS), runningSince: now, holds };
}

/** 消去を取り消す。以後は止める理由が外れても再開しない。 */
export function pinNoticeClock(clock: NoticeClock, now: number): NoticeClock {
  if (clock.pinned) return clock;
  return { ...clock, remaining: noticeRemaining(clock, now), runningSince: null, pinned: true };
}

/** 消えるまでのシークバー。`from` は通知を出しておく時間に対する残りの割合、`ms` はそこから 0 までの時間。 */
export interface NoticeTimer {
  from: number;
  ms: number;
  running: boolean;
}

/** シークバーの今の状態。消去を取り消した通知には出さない。 */
export function noticeTimer(clock: NoticeClock, now: number, duration: number = NOTICE_DURATION_MS): NoticeTimer | null {
  if (clock.pinned || duration <= 0) return null;
  const ms = noticeRemaining(clock, now);
  return { from: Math.min(1, Math.max(0, ms / duration)), ms, running: clock.runningSince !== null };
}

/** 時間切れか。止まっている間は消さない。 */
export function noticeExpired(clock: NoticeClock, now: number): boolean {
  return !clock.pinned && clock.runningSince !== null && noticeRemaining(clock, now) <= 0;
}

/** 連番付きの通知。古いタイマーが新しい通知を消さないよう、閉じるときは番号で照合する。 */
export interface Numbered {
  id: number;
}

export function dismissIfCurrent<T extends Numbered>(current: T | null, id: number): T | null {
  return current && current.id === id ? null : current;
}

/** 閉じる動きを始める。番号が違う・既に閉じ始めているときはそのまま。 */
export function leaveIfCurrent<T extends Numbered & { leaving?: boolean }>(current: T | null, id: number): T | null {
  return current && current.id === id && !current.leaving ? { ...current, leaving: true } : current;
}
