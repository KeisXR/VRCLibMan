import { useAssetUpdateNotice } from "./useAssetUpdateNotice";
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, appVersion, isTauri, listenBoothUsed, listenProgress, onRefresh } from "./api";
import { AvatarsPage } from "./pages/AvatarsPage";
import { AvatarDetailPage } from "./pages/AvatarDetailPage";
import { ImportPage } from "./pages/ImportPage";
import { ClassificationLogPage } from "./pages/ClassificationLogPage";
import { SettingsPage } from "./pages/SettingsPage";
import { BrowserPage } from "./pages/BrowserPage";
import { WorldsPage } from "./pages/WorldsPage";
import { AssetsPage } from "./pages/AssetsPage";
import { SearchPage } from "./pages/SearchPage";
import { SetupWizard } from "./pages/SetupWizard";
import { GenreColumn } from "./components/GenreColumn";
import { ResizeSeparator } from "./components/ResizeSeparator";
import { GenreBrowseProvider } from "./genreBrowseContext";
import { useAppLayout } from "./useAppLayout";
import { UI_GENRE_WIDTH, UI_RAIL_WIDTH } from "./uiLayout";
import { AppUpdaterProvider, useStartupUpdateCheck } from "./useAppUpdater";
import { SidebarUpdaterBadge } from "./components/SidebarUpdaterBadge";
import type { LibraryStats } from "./types";
import {
  EMPTY_PROGRESS,
  applyProgress,
  nextPruneDelay,
  othersLabel,
  progressStrip,
  pruneProgress,
  workInProgress as anyWorkInProgress,
} from "./progress";
import type { ProgressBoard } from "./progress";
import {
  accountAfterLogout,
  accountLabel,
  applyAccountResult,
  checkingAccount,
  type AccountSnapshot,
} from "./accountStatus";
import { SidebarClassificationHint } from "./components/SidebarClassificationHint";
import { BrandMark, NavIcon, SidebarToggleIcon, type NavIconName } from "./components/navIcons";
import { RightPanelColumn, RightPanelLauncher, RightPanelProvider, useRightPanelState } from "./components/RightPanel";
import { ProgressTrack, errorText } from "./ui";
import { BrowserAutoClassification } from "./components/BrowserAutoClassification";
import { ErrorBanner, NoticeBanner } from "./components/NoticeBanner";
import type { NoticeAction, NotifyOptions } from "./notice";
import { noticeAction, useBanners } from "./useBanners";

export type { NoticeAction, NotifyOptions } from "./notice";

type Page = "avatars" | "avatar" | "assets" | "worlds" | "search" | "classification-log" | "import" | "settings" | "browser";

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

export { noticeAction };

interface ShellValue {
  revision: number;
  refresh: () => void;
  /** 情報通知。時間で消える。失敗や注意を含むときは `{ persist: true }` で残す。 */
  notify: (message: string, action?: NoticeAction | (() => void), options?: NotifyOptions) => void;
  fail: (message: string) => void;
  builtinBrowser: boolean;
  refreshAccount: () => void;
  workInProgress: boolean;
  /** Library-wide search text, shared by the sidebar search box and the search page. */
  searchQuery: string;
  setSearchQuery: (query: string) => void;
}

const ShellContext = createContext<ShellValue | null>(null);

/** Lets a single screen part be rendered on its own (tests and previews). */
export const ShellProvider = ShellContext.Provider;

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
  if (parts[0] === "classification-log") {
    return { page: "classification-log", id: parts[1] ? decodeURIComponent(parts[1]) : undefined };
  }
  if (parts[0] === "search" || parts[0] === "import" || parts[0] === "settings" || parts[0] === "browser") {
    return { page: parts[0] };
  }
  return { page: "avatars" };
}

export default function App() {
  const [version, setVersion] = useState("");
  useEffect(() => { void appVersion().then(setVersion, () => setVersion("未確認")); }, []);
  const [hash, setHash] = useState(window.location.hash);
  const [stats, setStats] = useState<LibraryStats | null>(null);
  const [progressBoard, setProgressBoard] = useState<ProgressBoard>(EMPTY_PROGRESS);
  const { notice, error, notify, fail, setError, dismissNotice, dismissError, runNoticeAction } = useBanners();
  useAssetUpdateNotice(notify);
  const [revision, setRevision] = useState(0);
  const [searchQuery, setSearchQuery] = useState("");
  // While an IME composition is open in the sidebar box, moving to the search page would
  // unmount the input mid-composition and drop the text being converted.
  const searchComposing = useRef(false);
  const [builtinBrowser, setBuiltinBrowser] = useState(false);
  const [accountView, setAccountView] = useState<AccountSnapshot>(checkingAccount());
  const accountReload = useRef<() => void>(() => {});
  const accountGeneration = useRef(0);
  const [setupComplete, setSetupComplete] = useState<boolean | null>(null);
  const [checkUpdateOnStartup, setCheckUpdateOnStartup] = useState(false);
  const route = useMemo(() => parseRoute(hash), [hash]);
  const contentRef = useRef<HTMLDivElement>(null);
  const topbarRef = useRef<HTMLDivElement>(null);

  const refresh = useCallback(() => setRevision((value) => value + 1), []);

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

  // Sticky chip bars (.chip-bar) sit just under the sticky banners; publish their height.
  useEffect(() => {
    const pane = contentRef.current;
    const bar = topbarRef.current;
    if (!pane || !bar || typeof ResizeObserver === "undefined") return;
    const update = () => pane.style.setProperty("--topbar-height", `${Math.ceil(bar.getBoundingClientRect().height)}px`);
    const observer = new ResizeObserver(update);
    observer.observe(bar);
    update();
    return () => observer.disconnect();
  }, []);

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
  }, [revision, setError]);

  useEffect(() => {
    let stop = () => undefined as void;
    let alive = true;
    listenProgress((next) => {
      if (!alive) return;
      setProgressBoard((board) => applyProgress(board, next, Date.now()));
    }).then((unlisten) => {
      if (!alive) unlisten();
      else stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, []);

  // 終わった仕事は少し残してから消す（docs/design/README.md#進捗）。
  useEffect(() => {
    const delay = nextPruneDelay(progressBoard, Date.now());
    if (delay === null) return;
    const timer = window.setTimeout(() => {
      setProgressBoard((board) => pruneProgress(board, Date.now()));
    }, delay);
    return () => window.clearTimeout(timer);
  }, [progressBoard]);

  const refreshAccount = useCallback(() => {
    accountReload.current();
  }, []);

  useEffect(() => {
    if (!builtinBrowser) {
      accountReload.current = () => {};
      setAccountView(checkingAccount());
      return;
    }
    let alive = true;
    const load = () => {
      const ticket = ++accountGeneration.current;
      void api.boothAccount().then(
        (value) => {
          if (!alive) return;
          setAccountView((previous) =>
            applyAccountResult(previous, accountGeneration.current, ticket, { ok: true, account: value }),
          );
        },
        () => {
          if (!alive) return;
          setAccountView((previous) =>
            applyAccountResult(previous, accountGeneration.current, ticket, { ok: false }),
          );
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

  const working = anyWorkInProgress(progressBoard);
  const shell = useMemo<ShellValue>(
    () => ({ revision, refresh, notify, fail, builtinBrowser, refreshAccount, workInProgress: working, searchQuery, setSearchQuery }),
    [revision, refresh, notify, fail, builtinBrowser, refreshAccount, working, searchQuery],
  );
  const strip = progressStrip(progressBoard);
  const density =
    route.page === "settings"
      ? "density-roomy"
      : route.page === "avatars" || route.page === "avatar" || route.page === "assets" || route.page === "worlds" || route.page === "search" || route.page === "classification-log"
        ? "density-compact"
        : "";
  const browseKind = route.page === "assets" ? "asset" : route.page === "worlds" ? "world" : null;
  const browseBase = browseKind === "asset" ? "#/assets" : "#/worlds";
  const rightPanel = useRightPanelState(fail, { builtinBrowser });
  const layout = useAppLayout(browseKind !== null, rightPanel);
  const collapsed = layout.railCollapsed;
  const navItems: { href: string; label: string; icon: NavIconName; active: boolean; count?: number }[] = [
    { href: "#/", label: "アバター", icon: "avatars", active: route.page === "avatars" || route.page === "avatar", count: stats?.ownedAvatars ?? 0 },
    { href: "#/assets", label: "アセット", icon: "assets", active: route.page === "assets" },
    { href: "#/worlds", label: "ワールド", icon: "worlds", active: route.page === "worlds" },
    { href: "#/search", label: "検索", icon: "search", active: route.page === "search" },
    { href: "#/classification-log", label: "分類記録", icon: "log", active: route.page === "classification-log" },
    { href: "#/import", label: "インポート", icon: "import", active: route.page === "import" },
    { href: "#/settings", label: "設定", icon: "settings", active: route.page === "settings" },
    ...(builtinBrowser
      ? [{ href: "#/browser", label: "ブラウザ", icon: "browser" as const, active: route.page === "browser" }]
      : []),
  ];

  const mainPane = (
    <div className="main" ref={contentRef} hidden={layout.panelMaximized}>
      <div className="topbar" ref={topbarRef}>
        <RightPanelLauncher panel={rightPanel} />
        {error && <ErrorBanner key={error.id} error={error} onDismiss={dismissError} />}
        {notice && <NoticeBanner key={notice.id} notice={notice} onDismiss={dismissNotice} onAction={runNoticeAction} />}
        {strip && (
          <div className="progress">
            {strip.others > 0 ? (
              <div className="progress-head">
                <p>{strip.entry.message}</p>
                <span className="progress-more">{othersLabel(strip.others)}</span>
              </div>
            ) : (
              <p>{strip.entry.message}</p>
            )}
            <ProgressTrack
              current={strip.entry.current}
              total={strip.entry.total}
              state={strip.entry.state}
              label={strip.entry.message}
            />
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
        {route.page === "search" && <SearchPage />}
        {route.page === "classification-log" && <ClassificationLogPage boothId={route.id} />}
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
    <RightPanelProvider value={rightPanel}>
      <BrowserAutoClassification />
      {setupComplete === false && <SetupWizard onRefresh={refresh} onDone={() => { setSetupComplete(true); refresh(); }} />}
      <div className="app app-shell" style={layout.gridStyle} {...layout.railMotionAttrs}>
        <aside
          className={collapsed ? "rail collapsed" : "rail"}
          data-collapsed={collapsed ? "true" : "false"}
          data-auto-collapsed={layout.railAuto ? "true" : undefined}
        >
          <div className="brand">
            <span className="brand-lockup">
              <BrandMark />
              {/* Wordmark image outlined from Urbanist Black 900; see docs/design/README.md. */}
              <span className="brand-wordmark" role="img" aria-label="VRCLibMan" title="VRCLibMan" />
            </span>
            {/* A narrow window shows icons only without touching the saved state, so the toggle is hidden. */}
            {!layout.railAuto && (
            <button
              type="button"
              className="rail-toggle"
              aria-expanded={!collapsed}
              aria-controls="main-sidebar-nav"
              aria-label={collapsed ? "サイドバーを開く" : "サイドバーを閉じる"}
              title={collapsed ? "サイドバーを開く" : "サイドバーを閉じる"}
              onClick={() => layout.setRailCollapsed(!collapsed)}
            >
              <SidebarToggleIcon collapsed={collapsed} />
            </button>
            )}
          </div>
          {!collapsed && (
            <div className="sidebar-search">
              <input
                className="search"
                type="search"
                aria-label="ライブラリを検索"
                placeholder="ライブラリを検索"
                value={searchQuery}
                onChange={(event) => {
                  setSearchQuery(event.target.value);
                  if (!searchComposing.current && route.page !== "search") window.location.hash = "#/search";
                }}
                onCompositionStart={() => { searchComposing.current = true; }}
                onCompositionEnd={() => {
                  searchComposing.current = false;
                  if (route.page !== "search") window.location.hash = "#/search";
                }}
              />
            </div>
          )}
          <nav id="main-sidebar-nav" aria-label="メイン">
            {navItems.map((item) => (
              <a
                key={item.href}
                className={item.active ? "nav-link active" : "nav-link"}
                href={item.href}
                aria-current={item.active ? "page" : undefined}
                title={collapsed ? (item.count == null ? item.label : `${item.label}（${item.count}）`) : undefined}
              >
                <NavIcon name={item.icon} />
                <span className="nav-label">{item.label}</span>
                {item.count != null && <span className="count">{item.count}</span>}
              </a>
            ))}
          </nav>
          <div className="rail-foot">
            <SidebarClassificationHint revision={revision} compact={collapsed} />
            <StartupUpdateCheck enabled={checkUpdateOnStartup} />
            <SidebarUpdaterBadge onOpenSettings={openAppUpdateSettings} compact={collapsed} />
            {builtinBrowser && (
              <AccountPill
                view={accountView}
                compact={collapsed}
                onLogin={() => {
                  void api.openBoothBrowser().then(
                    () => refreshAccount(),
                    (err: unknown) => fail(errorText(err)),
                  );
                }}
                onLogout={async () => {
                  await api.clearBoothSession();
                  setAccountView(accountAfterLogout());
                  refreshAccount();
                }}
                onFail={(message) => fail(message)}
              />
            )}
            {!collapsed && (
              <div className="rail-status">
                <span className="app-version">{version === "未確認" ? "バージョン未確認" : version ? `v${version}` : "バージョン確認中"}</span>
                <span className="rail-status-sep" aria-hidden="true" />
                <span className="rail-status-count">{stats ? `ライブラリ ${stats.products} 商品` : "読み込み中"}</span>
                {!isTauri() && <span className="rail-preview-chip">ブラウザ確認用</span>}
              </div>
            )}
          </div>
        </aside>
        {collapsed ? (
          // Keeps the grid's separator column; the icon rail has a fixed width.
          <div className="resize-separator rail-fixed-separator" aria-hidden="true" />
        ) : (
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
        )}
        {browseKind ? (
          <GenreBrowseProvider
            kind={browseKind}
            genre={route.genre ?? null}
            basePath={browseBase}
            genreBar={layout.genreBar}
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
        <RightPanelColumn
          panel={rightPanel}
          docked={layout.panelDocked}
          width={layout.panelWidth}
          maxWidth={layout.panelMax}
        />
      </div>
    </RightPanelProvider>
    </ShellContext.Provider>
    </AppUpdaterProvider>
  );
}

function StartupUpdateCheck({ enabled }: { enabled: boolean }) {
  useStartupUpdateCheck(enabled);
  return null;
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
  view,
  compact = false,
  onLogin,
  onLogout,
  onFail,
}: {
  view: AccountSnapshot;
  /** Icon-only sidebar: show just the face; the name and state move to the tooltip and label. */
  compact?: boolean;
  onLogin: () => void;
  onLogout: () => Promise<void>;
  onFail: (message: string) => void;
}) {
  const [broken, setBroken] = useState(false);
  const [open, setOpen] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const label = accountLabel(view);
  const signedIn = view.account?.loggedIn === true && view.phase !== "checking";
  const avatar = !broken && signedIn ? safeAvatar(view.account?.avatarUrl ?? "") : "";
  const unverified = view.phase === "unverified" ? " unverified" : "";
  const initial = (view.account?.displayName.trim() || view.account?.pixivId.trim() || "").slice(0, 1);
  const face = avatar ? (
    <img className="account-avatar" src={avatar} alt="" onError={() => setBroken(true)} />
  ) : (
    <span className="account-avatar" aria-hidden="true">
      {signedIn && initial ? (
        initial
      ) : (
        <svg className="glyph" width="16" height="16" viewBox="0 0 16 16">
          <circle cx="8" cy="5.4" r="2.6" />
          <path d="M2.8 13.6c.7-2.3 2.7-3.5 5.2-3.5s4.5 1.2 5.2 3.5" />
        </svg>
      )}
    </span>
  );
  const nameText = signedIn
    ? view.account?.displayName.trim() || view.account?.pixivId.trim() || "ログイン中"
    : label;
  const subText = signedIn && view.phase === "unverified" ? "確認できません" : "";
  const compactLabel = signedIn ? `${nameText}${subText ? `（${subText}）` : ""}` : label;
  const pillClass = compact ? " compact" : "";
  const pillText = (
    <>
      {face}
      <span className="account-text">
        <span className="account-name">{nameText}</span>
        {subText && <span className="account-sub warn">{subText}</span>}
      </span>
    </>
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

  if (!signedIn) {
    if (view.phase === "checking") {
      return (
        <div className={`account-pill neutral${pillClass}`} title={label} data-account-phase={view.phase}>
          {pillText}
        </div>
      );
    }
    return (
      <button
        className={`account-pill neutral${unverified}${pillClass}`}
        type="button"
        onClick={onLogin}
        title={compact ? `${label}（ログイン）` : label}
        data-account-phase={view.phase}
      >
        {pillText}
        <span className="account-action">ログイン</span>
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
        className={`account-pill${unverified}${pillClass}`}
        type="button"
        title={compact ? compactLabel : label}
        data-account-phase={view.phase}
        aria-expanded={open}
        aria-haspopup="menu"
        onClick={() => {
          setConfirming(false);
          setOpen((value) => !value);
        }}
      >
        {pillText}
        <span className={`account-chevron${open ? " open" : ""}`} aria-hidden="true">
          <svg className="glyph" width="12" height="12" viewBox="0 0 12 12">
            <path d="M3 7.5 6 4.5l3 3" />
          </svg>
        </span>
      </button>
      {open && (
        <div className={compact ? "account-popover compact" : "account-popover"} role="menu">
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
