import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode, RefObject } from "react";
import { api, reportBrowserPanel } from "../api";
import { useShell } from "../App";
import { browserPanelVisible, coveringRects, panelRect } from "../browserPanel";
import { refreshBrowserState, subscribeBrowserState, useBrowserSelection } from "../browserStore";
import type { BrowserSnapshot } from "../browserStore";
import { FLOAT_PANEL_MOVED_EVENT } from "../floatingPanel";
import type { BrowserState } from "../types";
import { errorText } from "../ui";
import { BrowserChrome } from "./BrowserChrome";
import type { RightPanelValue } from "./RightPanel";

type HostInfo = Pick<BrowserState, "open" | "host" | "panelSupported">;

type HostView = { info: HostInfo | null; error: string };

function selectHost({ state, error }: BrowserSnapshot): HostView {
  // A failed re-read keeps the last known host, so a passing error does not unmount the page.
  if (state) return { info: { open: state.open, host: state.host, panelSupported: state.panelSupported }, error: "" };
  return { info: null, error };
}

function sameHost(a: HostView, b: HostView): boolean {
  if (a.error !== b.error) return false;
  if (!a.info || !b.info) return a.info === b.info;
  return a.info.open === b.info.open && a.info.host === b.info.host && a.info.panelSupported === b.info.panelSupported;
}

/**
 * Where the browser is shown. Only the host and the OS support are read, never page addresses.
 * Title and loading changes do not draw this tab again. The separate window can also be closed
 * from the OS; the shared slow re-read keeps this in step.
 */
function useBrowserHost() {
  const { info, error } = useBrowserSelection(selectHost, sameHost, { poll: true });
  return { info, error, reload: refreshBrowserState };
}

/** At most one page image per this many ms while pages change (macOS draws one; elsewhere there is none). */
export const SNAPSHOT_MIN_MS = 500;

/**
 * Pulls the page image shown while a menu covers the native page. One pull runs at a time.
 * `now` is for the moments right before the page may be covered; `soon` follows page changes
 * at most every SNAPSHOT_MIN_MS.
 */
function useSnapshotPull(docked: boolean) {
  const [snapshot, setSnapshot] = useState<string | null>(null);
  const puller = useRef<{ now: () => void; soon: () => void }>({ now: () => {}, soon: () => {} });
  useEffect(() => {
    if (!docked) {
      setSnapshot(null);
      return;
    }
    let alive = true;
    let running = false;
    let queued = false;
    let last = 0;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const run = () => {
      if (!alive) return;
      if (running) {
        queued = true;
        return;
      }
      running = true;
      last = Date.now();
      void api
        .browserPanelSnapshot()
        .then(
          (next) => {
            if (alive && next) setSnapshot(next);
          },
          () => undefined,
        )
        .finally(() => {
          running = false;
          if (queued) {
            queued = false;
            soon();
          }
        });
    };
    const soon = () => {
      if (!alive || timer !== null) return;
      timer = setTimeout(() => {
        timer = null;
        run();
      }, Math.max(0, last + SNAPSHOT_MIN_MS - Date.now()));
    };
    const now = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      run();
    };
    puller.current = { now, soon };
    now();
    const stop = subscribeBrowserState(soon);
    return () => {
      alive = false;
      if (timer !== null) clearTimeout(timer);
      stop();
      puller.current = { now: () => {}, soon: () => {} };
    };
  }, [docked]);
  const pullNow = useCallback(() => puller.current.now(), []);
  return { snapshot, pullNow };
}

/**
 * Sends the viewport's rectangle (CSS pixels in the main window) to the app, which places the
 * docked pages there. The pages are native views drawn above the HTML, so they are hidden while
 * a menu, dialog, overlay or drag would be under them, and when this tab leaves the screen.
 * In a narrow window the panel floats and can be moved; the pages follow when it lands.
 */
function useDockedArea(
  viewport: RefObject<HTMLDivElement | null>,
  docked: boolean,
  onCovered?: () => void,
) {
  const coveredRef = useRef(onCovered);
  coveredRef.current = onCovered;
  useEffect(() => {
    const element = viewport.current;
    if (!docked || !element) return;
    let frame = 0;
    let disposed = false;
    let dragging = false;
    let covered = false;
    const send = () => {
      if (disposed) return;
      frame = 0;
      const area = panelRect(element.getBoundingClientRect());
      const visible = browserPanelVisible(area, { dragging, covers: coveringRects(document, element) });
      if (!visible && area && !covered) coveredRef.current?.();
      covered = !visible && area !== null;
      reportBrowserPanel({ area, visible });
    };
    const schedule = () => {
      if (disposed || frame) return;
      frame = typeof requestAnimationFrame === "function" ? requestAnimationFrame(send) : window.setTimeout(send, 16);
    };
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
    resize?.observe(element);
    const layers = typeof MutationObserver === "undefined" ? null : new MutationObserver(schedule);
    layers?.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["class", "open", "hidden", "role", "aria-modal", "aria-expanded", "data-rail-hold"],
    });
    const startDrag = (event: Event) => {
      const target = event.target;
      if (event.type !== "dragstart" && !(target instanceof Element && target.closest(".resize-separator"))) return;
      dragging = true;
      schedule();
    };
    const endDrag = () => {
      if (!dragging) return;
      dragging = false;
      schedule();
    };
    const endTypes = ["pointerup", "pointercancel", "dragend", "drop"];
    window.addEventListener("resize", schedule);
    window.addEventListener(FLOAT_PANEL_MOVED_EVENT, schedule);
    document.addEventListener("pointerdown", startDrag, true);
    document.addEventListener("dragstart", startDrag, true);
    for (const type of endTypes) document.addEventListener(type, endDrag, true);
    // Moves without a size change (a column collapsing beside the panel) are caught here.
    const safety = window.setInterval(schedule, 500);
    schedule();
    return () => {
      disposed = true;
      if (frame) {
        if (typeof cancelAnimationFrame === "function") cancelAnimationFrame(frame);
        window.clearTimeout(frame);
      }
      window.clearInterval(safety);
      resize?.disconnect();
      layers?.disconnect();
      window.removeEventListener("resize", schedule);
      window.removeEventListener(FLOAT_PANEL_MOVED_EVENT, schedule);
      document.removeEventListener("pointerdown", startDrag, true);
      document.removeEventListener("dragstart", startDrag, true);
      for (const type of endTypes) document.removeEventListener(type, endDrag, true);
      reportBrowserPanel({ area: null, visible: false });
    };
  }, [docked, viewport]);
}

function PanelMessage({ text, children }: { text: string; children?: ReactNode }) {
  return (
    <div className="right-panel-empty browser-panel-message">
      <p className="meta">{text}</p>
      {children && <div className="row-actions">{children}</div>}
    </div>
  );
}

/**
 * The right panel's page view. Desktop platforms dock pages here or move them to a separate window.
 * The docked column and the floating panel of a narrow window both hold the pages.
 */
export function BrowserPanelTab({
  unified = false,
}: {
  panel: RightPanelValue;
  /** The parent strip already shows the page tabs. */
  unified?: boolean;
}) {
  const { refresh, refreshAccount, fail } = useShell();
  const { info, error: hostError, reload } = useBrowserHost();
  const [working, setWorking] = useState(false);
  const viewport = useRef<HTMLDivElement>(null);
  const docked = info?.host === "panel";
  const { snapshot, pullNow: refreshSnapshot } = useSnapshotPull(docked);
  useDockedArea(viewport, docked, refreshSnapshot);

  // The tab strip sits above the page; a pointer there may be about to open a menu over it.
  useEffect(() => {
    if (!docked) return;
    const head = viewport.current?.closest(".right-panel")?.querySelector(".right-panel-head");
    head?.addEventListener("pointerenter", refreshSnapshot);
    return () => head?.removeEventListener("pointerenter", refreshSnapshot);
  }, [docked, refreshSnapshot]);

  async function run(task: () => Promise<unknown>) {
    setWorking(true);
    try {
      await task();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setWorking(false);
      reload();
    }
  }
  const dock = () => void run(() => api.browserAction({ kind: "dock", area: null }));
  const undock = () => void run(() => api.browserAction({ kind: "undock" }));
  const raiseWindow = () => void run(() => api.openBoothBrowser());

  if (!info) return <PanelMessage text={hostError || "ブラウザを確認しています…"}>
    {hostError && <button className="button" type="button" onClick={reload}>もう一度確認</button>}
  </PanelMessage>;
  if (!info.panelSupported) {
    return (
      <PanelMessage text="この環境では、ブラウザは別ウィンドウで開きます。">
        <button className="button primary" type="button" disabled={working} onClick={raiseWindow}>
          {info.open ? "ブラウザを前面に" : "別ウィンドウで開く"}
        </button>
      </PanelMessage>
    );
  }
  if (info.host === "window") {
    return (
      <PanelMessage text="ブラウザは別ウィンドウで表示しています。">
        <button className="button primary" type="button" disabled={working} onClick={dock}>
          パネルに戻す
        </button>
        <button className="button" type="button" disabled={working} onClick={raiseWindow}>
          ウィンドウを前面に
        </button>
      </PanelMessage>
    );
  }
  if (!docked) {
    return (
      <PanelMessage text="BOOTH の購入ライブラリなどを、このパネルで開けます。ログイン状態は別ウィンドウと共通です。">
        <button className="button primary" type="button" disabled={working} onClick={dock}>
          ブラウザを開く
        </button>
        <button className="button" type="button" disabled={working} onClick={undock}>
          別ウィンドウで開く
        </button>
      </PanelMessage>
    );
  }
  return (
    <div className="browser-panel" onPointerEnter={refreshSnapshot} onFocusCapture={refreshSnapshot}>
      <BrowserChrome
        panel
        hostId="panel"
        showPageTabs={!unified}
        onImported={() => {
          refresh();
          refreshAccount();
        }}
      />
      <div className="browser-panel-viewport" ref={viewport} data-browser-panel-viewport="">
        {snapshot ? (
          <img className="browser-panel-snapshot" src={snapshot} alt="" aria-hidden="true" />
        ) : (
          <p className="meta">メニューやダイアログを閉じると、ここにページを表示します。</p>
        )}
      </div>
    </div>
  );
}
