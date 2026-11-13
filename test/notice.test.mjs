import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createServer } from "vite";

test("notices last 3 seconds regardless of length or action", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const n = await server.ssrLoadModule("/src/notice.ts");
    assert.equal(n.NOTICE_DURATION_MS, 3000);
    assert.equal(n.noticeDuration(), 3000);
  } finally {
    await server.close();
  }
});

test("notice clock pauses on hover/focus/inactive, resumes with a floor, and can be pinned", async () => {
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  try {
    const n = await server.ssrLoadModule("/src/notice.ts");
    let clock = n.startNoticeClock(3000, 0);
    assert.equal(n.noticeRemaining(clock, 1000), 2000);
    clock = n.holdNoticeClock(clock, "hover", 1000);
    assert.equal(n.noticeRemaining(clock, 60_000), 2000, "hover freezes the countdown");
    assert.equal(n.noticeExpired(clock, 999_999), false);
    clock = n.holdNoticeClock(clock, "focus", 2000);
    clock = n.releaseNoticeClock(clock, "hover", 3000);
    assert.equal(n.noticeExpired(clock, 999_999), false, "focus still holds after the mouse leaves");
    clock = n.releaseNoticeClock(clock, "focus", 4000);
    assert.equal(n.noticeRemaining(clock, 4000), 2000);
    assert.equal(n.noticeExpired(clock, 5999), false);
    assert.equal(n.noticeExpired(clock, 6000), true);

    let late = n.startNoticeClock(3000, 0);
    late = n.holdNoticeClock(late, "hover", 2900);
    late = n.releaseNoticeClock(late, "hover", 3500);
    assert.equal(n.noticeRemaining(late, 3500), n.NOTICE_RESUME_MIN_MS, "leaving leaves time to read");

    const hidden = n.startNoticeClock(3000, 0, ["inactive"]);
    assert.equal(n.noticeExpired(hidden, 100_000), false, "an inactive window does not count down");
    const back = n.releaseNoticeClock(hidden, "inactive", 100_000);
    assert.equal(n.noticeExpired(back, 102_999), false);
    assert.equal(n.noticeExpired(back, 103_000), true);

    let pinned = n.startNoticeClock(3000, 0);
    pinned = n.holdNoticeClock(pinned, "hover", 500);
    pinned = n.pinNoticeClock(pinned, 1500);
    pinned = n.releaseNoticeClock(pinned, "hover", 2000);
    assert.equal(pinned.pinned, true);
    assert.equal(n.noticeExpired(pinned, 999_999), false, "a pinned notice never expires");
    pinned = n.holdNoticeClock(pinned, "focus", 3000);
    pinned = n.releaseNoticeClock(pinned, "focus", 4000);
    assert.equal(n.noticeExpired(pinned, 999_999), false, "releasing other holds keeps it pinned");

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

    // 3秒で消える。role=status と ×（閉じる）がある。
    await act(async () => { shell.notify("素体を保存しました"); });
    assert.equal(noticeEl()?.getAttribute("role"), "status");
    assert.equal(noticeEl().querySelector(".banner-close").getAttribute("aria-label"), "閉じる");
    await advance(2999);
    assert.ok(noticeEl(), "still shown before 3s");
    await advance(1);
    assert.equal(noticeEl(), null, "gone after 3s");

    // 操作ボタン付きや長い文も3秒。
    let opened = 0;
    await act(async () => { shell.notify("ファイルをライブラリに移しました", { label: "保存先を開く", run: () => opened++ }); });
    await advance(2999);
    assert.ok(noticeEl());
    await advance(1);
    assert.equal(noticeEl(), null);
    await act(async () => { shell.notify("あ".repeat(200)); });
    await advance(3000);
    assert.equal(noticeEl(), null);

    // 新しい通知でタイマーが数え直し、古いタイマーが新しい通知を消さない。
    await act(async () => { shell.notify("一つ目"); });
    await advance(2500);
    await act(async () => { shell.notify("二つ目"); });
    await advance(1000);
    assert.equal(noticeEl()?.textContent.includes("二つ目"), true, "the new notice survives the old deadline");
    await advance(2000);
    assert.equal(noticeEl(), null);

    // 1秒未満のホバーは止めるだけ。離れたら残りを数える（最低1.5秒）。
    await act(async () => { shell.notify("ホバーの確認"); });
    await advance(2500);
    await fire(noticeEl(), "mouseover", { relatedTarget: document.body });
    await advance(999);
    assert.ok(noticeEl(), "hover keeps the notice");
    assert.equal(noticeEl().getAttribute("data-notice-pinned"), null, "a short hover does not pin");
    await fire(noticeEl(), "mouseout", { relatedTarget: document.body });
    await advance(1499);
    assert.ok(noticeEl(), "leaving gives at least 1.5s");
    await advance(1);
    assert.equal(noticeEl(), null);

    // 1秒乗り続けたら消去を取り消し、離れても×で閉じるまで残す。
    await act(async () => { shell.notify("取り消しの確認"); });
    await advance(2000);
    await fire(noticeEl(), "mouseover", { relatedTarget: document.body });
    await advance(1000);
    assert.equal(noticeEl()?.getAttribute("data-notice-pinned"), "true", "a 1s hover pins the notice");
    await fire(noticeEl(), "mouseout", { relatedTarget: document.body });
    await advance(10 * 60_000);
    assert.ok(noticeEl(), "a pinned notice stays after the mouse leaves");
    await fire(noticeEl().querySelector(".banner-close"), "click");
    assert.equal(noticeEl(), null);

    // 乗り続けている途中で新しい通知に替わったら、新しい通知は取り消し扱いにしない。
    await act(async () => { shell.notify("替わる前"); });
    await fire(noticeEl(), "mouseover", { relatedTarget: document.body });
    await advance(500);
    await act(async () => { shell.notify("替わった後"); });
    await advance(3000);
    assert.equal(noticeEl(), null, "the replaced notice's hover timer does not pin the new one");

    // 通知内のボタンにフォーカスがある間は消えない。
    await act(async () => { shell.notify("フォーカスの確認", { label: "保存先を開く", run: () => opened++ }); });
    const actionButton = noticeEl().querySelector("button.button");
    await fire(actionButton, "focusin", { relatedTarget: null });
    await advance(60_000);
    assert.ok(noticeEl(), "focus keeps the notice");
    await fire(actionButton, "focusout", { relatedTarget: document.body });
    await advance(3000);
    assert.equal(noticeEl(), null);

    // ウィンドウのフォーカスが外れただけ（内蔵ブラウザの操作中など）では止めない。
    await act(async () => { shell.notify("フォーカス外れの確認"); });
    focused = false;
    await act(async () => { window.dispatchEvent(new window.Event("blur")); });
    await advance(3000);
    assert.equal(noticeEl(), null, "a blurred window still dismisses the notice");
    await act(async () => { shell.notify("フォーカス外れのまま出た通知"); });
    await advance(3000);
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
    await advance(1999);
    assert.ok(noticeEl(), "resumes with the remaining time");
    await advance(1);
    assert.equal(noticeEl(), null);
    visibility = "hidden";
    await act(async () => { shell.notify("非表示の確認"); });
    await advance(60_000);
    assert.ok(noticeEl(), "a notice shown while hidden waits");
    visibility = "visible";
    await act(async () => { document.dispatchEvent(new window.Event("visibilitychange")); });
    await advance(3000);
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

test("banners slide out before they are removed, unless motion is reduced", async () => {
  const { parseHTML } = await import("linkedom");
  const { window } = parseHTML('<html><body><div id="root"></div></body></html>');
  window.location = new URL("http://localhost:1420/");
  for (const name of ["window", "document", "location", "HTMLElement", "Element", "Node", "Event", "CustomEvent", "EventTarget", "FocusEvent", "MouseEvent"]) {
    if (window[name] !== undefined) Object.defineProperty(globalThis, name, { configurable: true, writable: true, value: window[name] });
  }
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: "test" } });
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  let now = 1_000_000;
  let nextTimer = 1;
  const timers = new Map();
  const realNow = Date.now;
  Date.now = () => now;
  window.setTimeout = (fn, ms) => { const id = nextTimer++; timers.set(id, { at: now + Math.max(0, ms ?? 0), fn }); return id; };
  window.clearTimeout = (id) => { timers.delete(id); };
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
  let reduced = false;
  window.matchMedia = (query) => ({ matches: query.includes("reduce") && reduced, media: query });

  const { act, createElement, Fragment } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const server = await createServer({ server: { middlewareMode: true, hmr: false }, appType: "custom" });
  let root;
  try {
    const { useBanners } = await server.ssrLoadModule("/src/useBanners.ts");
    const { NoticeBanner, ErrorBanner } = await server.ssrLoadModule("/src/components/NoticeBanner.tsx");
    const { TOPBAR_LEAVE_FALLBACK_MS: BANNER_LEAVE_FALLBACK_MS } = await server.ssrLoadModule("/src/topbarMotion.ts");
    let shell;
    function Harness() {
      const b = useBanners();
      shell = b;
      return createElement(
        Fragment,
        null,
        b.error && createElement(ErrorBanner, { key: b.error.id, error: b.error, onDismiss: b.dismissError, onRemoved: b.removeError }),
        b.notice && createElement(NoticeBanner, { key: b.notice.id, notice: b.notice, onDismiss: b.dismissNotice, onAction: b.runNoticeAction, onRemoved: b.removeNotice }),
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

    // 時間切れでは閉じる動きに入り、animationend で取り除く。
    await act(async () => { shell.notify("保存しました"); });
    assert.equal(noticeEl().getAttribute("data-leaving"), null);
    assert.equal(noticeEl().getAttribute("data-topbar-motion"), "", "the notice uses the shared topbar motion");
    // シークバーは残り 100% から 3 秒かけて縮み、ホバーで止まり、離すと最低 1.5 秒に戻って再開する。
    const bar = () => noticeEl().querySelector(".banner-timer");
    assert.equal(bar().getAttribute("aria-hidden"), "true");
    assert.equal(bar().getAttribute("data-running"), "true");
    assert.match(bar().getAttribute("style"), /--timer-from:\s*1\b/);
    assert.match(bar().getAttribute("style"), /animation-duration:\s*3000ms/);
    await advance(2500);
    await fire(noticeEl(), "mouseover", { relatedTarget: document.body });
    assert.equal(bar().getAttribute("data-running"), null, "hover pauses the bar");
    assert.match(bar().getAttribute("style"), /--timer-from:\s*0\.1666/);
    assert.match(bar().getAttribute("style"), /animation-play-state:\s*paused/);
    await fire(noticeEl(), "mouseout", { relatedTarget: document.body });
    assert.equal(bar().getAttribute("data-running"), "true");
    assert.match(bar().getAttribute("style"), /--timer-from:\s*0\.5\b/, "the resume floor refills the bar to 1.5s");
    assert.match(bar().getAttribute("style"), /animation-duration:\s*1500ms/);
    await advance(1500);
    assert.equal(noticeEl()?.getAttribute("data-leaving"), "true", "the notice starts leaving instead of vanishing");
    await fire(noticeEl(), "animationend", { animationName: "banner-in" });
    assert.ok(noticeEl(), "only topbar-collapse ends the leave");
    await fire(noticeEl(), "animationend", { animationName: "topbar-collapse" });
    assert.equal(noticeEl(), null, "removed once the slide-out ends");

    // animationend が来なくても上限の時間で取り除く。
    await act(async () => { shell.notify("閉じるの確認"); });
    await fire(noticeEl().querySelector(".banner-close"), "click");
    assert.equal(noticeEl()?.getAttribute("data-leaving"), "true");
    await advance(BANNER_LEAVE_FALLBACK_MS - 1);
    assert.ok(noticeEl());
    await advance(1);
    assert.equal(noticeEl(), null, "the fallback removes a banner whose animation never ended");

    // 操作ボタンは閉じ始めてすぐ実行する。閉じる途中の帯は新しい通知にすぐ置き換わる。
    // 1秒乗せて消去を取り消すとシークバーは消える。時間で消えない通知には出さない。
    await act(async () => { shell.notify("取り消し"); });
    await fire(noticeEl(), "mouseover", { relatedTarget: document.body });
    await advance(1000);
    assert.equal(noticeEl().querySelector(".banner-timer"), null, "a pinned notice has no countdown");
    await fire(noticeEl().querySelector(".banner-close"), "click");
    await advance(BANNER_LEAVE_FALLBACK_MS);
    await act(async () => { shell.notify("残す", undefined, { persist: true }); });
    assert.equal(noticeEl().querySelector(".banner-timer"), null, "a persistent notice has no countdown");
    await fire(noticeEl().querySelector(".banner-close"), "click");
    await advance(BANNER_LEAVE_FALLBACK_MS);
    assert.equal(noticeEl(), null);

    let ran = 0;
    await act(async () => { shell.notify("実行の確認", { label: "保存先を開く", run: () => ran++ }); });
    await fire(noticeEl().querySelector("button.button"), "click");
    assert.equal(ran, 1);
    assert.equal(noticeEl()?.getAttribute("data-leaving"), "true");
    await act(async () => { shell.notify("次の通知"); });
    assert.equal(noticeEl()?.getAttribute("data-leaving"), null);
    assert.ok(noticeEl().textContent.includes("次の通知"));
    await advance(BANNER_LEAVE_FALLBACK_MS);
    assert.ok(noticeEl()?.textContent.includes("次の通知"), "the old leave timer does not remove the new notice");

    // 失敗の帯も×で閉じる動きに入る。
    await act(async () => { shell.fail("保存できませんでした"); });
    await fire(errorEl().querySelector(".banner-close"), "click");
    assert.equal(errorEl()?.getAttribute("data-leaving"), "true");
    await fire(errorEl(), "animationend", { animationName: "topbar-collapse" });
    assert.equal(errorEl(), null);

    // 動きを減らす設定ではすぐ取り除く。
    reduced = true;
    await act(async () => { shell.notify("動きを減らす"); });
    await advance(3000);
    assert.equal(noticeEl(), null, "reduced motion removes the notice at once");
  } finally {
    if (root) await act(async () => root.unmount());
    Date.now = realNow;
    delete window.matchMedia;
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
  // 上部の帯の出入りは、押せない状態で、左サイドバーの開閉と同じ速さ（高さ = --motion-rail、フェード = --motion-fast、--ease-out）。
  const leave = css.match(/:is\(\.banner, \.progress\)\[data-topbar-motion\]\[data-leaving\]\s*\{([^}]*)\}/);
  assert.ok(leave, "banners have a leaving rule");
  assert.match(leave[1], /pointer-events: none/);
  assert.match(leave[1], /topbar-fade-out var\(--motion-fast\) var\(--ease-out\)/);
  assert.match(leave[1], /topbar-collapse var\(--motion-rail\) var\(--ease-out\)/);
  const enter = css.match(/:is\(\.banner, \.progress\)\[data-topbar-motion\]\s*\{([^}]*)\}/);
  assert.match(enter[1], /banner-in var\(--motion-fast\) var\(--ease-out\) var\(--motion-delay\) both/);
  assert.match(enter[1], /topbar-grow var\(--motion-rail\) var\(--ease-out\) backwards/, "grow must not hold the margin after it ends");
  assert.match(css, /@keyframes topbar-collapse \{[\s\S]*?margin-top: calc\(-1 \* var\(--topbar-motion-height/);
  const rail = Number(css.match(/--motion-rail:\s*(\d+)ms/)[1]);
  assert.ok(rail < 400, "the leave ends before the JS fallback");
  // シークバーは朱にせず、角丸の分だけ内側の直線。動きを減らす設定では出さない。
  const timer = css.match(/\.banner-timer \{([^}]*)\}/)[1];
  assert.match(timer, /left: var\(--radius-control\);\s*right: var\(--radius-control\);/);
  assert.doesNotMatch(timer, /accent/);
  assert.match(css, /prefers-reduced-motion: reduce\) \{\s*\.banner-timer \{ display: none; \}/);
});
