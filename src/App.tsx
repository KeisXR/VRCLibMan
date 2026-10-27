import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, appVersion, isTauri, listenBoothUsed, listenProgress, onRefresh } from "./api";
import { AvatarsPage } from "./pages/AvatarsPage";
import { AvatarDetailPage } from "./pages/AvatarDetailPage";
import { ImportPage } from "./pages/ImportPage";
import { QueuePage } from "./pages/QueuePage";
import { SettingsPage } from "./pages/SettingsPage";
import { BrowserPage } from "./pages/BrowserPage";
import { WorldsPage } from "./pages/WorldsPage";
import { AssetsPage } from "./pages/AssetsPage";
import { SetupWizard } from "./pages/SetupWizard";
import { GenreColumn } from "./components/GenreColumn";
import { ResizeSeparator } from "./components/ResizeSeparator";
import { GenreBrowseProvider } from "./genreBrowseContext";
import { useAppLayout } from "./useAppLayout";
import { UI_GENRE_WIDTH, UI_RAIL_WIDTH } from "./uiLayout";
import { AppUpdaterProvider, useStartupUpdateCheck } from "./useAppUpdater";
import { SidebarUpdaterBadge } from "./components/SidebarUpdaterBadge";
import type { BoothAccount, JobProgress, LibraryStats } from "./types";
import { errorText } from "./ui";

type Page = "avatars" | "avatar" | "assets" | "worlds" | "queue" | "import" | "settings" | "browser";

interface Route {
  page: Page;
  id?: string;
  fileBoothId?: string;
  genre?: string;
}

function decodePart(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

interface ShellValue {
  revision: number;
  refresh: () => void;
  notify: (message: string) => void;
  fail: (message: string) => void;
  builtinBrowser: boolean;
  refreshAccount: () => void;
  workInProgress: boolean;
}

const ShellContext = createContext<ShellValue | null>(null);

export function useShell(): ShellValue {
  const value = useContext(ShellContext);
  if (!value) throw new Error("shell missing");
  return value;
}

function parseRoute(hash: string): Route {
  const parts = (hash.replace(/^#/, "") || "/").split("/").filter(Boolean);
  if (parts[0] === "avatar" && parts[1]) {
    return {
      page: "avatar",
      id: decodeURIComponent(parts[1]),
      fileBoothId: parts[2] === "files" && parts[3] ? decodeURIComponent(parts[3]) : undefined,
    };
  }
  if (parts[0] === "assets" || parts[0] === "worlds") {
    return { page: parts[0], genre: parts[1] ? decodePart(parts[1]) : undefined };
  }
  if (parts[0] === "queue" || parts[0] === "import" || parts[0] === "settings" || parts[0] === "browser") {
    return { page: parts[0] };
  }
  return { page: "avatars" };
}

export default function App() {
  const [version, setVersion] = useState("");
  useEffect(() => { void appVersion().then(setVersion, () => setVersion("未確認")); }, []);
  const [hash, setHash] = useState(window.location.hash);
  const [stats, setStats] = useState<LibraryStats | null>(null);
  const [progress, setProgress] = useState<JobProgress | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [builtinBrowser, setBuiltinBrowser] = useState(false);
  const [account, setAccount] = useState<BoothAccount | null>(null);
  const accountReload = useRef<() => void>(() => {});
  const [setupComplete, setSetupComplete] = useState<boolean | null>(null);
  const [checkUpdateOnStartup, setCheckUpdateOnStartup] = useState(false);
  const route = useMemo(() => parseRoute(hash), [hash]);
  const contentRef = useRef<HTMLDivElement>(null);

  const refresh = useCallback(() => setRevision((value) => value + 1), []);
  const notify = useCallback((message: string) => {
    setError(null);
    setNotice(message);
  }, []);
  const fail = useCallback((message: string) => {
    setNotice(null);
    setError(message);
  }, []);

  useEffect(() => {
    const onHash = () => setHash(window.location.hash);
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  useLayoutEffect(() => {
    const pane = contentRef.current;
    if (!pane) return;
    pane.scrollTop = 0;
  }, [hash]);

  useEffect(() => onRefresh(refresh), [refresh]);

  useEffect(() => {
    if (!isTauri()) return;
    let stop = () => undefined as void;
    let alive = true;
    import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen("library-changed", () => {
          if (alive) refresh();
        }),
      )
      .then((unlisten) => {
        stop = unlisten;
      });
    return () => {
      alive = false;
      stop();
    };
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    api.settings().then(
      (value) => {
        if (!cancelled) {
          setBuiltinBrowser(value.builtinBrowser);
          setCheckUpdateOnStartup(value.checkUpdateOnStartup);
        }
      },
      () => {
        if (!cancelled) setBuiltinBrowser(false);
      },
    );
    api.setupStatus().then(
      (value) => {
        if (!cancelled) setSetupComplete(value.complete);
      },
      () => {
        if (!cancelled) setSetupComplete(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision]);

  useEffect(() => {
    let cancelled = false;
    api.stats().then(
      (value) => {
        if (!cancelled) setStats(value);
      },
      (err: unknown) => {
        if (!cancelled) setError(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision]);

  useEffect(() => {
    let stop = () => undefined as void;
    let alive = true;
    listenProgress((next) => {
      if (!alive) return;
      setProgress(next);
      if (next.total > 0 && next.current >= next.total) {
        window.setTimeout(() => {
          if (alive) setProgress(null);
        }, 2500);
      }
    }).then((unlisten) => {
      stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, []);

  const refreshAccount = useCallback(() => {
    accountReload.current();
  }, []);

  useEffect(() => {
    if (!builtinBrowser) {
      accountReload.current = () => {};
      setAccount(null);
      return;
    }
    let alive = true;
    let pending = true;
    let generation = 0;
    const load = () => {
      const ticket = ++generation;
      if (pending) setAccount(null);
      pending = false;
      void api.boothAccount().then(
        (value) => {
          if (alive && ticket === generation) setAccount(value);
        },
        () => {
          if (alive && ticket === generation) {
            setAccount({ loggedIn: false, displayName: "", pixivId: "", avatarUrl: "" });
          }
        },
      );
    };
    accountReload.current = load;
    load();
    let stop = () => undefined as void;
    void listenBoothUsed(() => {
      if (alive) load();
    }).then((unlisten) => {
      if (!alive) unlisten();
      else stop = unlisten;
    });
    return () => {
      alive = false;
      accountReload.current = () => {};
      stop();
    };
  }, [builtinBrowser]);

  const shell = useMemo<ShellValue>(
    () => ({ revision, refresh, notify, fail, builtinBrowser, refreshAccount, workInProgress: progress !== null }),
    [revision, refresh, notify, fail, builtinBrowser, refreshAccount, progress],
  );
  const percent =
    progress && progress.total > 0 ? Math.min(100, (progress.current / progress.total) * 100) : 0;
  const density =
    route.page === "settings"
      ? "density-roomy"
      : route.page === "avatars" || route.page === "avatar" || route.page === "assets" || route.page === "worlds" || route.page === "queue"
        ? "density-compact"
        : "";
  const browseKind = route.page === "assets" ? "asset" : route.page === "worlds" ? "world" : null;
  const browseBase = browseKind === "asset" ? "#/assets" : "#/worlds";
  const layout = useAppLayout(browseKind !== null);

  const mainPane = (
    <div className="main" ref={contentRef}>
      <div className="topbar">
        {error && <div className="banner error">{error}</div>}
        {notice && <div className="banner notice">{notice}</div>}
        {progress && (
          <div className="progress">
            <p>{progress.message}</p>
            <div className={progress.total > 0 ? "track" : "track indeterminate"}>
              <span style={{ width: progress.total > 0 ? `${percent}%` : undefined }} />
            </div>
          </div>
        )}
      </div>
      <main className={`content ${density}`}>
        {route.page === "avatars" && <AvatarsPage />}
        {route.page === "avatar" && route.id && (
          <AvatarDetailPage id={route.id} fileBoothId={route.fileBoothId} />
        )}
        {route.page === "assets" && <AssetsPage />}
        {route.page === "worlds" && <WorldsPage />}
        {route.page === "queue" && <QueuePage />}
        {route.page === "import" && <ImportPage />}
        {route.page === "settings" && <SettingsPage />}
        {route.page === "browser" && <BrowserPage enabled={builtinBrowser} />}
      </main>
    </div>
  );

  const openAppUpdateSettings = useCallback(() => {
    window.location.hash = "#/settings";
    window.setTimeout(() => {
      document.getElementById("settings-app-update")?.scrollIntoView({ behavior: "smooth", block: "start" });
    }, 0);
  }, []);

  return (
    <AppUpdaterProvider>
    <ShellContext.Provider value={shell}>
      {setupComplete === false && <SetupWizard onDone={() => { setSetupComplete(true); refresh(); }} />}
      <div className="app app-shell" style={layout.gridStyle}>
        <aside className="rail">
          <div className="brand">
            <strong>VRCLibMan</strong>
            <span>持っているアバターと衣装</span>
          </div>
          <nav>
            <a className={route.page === "avatars" || route.page === "avatar" ? "nav-link active" : "nav-link"} href="#/">
              アバター
              <span className="count">{stats?.ownedAvatars ?? 0}</span>
            </a>
            <a className={route.page === "assets" ? "nav-link active" : "nav-link"} href="#/assets">
              アセット
            </a>
            <a className={route.page === "worlds" ? "nav-link active" : "nav-link"} href="#/worlds">
              ワールド
            </a>
            <a className={route.page === "queue" ? "nav-link active" : "nav-link"} href="#/queue">
              確認
              <span className="count">{stats?.queueCount ?? 0}</span>
            </a>
            <a className={route.page === "import" ? "nav-link active" : "nav-link"} href="#/import">
              インポート
            </a>
            <a className={route.page === "settings" ? "nav-link active" : "nav-link"} href="#/settings">
              設定
            </a>
            {builtinBrowser && (
              <a className={route.page === "browser" ? "nav-link active" : "nav-link"} href="#/browser">
                ブラウザ
              </a>
            )}
          </nav>
          <div className="rail-foot">
            <StartupUpdateCheck enabled={checkUpdateOnStartup} />
            <SidebarUpdaterBadge onOpenSettings={openAppUpdateSettings} />
            <span className="app-version">{version === "未確認" ? "バージョン未確認" : version ? `v${version}` : "バージョン確認中"}</span>
            {builtinBrowser && (
              <AccountPill
                account={account}
                onLogin={() => {
                  void api.openBoothBrowser().then(
                    () => refreshAccount(),
                    (err: unknown) => fail(errorText(err)),
                  );
                }}
                onLogout={async () => {
                  await api.clearBoothSession();
                  setAccount({ loggedIn: false, displayName: "", pixivId: "", avatarUrl: "" });
                  refreshAccount();
                }}
                onFail={(message) => fail(message)}
              />
            )}
            <div className="rail-meta">
              {stats ? `${stats.products} 商品` : "読み込み中"}
              {!isTauri() && <><br />ブラウザ確認用</>}
            </div>
          </div>
        </aside>
        <ResizeSeparator
          label="メインサイドバーの幅"
          value={layout.railWidth}
          min={UI_RAIL_WIDTH.min}
          max={UI_RAIL_WIDTH.max}
          onChange={layout.setRailWidth}
          onCommit={layout.persistRail}
          onReset={() => {
            layout.setRailWidth(UI_RAIL_WIDTH.default);
            layout.persistRail(UI_RAIL_WIDTH.default);
          }}
        />
        {browseKind ? (
          <GenreBrowseProvider
            kind={browseKind}
            genre={route.genre ?? null}
            basePath={browseBase}
            narrowLayout={layout.narrow}
          >
            {layout.genreVisible && (
              <GenreColumn />
            )}
            {layout.genreVisible && (
              <ResizeSeparator
                label="ジャンル列の幅"
                value={layout.genreWidth}
                min={UI_GENRE_WIDTH.min}
                max={UI_GENRE_WIDTH.max}
                onChange={layout.setGenreWidth}
                onCommit={layout.persistGenre}
                onReset={() => {
                  layout.setGenreWidth(UI_GENRE_WIDTH.default);
                  layout.persistGenre(UI_GENRE_WIDTH.default);
                }}
              />
            )}
            {mainPane}
          </GenreBrowseProvider>
        ) : (
          mainPane
        )}
      </div>
    </ShellContext.Provider>
    </AppUpdaterProvider>
  );
}

function StartupUpdateCheck({ enabled }: { enabled: boolean }) {
  useStartupUpdateCheck(enabled);
  return null;
}

function accountLabel(account: BoothAccount | null): string {
  if (!account) return "確認中";
  if (!account.loggedIn) return "未ログイン";
  if (account.displayName.trim()) return account.displayName.trim();
  if (account.pixivId.trim()) return account.pixivId.trim();
  return "ログイン中";
}

function safeAvatar(url: string): string {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:" || parsed.search || parsed.hash || parsed.username) return "";
    const host = parsed.hostname;
    const allowed =
      host === "booth.pm" ||
      host.endsWith(".booth.pm") ||
      host === "pximg.net" ||
      host.endsWith(".pximg.net");
    return allowed ? parsed.toString() : "";
  } catch {
    return "";
  }
}

function AccountPill({
  account,
  onLogin,
  onLogout,
  onFail,
}: {
  account: BoothAccount | null;
  onLogin: () => void;
  onLogout: () => Promise<void>;
  onFail: (message: string) => void;
}) {
  const [broken, setBroken] = useState(false);
  const [open, setOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const label = accountLabel(account);
  const avatar = !broken && account?.loggedIn ? safeAvatar(account.avatarUrl) : "";
  const face = avatar ? (
    <img className="account-avatar" src={avatar} alt="" onError={() => setBroken(true)} />
  ) : (
    <span className="account-avatar" aria-hidden="true" />
  );

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) {
        setOpen(false);
        setConfirming(false);
      }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        setConfirming(false);
      }
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  if (!account) {
    return (
      <div className="account-pill neutral" title={label}>
        {face}
        <span className="account-name">{label}</span>
      </div>
    );
  }
  if (!account.loggedIn) {
    return (
      <button className="account-pill neutral" type="button" onClick={onLogin} title={label}>
        {face}
        <span className="account-name">{label}</span>
      </button>
    );
  }

  async function confirmLogout() {
    if (busy) return;
    setBusy(true);
    try {
      await onLogout();
      setOpen(false);
      setConfirming(false);
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="account-menu" ref={rootRef}>
      <button
        className="account-pill"
        type="button"
        title={label}
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => {
          setConfirming(false);
          setOpen((value) => !value);
        }}
      >
        {face}
        <span className="account-name">{label}</span>
      </button>
      {open && (
        <div className="account-popover" role="menu">
          {confirming ? (
            <div className="account-confirm">
              <p>内蔵ブラウザの全サイトからログアウトし、閲覧データを削除しますか？</p>
              <button type="button" disabled={busy} onClick={() => void confirmLogout()}>
                ログアウト
              </button>
              <button type="button" disabled={busy} onClick={() => setConfirming(false)}>
                キャンセル
              </button>
            </div>
          ) : (
            <>
              <button type="button" role="menuitem" onClick={() => setConfirming(true)}>
                ログアウト
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setOpen(false);
                  onLogin();
                }}
              >
                ブラウザを開く
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
