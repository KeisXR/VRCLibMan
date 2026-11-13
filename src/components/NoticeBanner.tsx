import { useCallback, useEffect, useLayoutEffect, useRef, useState, type AnimationEvent, type FocusEvent } from "react";
import {
  BANNER_LEAVE_FALLBACK_MS,
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
 * 閉じる動き（`banner-out`）。今の上の余白を `--banner-top`、帯の高さを `--banner-height` に置き、
 * 帯が薄れたあと上の余白を負にして下の内容を滑らかに詰める。
 * 動きが終わったら取り除く。animationend が来ないとき（画面が隠れているなど）も上限の時間で取り除く。
 */
function useBannerLeave(id: number, leaving: boolean | undefined, onRemoved: ((id: number) => void) | undefined) {
  const ref = useRef<HTMLDivElement>(null);
  const done = useRef(false);
  const finish = useCallback(() => {
    if (done.current) return;
    done.current = true;
    onRemoved?.(id);
  }, [id, onRemoved]);
  useLayoutEffect(() => {
    if (!leaving) return;
    const el = ref.current;
    if (el) {
      // data-leaving が付いた時点で banner-out が効いているので、測る間だけ外して元の余白を読む。
      el.style.animation = "none";
      el.style.setProperty("--banner-top", window.getComputedStyle?.(el).marginTop || "0px");
      el.style.setProperty("--banner-height", `${el.offsetHeight ?? 0}px`);
      el.style.animation = "";
    }
    const fallback = window.setTimeout(finish, BANNER_LEAVE_FALLBACK_MS);
    return () => window.clearTimeout(fallback);
  }, [leaving, finish]);
  const onAnimationEnd = (event: AnimationEvent<HTMLDivElement>) => {
    if (leaving && event.target === event.currentTarget && event.animationName === "banner-out") finish();
  };
  return { ref, onAnimationEnd, leavingAttr: leaving ? "true" : undefined };
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
  onRemoved,
}: {
  notice: NoticeEntry;
  onDismiss: (id: number) => void;
  onAction: (notice: NoticeEntry) => void;
  /** 閉じる動きが終わったときに呼ぶ。 */
  onRemoved?: (id: number) => void;
}) {
  const clock = useRef<NoticeClock | null>(null);
  const timer = useRef<number | null>(null);
  const pinTimer = useRef<number | null>(null);
  const [pinned, setPinned] = useState(false);
  const { id, message, persist } = notice;
  const leave = useBannerLeave(id, notice.leaving, onRemoved);

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
      ref={leave.ref}
      className="banner notice"
      role="status"
      data-notice-id={id}
      data-leaving={leave.leavingAttr}
      onAnimationEnd={leave.onAnimationEnd}
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
export function ErrorBanner({
  error,
  onDismiss,
  onRemoved,
}: {
  error: ErrorEntry;
  onDismiss: (id: number) => void;
  /** 閉じる動きが終わったときに呼ぶ。 */
  onRemoved?: (id: number) => void;
}) {
  const leave = useBannerLeave(error.id, error.leaving, onRemoved);
  return (
    <div
      ref={leave.ref}
      className="banner error"
      data-error-id={error.id}
      data-leaving={leave.leavingAttr}
      onAnimationEnd={leave.onAnimationEnd}
    >
      <span className="banner-message">{error.message}</span>
      <CloseButton onClick={() => onDismiss(error.id)} />
    </div>
  );
}
