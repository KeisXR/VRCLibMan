import { useCallback, useEffect, useRef, type FocusEvent } from "react";
import {
  holdNoticeClock,
  noticeDuration,
  noticeExpired,
  noticeRemaining,
  releaseNoticeClock,
  startNoticeClock,
  type ErrorEntry,
  type NoticeClock,
  type NoticeEntry,
  type NoticeHold,
} from "../notice";

function windowInactive(): boolean {
  if (document.visibilityState === "hidden") return true;
  return typeof document.hasFocus === "function" && !document.hasFocus();
}

function CloseButton({ onClick }: { onClick: () => void }) {
  return (
    <button className="banner-close" type="button" aria-label="閉じる" title="閉じる" onClick={onClick}>
      <span aria-hidden="true">×</span>
    </button>
  );
}

/**
 * 情報通知。`persist` でなければ一定時間で消える。
 * マウスが上にある間、通知内のボタンにフォーカスがある間、ウィンドウが非アクティブな間は止まる。
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
  const { id, message, persist } = notice;
  const hasAction = notice.action !== undefined;

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
    clock.current = startNoticeClock(noticeDuration(message, hasAction), Date.now(), windowInactive() ? ["inactive"] : []);
    schedule();
    const onActivity = () => change("inactive", windowInactive());
    document.addEventListener("visibilitychange", onActivity);
    window.addEventListener("focus", onActivity);
    window.addEventListener("blur", onActivity);
    return () => {
      document.removeEventListener("visibilitychange", onActivity);
      window.removeEventListener("focus", onActivity);
      window.removeEventListener("blur", onActivity);
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = null;
      clock.current = null;
    };
    // 新しい通知（id が変わる）ごとに数え直す。
  }, [id, message, hasAction, persist, schedule, change]);

  const onBlur = (event: FocusEvent<HTMLDivElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) change("focus", false);
  };

  return (
    <div
      className="banner notice"
      role="status"
      data-notice-id={id}
      data-notice-persist={persist ? "true" : undefined}
      onMouseEnter={() => change("hover", true)}
      onMouseLeave={() => change("hover", false)}
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
