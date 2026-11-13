import { useCallback, useRef, useState } from "react";
import { dismissIfCurrent, leaveIfCurrent, type ErrorEntry, type NoticeAction, type NoticeEntry, type NotifyOptions } from "./notice";

/** 関数だけを渡したときは、これまでどおり「元に戻す」として出す。 */
export function noticeAction(action?: NoticeAction | (() => void)): NoticeAction | undefined {
  if (!action) return undefined;
  if (typeof action === "function") return { label: "元に戻す", run: action };
  return action;
}

/**
 * 帯を閉じるときに動かすか。OS の「動きを減らす」と、matchMedia の無い環境（テスト）ではすぐ取り除く。
 * CSS 側も prefers-reduced-motion で animation を止めるので、animationend を待たない。
 */
function bannerMotion(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * 画面上部の情報通知と失敗の帯の状態。どちらも出すたびに連番を振り、
 * 閉じるときは番号で照合するので、古いタイマーや古い×が新しい表示を消さない。
 * 閉じる（時間切れ・×・操作ボタン）と `leaving` を付けて閉じる動きを見せ、終わったら `remove*` で取り除く。
 * 別の帯に置き換わるときは、二つが重なって動かないよう、これまでどおりすぐ入れ替える。
 */
export function useBanners() {
  const [notice, setNotice] = useState<NoticeEntry | null>(null);
  const [error, setErrorEntry] = useState<ErrorEntry | null>(null);
  const serial = useRef(0);
  const setError = useCallback((message: string | null) => {
    setErrorEntry(message === null ? null : { id: ++serial.current, message });
  }, []);
  const notify = useCallback((message: string, action?: NoticeAction | (() => void), options?: NotifyOptions) => {
    setErrorEntry(null);
    setNotice({ id: ++serial.current, message, action: noticeAction(action), persist: options?.persist === true });
  }, []);
  const fail = useCallback((message: string) => {
    setNotice(null);
    setErrorEntry({ id: ++serial.current, message });
  }, []);
  const dismissNotice = useCallback(
    (id: number) => setNotice((current) => (bannerMotion() ? leaveIfCurrent(current, id) : dismissIfCurrent(current, id))),
    [],
  );
  const dismissError = useCallback(
    (id: number) => setErrorEntry((current) => (bannerMotion() ? leaveIfCurrent(current, id) : dismissIfCurrent(current, id))),
    [],
  );
  const removeNotice = useCallback((id: number) => setNotice((current) => dismissIfCurrent(current, id)), []);
  const removeError = useCallback((id: number) => setErrorEntry((current) => dismissIfCurrent(current, id)), []);
  const runNoticeAction = useCallback(
    (entry: NoticeEntry) => {
      dismissNotice(entry.id);
      entry.action?.run();
    },
    [dismissNotice],
  );
  return { notice, error, notify, fail, setError, dismissNotice, dismissError, removeNotice, removeError, runNoticeAction };
}
