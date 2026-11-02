import { createContext, createElement, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { appUpdater, isTauri, listenUpdaterChanged } from "./api";
import type { AppUpdateStatus } from "./types";

function acceptStatus(current: AppUpdateStatus | null, next: AppUpdateStatus): AppUpdateStatus {
  return !current || next.revision >= current.revision ? next : current;
}

function useAppUpdaterState() {
  const [status, setStatus] = useState<AppUpdateStatus | null>(null);
  const [pending, setPending] = useState(false);
  const pendingRef = useRef(false);
  const [error, setError] = useState("");

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    void appUpdater.status().then(
      (next) => {
        if (alive) setStatus(next);
      },
      () => undefined,
    );
    void listenUpdaterChanged((next) => {
      if (alive) setStatus((current) => acceptStatus(current, next));
    }).then((stop) => {
      if (!alive) stop();
      else unlisten = stop;
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  const run = useCallback(async (operation: () => Promise<AppUpdateStatus>) => {
    // Guard synchronously without changing the callbacks used by the startup effect.
    if (pendingRef.current) return;
    pendingRef.current = true;
    setPending(true);
    setError("");
    try {
      const next = await operation();
      setStatus((current) => acceptStatus(current, next));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      pendingRef.current = false;
      setPending(false);
    }
  }, []);

  const check = useCallback((manual = true) => run(() => appUpdater.check(manual)), [run]);

  return { status, pending, error, setError, run, check };
}

type UpdaterValue = ReturnType<typeof useAppUpdaterState>;

const UpdaterContext = createContext<UpdaterValue | null>(null);

export function AppUpdaterProvider({ children }: { children: ReactNode }) {
  const value = useAppUpdaterState();
  return createElement(UpdaterContext.Provider, { value }, children);
}

export function useAppUpdater(): UpdaterValue {
  const value = useContext(UpdaterContext);
  if (!value) throw new Error("AppUpdaterProvider が見つかりません");
  return value;
}

export function useStartupUpdateCheck(enabled: boolean) {
  const { check } = useAppUpdater();
  const checked = useRef(false);
  useEffect(() => {
    if (!enabled || !isTauri() || checked.current) return;
    checked.current = true;
    void check(false);
  }, [enabled, check]);
}
