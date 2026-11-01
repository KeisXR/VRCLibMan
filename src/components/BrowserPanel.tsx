import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode, RefObject } from "react";
import { api, listenBrowserChanged } from "../api";
import { useShell } from "../App";
import { browserPanelVisible, coveringRects, panelRect, sameReport } from "../browserPanel";
import type { BrowserPanelRect, BrowserState } from "../types";
import { errorText } from "../ui";
import { BrowserChrome } from "./BrowserChrome";
import type { RightPanelValue } from "./RightPanel";

type HostInfo = Pick<BrowserState, "open" | "host" | "panelSupported">;
type Report = { area: BrowserPanelRect | null; visible: boolean };

/** Where the browser is shown. Only the host and the OS support are read, never page addresses. */
function useBrowserHost() {
  const [info, setInfo] = useState<HostInfo | null>(null);
  const load = useCallback(() => {
    void api.browserState().then(
      (state) => setInfo({ open: state.open, host: state.host, panelSupported: state.panelSupported }),
      () => undefined,
    );
  }, []);
  useEffect(() => {
    let alive = true;
    let stop = () => {};
    load();
    void listenBrowserChanged(() => {
      if (alive) load();
    }).then((unlisten) => {
      if (alive) stop = unlisten;
      else unlisten();
    });
    // The separate window can also be closed from the OS; a slow re-read keeps this in step.
    const poll = window.setInterval(() => {
      if (alive) load();
    }, 2000);
    return () => {
      alive = false;
      stop();
      window.clearInterval(poll);
    };
  }, [load]);
  return { info, reload: load };
}

/**
 * Sends the viewport's rectangle (CSS pixels in the main window) to the app, which places the
 * docked pages there. The pages are native views drawn above the HTML, so they are hidden while
 * a menu, dialog, overlay or drag would be under them, and when this tab leaves the screen.
 */
function useDockedArea(viewport: RefObject<HTMLDivElement | null>, docked: boolean, overlay: boolean) {
  useEffect(() => {
    const element = viewport.current;
    if (!docked || !element) return;
    let frame = 0;
    let dragging = false;
    let last: Report | null = null;
    const send = () => {
      frame = 0;
      const area = panelRect(element.getBoundingClientRect());
      const visible = browserPanelVisible(area, { overlay, dragging, covers: coveringRects(document, element) });
      const next = { area, visible };
      if (sameReport(last, next)) return;
      last = next;
      void api.browserAction({ kind: "panelArea", area, visible }).catch(() => undefined);
    };
    const schedule = () => {
      if (frame) return;
      frame = typeof requestAnimationFrame === "function" ? requestAnimationFrame(send) : window.setTimeout(send, 16);
    };
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
    resize?.observe(element);
    const layers = typeof MutationObserver === "undefined" ? null : new MutationObserver(schedule);
    layers?.observe(document.body, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["class", "open", "hidden", "role", "aria-modal", "aria-expanded"],
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
    document.addEventListener("pointerdown", startDrag, true);
    document.addEventListener("dragstart", startDrag, true);
    for (const type of endTypes) document.addEventListener(type, endDrag, true);
    // Moves without a size change (a column collapsing beside the panel) are caught here.
    const safety = window.setInterval(schedule, 500);
    schedule();
    return () => {
      if (frame) {
        if (typeof cancelAnimationFrame === "function") cancelAnimationFrame(frame);
        window.clearTimeout(frame);
      }
      window.clearInterval(safety);
      resize?.disconnect();
      layers?.disconnect();
      window.removeEventListener("resize", schedule);
      document.removeEventListener("pointerdown", startDrag, true);
      document.removeEventListener("dragstart", startDrag, true);
      for (const type of endTypes) document.removeEventListener(type, endDrag, true);
      void api.browserAction({ kind: "panelArea", area: null, visible: false }).catch(() => undefined);
    };
  }, [docked, overlay, viewport]);
}

function PanelMessage({ text, children }: { text: string; children?: ReactNode }) {
  return (
    <div className="right-panel-empty browser-panel-message">
      <p className="meta">{text}</p>
      {children && <div className="row-actions">{children}</div>}
    </div>
  );
}

/** The right panel's browser tab. Windows docks the pages here; other systems use the separate window. */
export function BrowserPanelTab({ overlay }: { panel: RightPanelValue; overlay: boolean }) {
  const { refresh, refreshAccount, fail } = useShell();
  const { info, reload } = useBrowserHost();
  const [working, setWorking] = useState(false);
  const viewport = useRef<HTMLDivElement>(null);
  const docked = info?.host === "panel" && !overlay;
  useDockedArea(viewport, info?.host === "panel", overlay);

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

  if (!info) return <PanelMessage text="ブラウザを確認しています…" />;
  if (!info.panelSupported) {
    return (
      <PanelMessage text="この環境では、ブラウザは別ウィンドウで開きます（右パネルへの表示は Windows で使えます）。">
        <button className="button primary" type="button" disabled={working} onClick={raiseWindow}>
          {info.open ? "ブラウザを前面に" : "別ウィンドウで開く"}
        </button>
      </PanelMessage>
    );
  }
  if (overlay) {
    return (
      <PanelMessage text="ウィンドウの幅が狭いため、ここにはブラウザを表示できません。ウィンドウを広げるか、別ウィンドウで開いてください。">
        <button className="button primary" type="button" disabled={working} onClick={undock}>
          別ウィンドウで開く
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
    <div className="browser-panel">
      <BrowserChrome
        panel
        onImported={() => {
          refresh();
          refreshAccount();
        }}
      />
      <div className="browser-panel-viewport" ref={viewport} data-browser-panel-viewport="">
        <p className="meta">メニューやダイアログを閉じると、ここにページを表示します。</p>
      </div>
    </div>
  );
}
