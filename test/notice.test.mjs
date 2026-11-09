import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("notice duration grows with length and action, within a cap", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const n = await server.ssrLoadModule("/src/notice.ts");
    assert.equal(n.noticeDuration("素体を保存しました", false), 6000);
    assert.equal(n.noticeDuration("ファイルをライブラリに移しました", true), 12000);
    const long = "あ".repeat(n.NOTICE_FREE_CHARS + 25);
    assert.equal(n.noticeDuration(long, false), 6000 + 25 * n.NOTICE_PER_CHAR_MS);
    assert.equal(n.noticeDuration("あ".repeat(1000), false), 6000 + n.NOTICE_EXTRA_MAX_MS);
    assert.equal(n.noticeDuration("あ".repeat(1000), true), 12000 + n.NOTICE_EXTRA_MAX_MS);
    assert.ok(n.noticeDuration(long, true) > n.noticeDuration(long, false));
  } finally {
    await server.close();
  }
});

test("notice clock pauses on hover/focus/inactive and resumes with a floor", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const n = await server.ssrLoadModule("/src/notice.ts");
    let clock = n.startNoticeClock(6000, 0);
    assert.equal(n.noticeRemaining(clock, 1000), 5000);
    clock = n.holdNoticeClock(clock, "hover", 1000);
    assert.equal(n.noticeRemaining(clock, 60_000), 5000, "hover freezes the countdown");
    assert.equal(n.noticeExpired(clock, 999_999), false);
    clock = n.holdNoticeClock(clock, "focus", 2000);
    clock = n.releaseNoticeClock(clock, "hover", 3000);
    assert.equal(n.noticeExpired(clock, 999_999), false, "focus still holds after the mouse leaves");
    clock = n.releaseNoticeClock(clock, "focus", 4000);
    assert.equal(n.noticeRemaining(clock, 4000), 5000);
    assert.equal(n.noticeExpired(clock, 8999), false);
    assert.equal(n.noticeExpired(clock, 9000), true);

    let late = n.startNoticeClock(6000, 0);
    late = n.holdNoticeClock(late, "hover", 5900);
    late = n.releaseNoticeClock(late, "hover", 7000);
    assert.equal(n.noticeRemaining(late, 7000), n.NOTICE_RESUME_MIN_MS, "leaving leaves time to read");

    const hidden = n.startNoticeClock(6000, 0, ["inactive"]);
    assert.equal(n.noticeExpired(hidden, 100_000), false, "an inactive window does not count down");
    const back = n.releaseNoticeClock(hidden, "inactive", 100_000);
    assert.equal(n.noticeExpired(back, 105_999), false);
    assert.equal(n.noticeExpired(back, 106_000), true);

    assert.deepEqual(n.dismissIfCurrent({ id: 2 }, 1), { id: 2 }, "an old timer keeps a newer notice");
    assert.equal(n.dismissIfCurrent({ id: 2 }, 2), null);
    assert.equal(n.dismissIfCurrent(null, 2), null);
  } finally {
    await server.close();
  }
});

test("banners: notices auto-dismiss, errors stay until closed", async () => {
  const { parseHTML } = await import("linkedom");
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/");
  for (const name of ["window", "document", "location", "HTMLElement", "Element", "Node", "Event", "CustomEvent", "EventTarget", "FocusEvent", "MouseEvent"]) {
    if (window[name] !== undefined) Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;

  // 時計とタイマーを手で進める。
  let now = 1_000_000;
  let nextTimer = 1;
  const timers = new Map();
  const realNow = Date.now;
  Date.now = () => now;
  window.setTimeout = (fn, ms) => { const id = nextTimer++; timers.set(id, { at: now + Math.max(0, ms ?? 0), fn }); return id; };
  window.clearTimeout = (id) => { timers.delete(id); };
  let focused = true;
  let visibility = "visible";
  document.hasFocus = () => focused;
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });

  const { act, createElement, Fragment } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { useBanners } = await server.ssrLoadModule("/src/useBanners.ts");
    const { NoticeBanner, ErrorBanner } = await server.ssrLoadModule("/src/components/NoticeBanner.tsx");
    let shell;
    function Harness() {
      const b = useBanners();
      shell = b;
      return createElement(
        Fragment,
        null,
        b.error && createElement(ErrorBanner, { key: b.error.id, error: b.error, onDismiss: b.dismissError }),
        b.notice && createElement(NoticeBanner, { key: b.notice.id, notice: b.notice, onDismiss: b.dismissNotice, onAction: b.runNoticeAction }),
      );
    }
    const advance = async (ms) => {
      const until = now + ms;
      for (;;) {
        const due = [...timers.entries()].filter(([, t]) => t.at <= until).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        timers.delete(due[0]);
        now = due[1].at;
        await act(async () => { due[1].fn(); });
      }
      now = until;
    };
    const noticeEl = () => document.querySelector(".banner.notice");
    const errorEl = () => document.querySelector(".banner.error");
    const fire = async (el, type, init = {}) => {
      const event = new window.Event(type, { bubbles: true });
      Object.assign(event, init);
      await act(async () => { el.dispatchEvent(event); });
    };
    root = createRoot(document.getElementById("root"));
    await act(async () => { root.render(createElement(Harness)); });

    // 操作ボタンなしは約6秒で消える。role=status と ×（閉じる）がある。
    await act(async () => { shell.notify("素体を保存しました"); });
    assert.equal(noticeEl()?.getAttribute("role"), "status");
    assert.equal(noticeEl().querySelector(".banner-close").getAttribute("aria-label"), "閉じる");
    await advance(5999);
    assert.ok(noticeEl(), "still shown before 6s");
    await advance(1);
    assert.equal(noticeEl(), null, "gone after 6s");

    // 操作ボタン付きは約12秒。
    let opened = 0;
    await act(async () => { shell.notify("ファイルをライブラリに移しました", { label: "保存先を開く", run: () => opened++ }); });
    await advance(11_000);
    assert.ok(noticeEl());
    await advance(1000);
    assert.equal(noticeEl(), null);

    // 新しい通知でタイマーが数え直し、古いタイマーが新しい通知を消さない。
    await act(async () => { shell.notify("一つ目"); });
    await advance(5000);
    await act(async () => { shell.notify("二つ目"); });
    await advance(2000);
    assert.equal(noticeEl()?.textContent.includes("二つ目"), true, "the new notice survives the old deadline");
    await advance(4000);
    assert.equal(noticeEl(), null);

    // ホバー中は消えない。離れたら残りを数える（最低3秒）。
    await act(async () => { shell.notify("ホバーの確認"); });
    await advance(5500);
    await fire(noticeEl(), "mouseover", { relatedTarget: document.body });
    await advance(60_000);
    assert.ok(noticeEl(), "hover keeps the notice");
    await fire(noticeEl(), "mouseout", { relatedTarget: document.body });
    await advance(2999);
    assert.ok(noticeEl(), "leaving gives at least 3s");
    await advance(1);
    assert.equal(noticeEl(), null);

    // 通知内のボタンにフォーカスがある間は消えない。
    await act(async () => { shell.notify("フォーカスの確認", { label: "保存先を開く", run: () => opened++ }); });
    const actionButton = noticeEl().querySelector("button.button");
    await fire(actionButton, "focusin", { relatedTarget: null });
    await advance(60_000);
    assert.ok(noticeEl(), "focus keeps the notice");
    await fire(actionButton, "focusout", { relatedTarget: document.body });
    await advance(12_000);
    assert.equal(noticeEl(), null);

    // ウィンドウのフォーカスが外れただけ（内蔵ブラウザの操作中など）では止めない。
    await act(async () => { shell.notify("フォーカス外れの確認"); });
    focused = false;
    await act(async () => { window.dispatchEvent(new window.Event("blur")); });
    await advance(6000);
    assert.equal(noticeEl(), null, "a blurred window still dismisses the notice");
    await act(async () => { shell.notify("フォーカス外れのまま出た通知"); });
    await advance(6000);
    assert.equal(noticeEl(), null, "a notice shown while blurred also dismisses");
    focused = true;
    await act(async () => { window.dispatchEvent(new window.Event("focus")); });
    // 画面が隠れている間は数えない。
    await act(async () => { shell.notify("隠れる確認"); });
    await advance(1000);
    visibility = "hidden";
    await act(async () => { document.dispatchEvent(new window.Event("visibilitychange")); });
    await advance(60_000);
    assert.ok(noticeEl(), "a hidden window keeps the notice");
    visibility = "visible";
    await act(async () => { document.dispatchEvent(new window.Event("visibilitychange")); });
    await advance(4999);
    assert.ok(noticeEl(), "resumes with the remaining time");
    await advance(1);
    assert.equal(noticeEl(), null);
    visibility = "hidden";
    await act(async () => { shell.notify("非表示の確認"); });
    await advance(60_000);
    assert.ok(noticeEl(), "a notice shown while hidden waits");
    visibility = "visible";
    await act(async () => { document.dispatchEvent(new window.Event("visibilitychange")); });
    await advance(6000);
    assert.equal(noticeEl(), null);

    // 操作ボタンはすぐ消して実行する。×でも閉じる。
    await act(async () => { shell.notify("実行の確認", { label: "保存先を開く", run: () => opened++ }); });
    await fire(noticeEl().querySelector("button.button"), "click");
    assert.equal(opened, 1);
    assert.equal(noticeEl(), null);
    await act(async () => { shell.notify("閉じるの確認"); });
    await fire(noticeEl().querySelector(".banner-close"), "click");
    assert.equal(noticeEl(), null);

    // persist の通知は時間で消えない。
    await act(async () => { shell.notify("10 件を分類しました（2 件失敗）", undefined, { persist: true }); });
    await advance(10 * 60_000);
    assert.ok(noticeEl()?.getAttribute("data-notice-persist"), "persist notice stays");
    await fire(noticeEl().querySelector(".banner-close"), "click");
    assert.equal(noticeEl(), null);

    // 失敗の帯は時間で消えず、×（閉じる）で閉じる。同じ文言の失敗はまた出る。
    await act(async () => { shell.fail("保存できませんでした"); });
    await advance(10 * 60_000);
    assert.ok(errorEl());
    const close = errorEl().querySelector(".banner-close");
    assert.equal(close.getAttribute("aria-label"), "閉じる");
    assert.equal(close.textContent, "×");
    await fire(close, "click");
    assert.equal(errorEl(), null);
    await act(async () => { shell.fail("保存できませんでした"); });
    assert.ok(errorEl()?.textContent.includes("保存できませんでした"), "the same error shows again");
    const firstId = errorEl().getAttribute("data-error-id");
    await act(async () => { shell.notify("次の操作"); });
    assert.equal(errorEl(), null, "a new notice replaces the error as before");
    await act(async () => { shell.fail("保存できませんでした"); });
    assert.notEqual(errorEl().getAttribute("data-error-id"), firstId);
  } finally {
    if (root) await act(async () => root.unmount());
    Date.now = realNow;
    await server.close();
  }
});

test("failures that go through notify are marked to stay", () => {
  const importPage = readFileSync(new URL("../src/pages/ImportPage.tsx", import.meta.url), "utf8");
  assert.match(importPage, /\{ persist: result\.truncated \}/);
  const settings = readFileSync(new URL("../src/pages/SettingsPage.tsx", import.meta.url), "utf8");
  assert.match(settings, /\{ persist: report\.truncated \}/);
  const unity = readFileSync(new URL("../src/components/UnitySettings.tsx", import.meta.url), "utf8");
  assert.match(unity, /"Unity が見つかりませんでした", undefined, \{ persist: true \}/);
  const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");
  assert.match(css, /prefers-reduced-motion: reduce\)\s*\{\s*\*, \*::before, \*::after \{\s*animation: none !important;/);
});
