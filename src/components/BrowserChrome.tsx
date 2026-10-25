import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  isTauri,
  listenBrowserChanged,
  listenBrowserControlEvents,
} from "../api";
import type { BrowserAction, BrowserBookmark, BrowserState } from "../types";
import { errorText } from "../ui";

const HOME = "https://accounts.booth.pm/library";
const emptyState: BrowserState = {
  open: false,
  tabs: [],
  activeId: null,
  bookmarks: [],
  busy: false,
};
type BookmarkEdit = { originalUrl: string | null; title: string; url: string };

/** Reusable controls: their state and actions do not depend on an external window or a dock. */
export function BrowserChrome({
  embedded = false,
  onImported,
}: {
  embedded?: boolean;
  onImported?: () => void;
}) {
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
  const active = state.tabs.find((tab) => tab.id === state.activeId);
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
  }, [state.activeId]);

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
          .browserAction({ kind: "toolbarHeight", height })
          .catch((err) => setError(errorText(err)));
      }
    });
    observer.observe(controls.current);
    return () => observer.disconnect();
  }, [embedded]);

  const action = useCallback(async (command: BrowserAction) => {
    epoch.current++;
    setWorking(true);
    setError("");
    setMessage("");
    try {
      const next = await api.browserAction(command);
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
  }, []);

  useEffect(() => {
    function shortcut(event: KeyboardEvent) {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || disabled) return;
      if (event.key.toLowerCase() === "l") {
        event.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
      if (event.key.toLowerCase() === "t") {
        event.preventDefault();
        void action({ kind: "newTab" });
      }
      if (event.key.toLowerCase() === "w" && state.activeId) {
        event.preventDefault();
        void action({ kind: "closeTab", tabId: state.activeId });
      }
      if (event.key.toLowerCase() === "r" && state.activeId) {
        event.preventDefault();
        void action({ kind: "reload" });
      }
    }
    window.addEventListener("keydown", shortcut);
    return () => window.removeEventListener("keydown", shortcut);
  }, [action, disabled, state.activeId]);

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
      className={embedded ? "browser-chrome embedded" : "browser-chrome"}
      ref={controls}
    >
      <div className="browser-tabs-row">
        <div
          className="browser-tabs"
          role="tablist"
          aria-label="ブラウザのタブ"
        >
          {state.tabs.map((tab, index) => (
            <div
              className={
                tab.id === state.activeId ? "browser-tab active" : "browser-tab"
              }
              key={tab.id}
            >
              <button
                type="button"
                role="tab"
                aria-selected={tab.id === state.activeId}
                disabled={disabled}
                title={tab.url}
                onClick={() =>
                  void action({ kind: "selectTab", tabId: tab.id })
                }
                onKeyDown={(event) => {
                  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
                    event.preventDefault();
                    const next =
                      state.tabs[
                        (index +
                          (event.key === "ArrowRight"
                            ? 1
                            : state.tabs.length - 1)) %
                          state.tabs.length
                      ];
                    if (next)
                      void action({ kind: "selectTab", tabId: next.id });
                  }
                }}
              >
                <span aria-hidden="true">{tab.loading ? "◌" : "◻"}</span>
                <span>{tab.title || "新しいタブ"}</span>
              </button>
              <button
                className="browser-tab-close"
                type="button"
                disabled={disabled}
                aria-label={`${tab.title || "このページ"}のタブを閉じる`}
                onClick={() => void action({ kind: "closeTab", tabId: tab.id })}
              >
                ×
              </button>
            </div>
          ))}
        </div>
        <button
          className="browser-icon"
          type="button"
          disabled={disabled || state.tabs.length >= 20}
          aria-label="新しいタブ"
          title="新しいタブ"
          onClick={() => void action({ kind: "newTab" })}
        >
          ＋
        </button>
      </div>
      <div className="browser-navigation">
        <button
          className="browser-icon"
          type="button"
          disabled={disabled || !active?.canGoBack}
          aria-label="戻る"
          title="戻る"
          onClick={() => void action({ kind: "back" })}
        >
          ←
        </button>
        <button
          className="browser-icon"
          type="button"
          disabled={disabled || !active?.canGoForward}
          aria-label="進む"
          title="進む"
          onClick={() => void action({ kind: "forward" })}
        >
          →
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
          {active?.loading ? "×" : "↻"}
        </button>
        <button
          className="browser-icon"
          type="button"
          disabled={disabled}
          aria-label="購入ライブラリを開く"
          title="購入ライブラリ"
          onClick={() => void action({ kind: "navigate", url: HOME })}
        >
          ⌂
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
          {bookmarked ? "★" : "☆"}
        </button>
        <button
          className="browser-import"
          type="button"
          disabled={disabled || !canImport}
          title="BOOTHのライブラリをインポート"
          onClick={() => void importPage()}
        >
          インポート
        </button>
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
