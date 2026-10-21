import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { api, isTauri, listenProgress, onRefresh } from "./api";
import { AvatarsPage } from "./pages/AvatarsPage";
import { AvatarDetailPage } from "./pages/AvatarDetailPage";
import { ImportPage } from "./pages/ImportPage";
import { QueuePage } from "./pages/QueuePage";
import { SettingsPage } from "./pages/SettingsPage";
import type { JobProgress, LibraryStats } from "./types";
import { errorText } from "./ui";

type Page = "avatars" | "avatar" | "queue" | "import" | "settings";

interface Route {
  page: Page;
  id?: string;
}

interface ShellValue {
  revision: number;
  refresh: () => void;
  notify: (message: string) => void;
  fail: (message: string) => void;
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
    return { page: "avatar", id: decodeURIComponent(parts[1]) };
  }
  if (parts[0] === "queue" || parts[0] === "import" || parts[0] === "settings") {
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

  const shell = useMemo<ShellValue>(
    () => ({ revision, refresh, notify, fail }),
    [revision, refresh, notify, fail],
  );
  const percent =
    progress && progress.total > 0 ? Math.min(100, (progress.current / progress.total) * 100) : 0;

  return (
    <ShellContext.Provider value={shell}>
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
          </nav>
          <div className="rail-foot">
            {stats ? `${stats.products} 商品` : "読み込み中"}
            <br />
            {isTauri() ? stats?.ollamaModel : "ブラウザ確認用"}
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
          <main className="content">
            {route.page === "avatars" && <AvatarsPage />}
            {route.page === "avatar" && route.id && <AvatarDetailPage id={route.id} />}
            {route.page === "queue" && <QueuePage />}
            {route.page === "import" && <ImportPage />}
            {route.page === "settings" && <SettingsPage />}
          </main>
        </div>
      </div>
    </ShellContext.Provider>
  );
}
