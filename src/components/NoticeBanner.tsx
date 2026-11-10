import { useCallback, useEffect, useRef, useState, type FocusEvent } from "react";
import {
  NOTICE_PIN_HOVER_MS,
  holdNoticeClock,
  noticeDuration,
  noticeExpired,
  noticeRemaining,
  pinNoticeClock,
  releaseNoticeClock,
  startNoticeClock,
  type ErrorEntry,
  type NoticeClock,
  type NoticeEntry,
  type NoticeHold,
} from "../notice";

/** 画面が隠れている（最小化や別タブなど）ときだけ止める。フォーカスが外れただけでは止めない。 */
function windowInactive(): boolean {
  return document.visibilityState === "hidden";
}

function CloseButton({ onClick }: { onClick: () => void }) {
  return (
    <button className="banner-close" type="button" aria-label="閉じる" title="閉じる" onClick={onClick}>
      <span aria-hidden="true">×</span>
    </button>
  );
}

/**
 * 情報通知。`persist` でなければ3秒で消える。
 * マウスが上にある間、通知内のボタンにフォーカスがある間、画面が隠れている間は止まる。
 * マウスが NOTICE_PIN_HOVER_MS 乗り続けたら消去を取り消し、×で閉じるまで残す。
 */
export function NoticeBanner({
  notice,
  onDismiss,
  onAction,
}: {
  notice: NoticeEntry;
  onDismiss: (id: number) => void;
  onAction: (notice: NoticeEntry) => void;
}) {
  const clock = useRef<NoticeClock | null>(null);
  const timer = useRef<number | null>(null);
  const pinTimer = useRef<number | null>(null);
  const [pinned, setPinned] = useState(false);
  const { id, message, persist } = notice;

  const clearPinTimer = useCallback(() => {
    if (pinTimer.current !== null) window.clearTimeout(pinTimer.current);
    pinTimer.current = null;
  }, []);

  const schedule = useCallback(() => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
    const current = clock.current;
    if (!current || current.runningSince === null) return;
    timer.current = window.setTimeout(() => {
      timer.current = null;
      if (clock.current && noticeExpired(clock.current, Date.now())) onDismiss(id);
      else schedule();
    }, Math.max(0, noticeRemaining(current, Date.now())));
  }, [id, onDismiss]);

  const change = useCallback(
    (hold: NoticeHold, on: boolean) => {
      const current = clock.current;
      if (!current) return;
      const now = Date.now();
      clock.current = on ? holdNoticeClock(current, hold, now) : releaseNoticeClock(current, hold, now);
      if (clock.current !== current) schedule();
    },
    [schedule],
  );

  useEffect(() => {
    if (persist) {
      clock.current = null;
      return;
    }
    clock.current = startNoticeClock(noticeDuration(), Date.now(), windowInactive() ? ["inactive"] : []);
    schedule();
    const onActivity = () => change("inactive", windowInactive());
    document.addEventListener("visibilitychange", onActivity);
    return () => {
      document.removeEventListener("visibilitychange", onActivity);
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = null;
      clearPinTimer();
      clock.current = null;
    };
    // 新しい通知（id が変わる）ごとに数え直す。
  }, [id, persist, schedule, change, clearPinTimer]);

  const onMouseEnter = () => {
    change("hover", true);
    if (!clock.current || clock.current.pinned || pinTimer.current !== null) return;
    pinTimer.current = window.setTimeout(() => {
      pinTimer.current = null;
      if (!clock.current) return;
      clock.current = pinNoticeClock(clock.current, Date.now());
      schedule();
      setPinned(true);
    }, NOTICE_PIN_HOVER_MS);
  };

  const onMouseLeave = () => {
    clearPinTimer();
    change("hover", false);
  };

  const onBlur = (event: FocusEvent<HTMLDivElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) change("focus", false);
  };

  return (
    <div
      className="banner notice"
      role="status"
      data-notice-id={id}
      data-notice-persist={persist ? "true" : undefined}
      data-notice-pinned={pinned ? "true" : undefined}
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      onFocus={() => change("focus", true)}
      onBlur={onBlur}
    >
      <span className="banner-message">{message}</span>
      {notice.action && (
        <button className="button" type="button" onClick={() => onAction(notice)}>
          {notice.action.label}
        </button>
      )}
      <CloseButton onClick={() => onDismiss(id)} />
    </div>
  );
}

/** 失敗の帯。時間では消さず、×で閉じるまで残す。 */
export function ErrorBanner({ error, onDismiss }: { error: ErrorEntry; onDismiss: (id: number) => void }) {
  return (
    <div className="banner error" data-error-id={error.id}>
      <span className="banner-message">{error.message}</span>
      <CloseButton onClick={() => onDismiss(error.id)} />
    </div>
  );
}
