import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  isTauri,
  listenBrowserChanged,
  listenBrowserControlEvents,
} from "../api";
import type { BrowserAction, BrowserBookmark, BrowserState } from "../types";
import { errorText } from "../ui";
import { BrowserTitleBar } from "./BrowserTitleBar";
import {
  ArrowGlyph,
  CloseGlyph,
  DockGlyph,
  HomeGlyph,
  PlusGlyph,
  PopOutGlyph,
  ReloadGlyph,
  StarGlyph,
  StopGlyph,
} from "./glyphs";

const HOME = "https://accounts.booth.pm/library";
const emptyState: BrowserState = {
  open: false,
  tabs: [],
  activeId: null,
  hosts: [],
  bookmarks: [],
  busy: false,
  host: null,
  panelSupported: false,
};
type BookmarkEdit = { originalUrl: string | null; title: string; url: string };

/**
 * Reusable controls for the separate browser window (default), the browser page (`embedded`)
 * and the right panel (`panel`, which is embedded too). The pages themselves are native views
 * that the app places next to these controls.
 */
export function BrowserChrome({
  embedded: embeddedProp = false,
  panel = false,
  hostId,
  showPageTabs = !panel,
  onImported,
}: {
  embedded?: boolean;
  /** Shown in the main window's right panel, above the docked pages. */
  panel?: boolean;
  /** Limit controls to one panel or window. The right panel passes "panel". */
  hostId?: string;
  /** The right panel draws page tabs in its own strip, so this row stays hidden there. */
  showPageTabs?: boolean;
  onImported?: () => void;
}) {
  const embedded = embeddedProp || panel;
  const [state, setState] = useState(emptyState);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const [address, setAddress] = useState("");
  const [edit, setEdit] = useState<BookmarkEdit | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const controls = useRef<HTMLDivElement>(null);
  const editingAddress = useRef(false);
  const activeId = useRef<string | null>(null);
  const epoch = useRef(0);
  const reading = useRef(false);
  const alive = useRef(false);
  const scopedHost = hostId ?? new URLSearchParams(window.location.search).get("browserHost") ?? undefined;
  const host = scopedHost ? state.hosts.find((item) => item.id === scopedHost) : undefined;
  const pageTabs = host ? state.tabs.filter((tab) => tab.hostId === host.id) : state.tabs;
  const selectedId = host?.activeId ?? state.activeId;
  const active = pageTabs.find((tab) => tab.id === selectedId) ?? state.tabs.find((tab) => tab.id === selectedId);
  const disabled = working || state.busy;
  const bookmarked = state.bookmarks.find((item) => item.url === active?.url);
  const canImport = (() => {
    if (!active) return false;
    try {
      const url = new URL(active.url);
      return (
        url.protocol === "https:" &&
        (url.hostname === "booth.pm" || url.hostname.endsWith(".booth.pm")) &&
        url.pathname.includes("/library")
      );
    } catch {
      return false;
    }
  })();

  const load = useCallback(async () => {
    if (reading.current) return;
    reading.current = true;
    const ticket = epoch.current;
    try {
      const next = await api.browserState();
      if (alive.current && ticket === epoch.current) setState(next);
    } catch (err) {
      if (alive.current) setError(errorText(err));
    } finally {
      reading.current = false;
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    void load();
    let stop = () => {};
    void listenBrowserChanged(() => void load()).then((unlisten) => {
      if (!cancelled) stop = unlisten;
      else unlisten();
    });
    let stopControls = () => {};
    if (!embedded)
      void listenBrowserControlEvents(() => {
        input.current?.focus();
        input.current?.select();
      }, setError).then((stop) => {
        if (!cancelled) stopControls = stop;
        else stop();
      });
    const poll = window.setInterval(() => void load(), 1000);
    if (!embedded) document.body.classList.add("browser-toolbar-window");
    return () => {
      cancelled = true;
      alive.current = false;
      window.clearInterval(poll);
      stop();
      stopControls();
      if (!embedded) document.body.classList.remove("browser-toolbar-window");
    };
  }, [embedded, load]);

  useEffect(() => {
    const switched = activeId.current !== (active?.id ?? null);
    activeId.current = active?.id ?? null;
    if (switched || !editingAddress.current) setAddress(active?.url ?? "");
    if (switched) setEdit(null);
  }, [active?.id, active?.url]);

  useEffect(() => {
    controls.current
      ?.querySelector('[role="tab"][aria-selected="true"]')
      ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [selectedId]);

  useEffect(() => {
    if (
      embedded ||
      !isTauri() ||
      !controls.current ||
      typeof ResizeObserver === "undefined"
    )
      return;
    let previous = 0;
    const observer = new ResizeObserver(([entry]) => {
      const height = Math.max(136, Math.ceil(entry.contentRect.height));
      if (height !== previous) {
        previous = height;
        void api
          .browserAction({ kind: "toolbarHeight", height, hostId: scopedHost })
          .catch((err) => setError(errorText(err)));
      }
    });
    observer.observe(controls.current);
    return () => observer.disconnect();
  }, [embedded, scopedHost]);

  const action = useCallback(async (command: BrowserAction) => {
    const hostIdForCommand = scopedHost;
    const tabId = active?.id;
    let targeted = command;
    if (command.kind === "newTab") targeted = { ...command, hostId: command.hostId ?? hostIdForCommand };
    else if (command.kind === "navigate" || command.kind === "back" || command.kind === "forward" || command.kind === "reload" || command.kind === "stop")
      targeted = { ...command, hostId: command.hostId ?? hostIdForCommand, tabId: command.tabId ?? tabId };
    else if (command.kind === "toolbarHeight" || command.kind === "dock")
      targeted = { ...command, hostId: command.hostId ?? hostIdForCommand };
    epoch.current++;
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const next = await api.browserAction(targeted);
      if (alive.current) {
        setState(next);
        if (command.kind === "newTab")
          window.setTimeout(() => {
            input.current?.focus();
            input.current?.select();
          }, 0);
      }
    } catch (err) {
      if (alive.current) setError(errorText(err));
    } finally {
      if (alive.current) setWorking(false);
    }
  }, [active?.id, scopedHost]);

  useEffect(() => {
    // Native accelerators also work when an external tab has keyboard focus.
    if (!embedded && isTauri()) return;
    function shortcut(event: KeyboardEvent) {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || disabled) return;
      // In the right panel the rest of the app keeps its keys; only focus in these controls counts.
      if (panel && !controls.current?.contains(document.activeElement)) return;
      if (event.key.toLowerCase() === "l") {
        event.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
      if (event.key.toLowerCase() === "t") {
        event.preventDefault();
        void action({ kind: "newTab" });
      }
      if (event.key.toLowerCase() === "w" && active?.id) {
        event.preventDefault();
        void action({ kind: "closeTab", tabId: active.id });
      }
      if (event.key.toLowerCase() === "r" && active?.id) {
        event.preventDefault();
        void action({ kind: "reload" });
      }
    }
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, [action, disabled, active?.id, embedded, panel]);

  async function saveBookmark() {
    if (!edit) return;
    epoch.current++;
    setWorking(true);
    setError("");
    try {
      setState(
        await api.saveBrowserBookmark(edit.originalUrl, edit.title, edit.url),
      );
      setEdit(null);
      setMessage("ブックマークを保存しました");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setWorking(false);
    }
  }
  async function deleteBookmark() {
    if (!edit?.originalUrl) return;
    epoch.current++;
    setWorking(true);
    setError("");
    try {
      setState(await api.deleteBrowserBookmark(edit.originalUrl));
      setEdit(null);
      setMessage("ブックマークを削除しました");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setWorking(false);
    }
  }
  function editBookmark(bookmark: BrowserBookmark) {
    setEdit({ originalUrl: bookmark.url, ...bookmark });
  }

  async function importPage() {
    setWorking(true);
    setError("");
    try {
      const result = await api.importBoothPage();
      setMessage(`${result.inserted} 件追加、${result.updated} 件更新しました`);
      onImported?.();
    } catch (err) {
      setError(errorText(err));
    } finally {
      setWorking(false);
    }
  }

  return (
    <div
      className={panel ? "browser-chrome embedded panel" : embedded ? "browser-chrome embedded" : "browser-chrome"}
      ref={controls}
    >
      {showPageTabs && <BrowserTitleBar embedded={embedded} onError={setError}>
        <div
          className="browser-tabs"
          data-tauri-drag-region={embedded ? undefined : "false"}
          role="tablist"
          aria-label="ブラウザのタブ"
        >
          {pageTabs.map((tab, index) => (
            <div
              className={
                tab.id === selectedId ? "browser-tab active" : "browser-tab"
              }
              key={tab.id}
            >
              <button
                type="button"
                role="tab"
                aria-selected={tab.id === selectedId}
                disabled={disabled}
                title={tab.url}
                onClick={() =>
                  void action({ kind: "selectTab", tabId: tab.id })
                }
                onKeyDown={(event) => {
                  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
                    event.preventDefault();
                    const next =
                      pageTabs[
                        (index +
                          (event.key === "ArrowRight"
                            ? 1
                            : pageTabs.length - 1)) %
                          pageTabs.length
                      ];
                    if (next)
                      void action({ kind: "selectTab", tabId: next.id });
                  }
                }}
              >
                {tab.loading && <span className="browser-tab-loading" aria-hidden="true" />}
                <span className="browser-tab-title">{tab.title || "新しいタブ"}</span>
              </button>
              <button
                className="browser-tab-close"
                type="button"
                disabled={disabled}
                aria-label={`${tab.title || "このページ"}のタブを閉じる`}
                title="タブを閉じる"
                onClick={() => void action({ kind: "closeTab", tabId: tab.id })}
              >
                <CloseGlyph />
              </button>
            </div>
          ))}
        </div>
        <button
          className="browser-icon browser-new-tab"
          type="button"
          disabled={disabled || pageTabs.length >= 20}
          aria-label="新しいタブ"
          title="新しいタブ"
          onClick={() => void action({ kind: "newTab" })}
        >
          <PlusGlyph />
        </button>
      </BrowserTitleBar>}
      <div className="browser-navigation">
        <button
          className="browser-icon"
          type="button"
          disabled={disabled || !active?.canGoBack}
          aria-label="戻る"
          title="戻る"
          onClick={() => void action({ kind: "back" })}
        >
          <ArrowGlyph direction="back" />
        </button>
        <button
          className="browser-icon"
          type="button"
          disabled={disabled || !active?.canGoForward}
          aria-label="進む"
          title="進む"
          onClick={() => void action({ kind: "forward" })}
        >
          <ArrowGlyph direction="forward" />
        </button>
        <button
          className="browser-icon"
          type="button"
          disabled={disabled || !active}
          aria-label={active?.loading ? "読み込みを停止" : "再読み込み"}
          title={active?.loading ? "読み込みを停止" : "再読み込み"}
          onClick={() =>
            void action({ kind: active?.loading ? "stop" : "reload" })
          }
        >
          {active?.loading ? <StopGlyph /> : <ReloadGlyph />}
        </button>
        <button
          className="browser-icon"
          type="button"
          disabled={disabled}
          aria-label="購入ライブラリを開く"
          title="購入ライブラリ"
          onClick={() => void action({ kind: "navigate", url: HOME })}
        >
          <HomeGlyph />
        </button>
        <form
          className="browser-address"
          onSubmit={(event) => {
            event.preventDefault();
            editingAddress.current = false;
            void action({ kind: "navigate", url: address });
          }}
        >
          <input
            aria-label="アドレスバー"
            ref={input}
            value={address}
            placeholder="URLを入力"
            spellCheck={false}
            autoComplete="off"
            onFocus={() => {
              editingAddress.current = true;
            }}
            onBlur={() => {
              editingAddress.current = false;
            }}
            onChange={(event) => setAddress(event.target.value)}
          />
          <button
            type="submit"
            disabled={disabled || !address.trim()}
            aria-label="入力したURLを開く"
          >
            開く
          </button>
        </form>
        <button
          className="browser-icon"
          type="button"
          disabled={working || !active}
          aria-label={
            bookmarked ? "ブックマークを編集" : "このページをブックマーク"
          }
          title={bookmarked ? "ブックマークを編集" : "このページをブックマーク"}
          onClick={() =>
            bookmarked
              ? editBookmark(bookmarked)
              : setEdit({
                  originalUrl: null,
                  title: active?.title ?? "",
                  url: active?.url ?? "",
                })
          }
        >
          <StarGlyph filled={Boolean(bookmarked)} />
        </button>
        {(!embedded || panel) && (
          <button
            className="browser-import"
            type="button"
            disabled={disabled || !canImport}
            title="BOOTHのライブラリをインポート"
            onClick={() => void importPage()}
          >
            インポート
          </button>
        )}
        {panel && (
          <button
            className="browser-icon browser-host-toggle"
            type="button"
            disabled={working}
            aria-label="別ウィンドウで開く"
            title="別ウィンドウで開く"
            onClick={() => void action({ kind: "undock" })}
          >
            <PopOutGlyph />
          </button>
        )}
        {!embedded && state.panelSupported && (
          <button
            className="browser-icon browser-host-toggle"
            type="button"
            disabled={working}
            aria-label="パネルに戻す"
            title="アプリの右パネルに戻す"
            onClick={() => void action({ kind: "dock", area: null })}
          >
            <DockGlyph />
          </button>
        )}
      </div>
      <div className="browser-bookmarks" aria-label="ブックマークバー">
        {state.bookmarks.map((bookmark) => (
          <span className="browser-bookmark" key={bookmark.url}>
            <button
              type="button"
              title={bookmark.url}
              disabled={disabled}
              onClick={(event) =>
                void action(
                  event.ctrlKey || event.metaKey
                    ? { kind: "newTab", url: bookmark.url }
                    : { kind: "navigate", url: bookmark.url },
                )
              }
              onAuxClick={(event) => {
                if (event.button === 1) {
                  event.preventDefault();
                  void action({ kind: "newTab", url: bookmark.url });
                }
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                editBookmark(bookmark);
              }}
            >
              {bookmark.title}
            </button>
            <button
              className="bookmark-edit"
              type="button"
              aria-label={`${bookmark.title}を編集`}
              title="ブックマークを編集"
              onClick={() => editBookmark(bookmark)}
            >
              ⋯
            </button>
          </span>
        ))}
        {state.bookmarks.length === 0 && (
          <span className="meta">☆ からブックマークを追加できます</span>
        )}
      </div>
      {edit && (
        <form
          className="browser-bookmark-editor"
          aria-label="ブックマークの編集"
          onSubmit={(event) => {
            event.preventDefault();
            void saveBookmark();
          }}
        >
          <label>
            <span>名前</span>
            <input
              aria-label="ブックマーク名"
              value={edit.title}
              onChange={(event) =>
                setEdit({ ...edit, title: event.target.value })
              }
              maxLength={100}
            />
          </label>
          <label>
            <span>URL</span>
            <input
              aria-label="ブックマークURL"
              value={edit.url}
              onChange={(event) =>
                setEdit({ ...edit, url: event.target.value })
              }
              maxLength={4096}
            />
          </label>
          <div className="row-actions">
            <button className="button primary" type="submit" disabled={working}>
              保存
            </button>
            {edit.originalUrl && (
              <button
                className="button"
                type="button"
                disabled={working}
                onClick={() => void deleteBookmark()}
              >
                削除
              </button>
            )}
            <button
              className="button"
              type="button"
              onClick={() => setEdit(null)}
            >
              キャンセル
            </button>
          </div>
        </form>
      )}
      <div
        className={error ? "browser-status warn-text" : "browser-status meta"}
        role={error ? "alert" : "status"}
      >
        {error ||
          message ||
          (state.busy
            ? "インポートまたはダウンロード中"
            : active?.loading
              ? "読み込み中…"
              : !state.open
                ? "ブラウザを開くか、URLを入力してください"
                : "")}
      </div>
    </div>
  );
}
