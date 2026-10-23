import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, isTauri, listenBoothUsed, listenProgress, onRefresh } from "./api";
import { AvatarsPage } from "./pages/AvatarsPage";
import { AvatarDetailPage } from "./pages/AvatarDetailPage";
import { ImportPage } from "./pages/ImportPage";
import { QueuePage } from "./pages/QueuePage";
import { SettingsPage } from "./pages/SettingsPage";
import { BrowserPage } from "./pages/BrowserPage";
import { SetupWizard } from "./pages/SetupWizard";
import type { BoothAccount, JobProgress, LibraryStats } from "./types";
import { errorText } from "./ui";

type Page = "avatars" | "avatar" | "queue" | "import" | "settings" | "browser";

interface Route {
  page: Page;
  id?: string;
  fileBoothId?: string;
}

interface ShellValue {
  revision: number;
  refresh: () => void;
  notify: (message: string) => void;
  fail: (message: string) => void;
  builtinBrowser: boolean;
  refreshAccount: () => void;
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
  if (parts[0] === "queue" || parts[0] === "import" || parts[0] === "settings" || parts[0] === "browser") {
    return { page: parts[0] };
  }
  return { page: "avatars" };
}

export default function App() {
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
  const route = useMemo(() => parseRoute(hash), [hash]);

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
        if (!cancelled) setBuiltinBrowser(value.builtinBrowser);
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
    const load = () => {
      if (pending) setAccount(null);
      pending = false;
      void api.boothAccount().then(
        (value) => {
          if (alive) setAccount(value);
        },
        () => {
          if (alive) setAccount({ loggedIn: false, displayName: "", pixivId: "", avatarUrl: "" });
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
    () => ({ revision, refresh, notify, fail, builtinBrowser, refreshAccount }),
    [revision, refresh, notify, fail, builtinBrowser, refreshAccount],
  );
  const percent =
    progress && progress.total > 0 ? Math.min(100, (progress.current / progress.total) * 100) : 0;
  const density =
    route.page === "settings"
      ? "density-roomy"
      : route.page === "avatars" || route.page === "avatar" || route.page === "queue"
        ? "density-compact"
        : "";

  return (
    <ShellContext.Provider value={shell}>
      {setupComplete === false && <SetupWizard onDone={() => setSetupComplete(true)} />}
      <div className="app">
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
            <a className={route.page === "queue" ? "nav-link active" : "nav-link"} href="#/queue">
              確認
              <span className="count">{stats?.queueCount ?? 0}</span>
            </a>
            <a className={route.page === "import" ? "nav-link active" : "nav-link"} href="#/import">
              取り込み
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
            {builtinBrowser && (
              <AccountPill
                account={account}
                onLogin={() => {
                  void api.openBoothBrowser().then(
                    () => refreshAccount(),
                    (err: unknown) => fail(errorText(err)),
                  );
                }}
              />
            )}
            <div className="rail-meta">
              {stats ? `${stats.products} 商品` : "読み込み中"}
              <br />
              {isTauri() ? stats?.ollamaModel : "ブラウザ確認用"}
            </div>
          </div>
        </aside>
        <div className="main">
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
            {route.page === "queue" && <QueuePage />}
            {route.page === "import" && <ImportPage />}
            {route.page === "settings" && <SettingsPage />}
            {route.page === "browser" && <BrowserPage enabled={builtinBrowser} />}
          </main>
        </div>
      </div>
    </ShellContext.Provider>
  );
}

function accountLabel(account: BoothAccount | null): string {
  if (!account) return "確認しています";
  if (!account.loggedIn) return "ログインしていません";
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

function AccountPill({ account, onLogin }: { account: BoothAccount | null; onLogin: () => void }) {
  const [broken, setBroken] = useState(false);
  const label = accountLabel(account);
  const avatar = !broken && account?.loggedIn ? safeAvatar(account.avatarUrl) : "";
  const face = avatar ? (
    <img className="account-avatar" src={avatar} alt="" onError={() => setBroken(true)} />
  ) : (
    <span className="account-avatar" aria-hidden="true" />
  );
  if (!account || account.loggedIn) {
    return (
      <div className={account ? "account-pill" : "account-pill neutral"} title={label}>
        {face}
        <span className="account-name">{label}</span>
      </div>
    );
  }
  return (
    <button className="account-pill neutral" type="button" onClick={onLogin} title={label}>
      {face}
      <span className="account-name">{label}</span>
    </button>
  );
}
