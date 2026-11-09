import { useEffect, useState, type ReactNode } from "react";
import { isTauri } from "../api";
import { errorText } from "../ui";

/** Window controls belong only to the external host, never to an embedded panel. */
export function BrowserTitleBar({
  embedded,
  children,
  onError,
}: {
  embedded: boolean;
  children: ReactNode;
  onError: (message: string) => void;
}) {
  const macos = /Macintosh|Mac OS X/.test(navigator.userAgent);
  const nativeControls = !embedded && !macos && isTauri();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!nativeControls) return;
    let cancelled = false;
    let unlisten = () => {};
    void import("@tauri-apps/api/window")
      .then(async ({ getCurrentWindow }) => {
        if (cancelled) return;
        const host = getCurrentWindow();
        const update = () =>
          void host
            .isMaximized()
            .then((value) => {
              if (!cancelled) setMaximized(value);
            })
            .catch((error) => {
              if (!cancelled) onError(errorText(error));
            });
        update();
        const stop = await host.onResized(update);
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch((error) => {
        if (!cancelled) onError(errorText(error));
      });
    return () => {
      cancelled = true;
      unlisten();
    };
  }, [nativeControls, onError]);

  async function control(action: "minimize" | "maximize" | "close") {
    if (!nativeControls) return;
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const host = getCurrentWindow();
      if (action === "minimize") await host.minimize();
      else if (action === "close") await host.close();
      else {
        await host.toggleMaximize();
        setMaximized(await host.isMaximized());
      }
    } catch (error) {
      onError(errorText(error));
    }
  }

  return (
    <div
      className={`browser-tabs-row${embedded ? "" : " browser-titlebar"}${!embedded && macos ? " macos" : ""}`}
      data-tauri-drag-region={embedded ? undefined : "deep"}
    >
      {children}
      {!embedded && (
        <div className="browser-window-drag-space" aria-hidden="true" />
      )}
      {!embedded && !macos && (
        <div className="browser-window-controls" data-tauri-drag-region="false">
          <button
            type="button"
            aria-label="ウィンドウを最小化"
            title="最小化"
            disabled={!nativeControls}
            onClick={() => void control("minimize")}
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="M0 5.5h10" />
            </svg>
          </button>
          <button
            type="button"
            aria-label={
              maximized ? "ウィンドウを元のサイズに戻す" : "ウィンドウを最大化"
            }
            title={maximized ? "元のサイズに戻す" : "最大化"}
            disabled={!nativeControls}
            onClick={() => void control("maximize")}
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              {maximized ? (
                <path d="M2.5 2.5v-2h7v7h-2m-7-5h7v7h-7z" />
              ) : (
                <path d="M.5.5h9v9h-9z" />
              )}
            </svg>
          </button>
          <button
            className="browser-window-close"
            type="button"
            aria-label="ブラウザウィンドウを閉じる"
            title="閉じる"
            disabled={!nativeControls}
            onClick={() => void control("close")}
          >
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <path d="m.5.5 9 9m0-9-9 9" />
            </svg>
          </button>
        </div>
      )}
    </div>
  );
}
