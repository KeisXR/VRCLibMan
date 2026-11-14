import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { FILE_PRESENCE_LABEL } from "./media";
import type { DownloadView, ItemView, RemoteProductProbe } from "./types";

export const REMOTE_PROBE_CACHE_MS = 60_000;
export const REMOTE_PROBE_CLIENT_TIMEOUT_MS = 10_000;

export type RemoteProbeStatus = "idle" | "probing" | "ok" | "failed";

const probeCache = new Map<string, { probe: RemoteProductProbe; expires: number }>();
let cacheGeneration = 0;
const cacheResetListeners = new Set<() => void>();

export function itemNeedsRemoteProbe(item: ItemView): boolean {
  return item.downloads.length > 0 && item.remoteConfigured && !item.remoteChecked;
}

export function readProbeCache(boothId: string, now = Date.now()): RemoteProductProbe | null {
  const hit = probeCache.get(boothId);
  if (!hit || hit.expires <= now) {
    if (hit) probeCache.delete(boothId);
    return null;
  }
  return hit.probe;
}

export function writeProbeCache(probes: RemoteProductProbe[], now = Date.now()) {
  const expires = now + REMOTE_PROBE_CACHE_MS;
  for (const probe of probes) {
    probeCache.set(probe.boothId, { probe, expires });
  }
}

export function clearRemoteProbeCache() {
  // A result started for an older library must not refill the cleared cache.
  cacheGeneration++;
  probeCache.clear();
  for (const reset of cacheResetListeners) reset();
}

const invalidationListeners = new Set<(boothId: string) => void>();

/** ダウンロード完了などで手元の状態が変わった商品の、古いリモート確認結果を捨てる。 */
export function invalidateRemoteProbe(boothId: string) {
  probeCache.delete(boothId);
  for (const listener of invalidationListeners) listener(boothId);
}

export function onRemoteProbeInvalidated(listener: (boothId: string) => void): () => void {
  invalidationListeners.add(listener);
  return () => {
    invalidationListeners.delete(listener);
  };
}

function fileKey(file: DownloadView): string {
  return `${file.variation}\u0000${file.filename}`;
}

/**
 * 手元にあるファイルは、新しく読み直した一覧の状態を正とする。
 * リモートの確認結果は、手元に無い行の「リモートのみ／未ダウンロード」の判断だけに使う。
 * 確認のあとで名前が変わった行や増えた行は、読み直した一覧のまま残す。
 */
export function applyRemoteProbe(item: ItemView, probe: RemoteProductProbe | null): ItemView {
  if (!probe) return item;
  const probed = new Map(probe.downloads.map((file) => [fileKey(file), file]));
  return {
    ...item,
    remoteChecked: probe.remoteChecked,
    remoteReachable: probe.remoteReachable,
    downloads: item.downloads.map((file) => {
      if (file.state === "local") return file;
      const hit = probed.get(fileKey(file));
      return hit && hit.state !== "local" ? hit : file;
    }),
  };
}

export function remoteProbeStatusFor(
  item: ItemView,
  overlays: Record<string, RemoteProbeStatus>,
): RemoteProbeStatus {
  if (!item.remoteConfigured || item.downloads.length === 0) return "ok";
  if (item.remoteChecked) return "ok";
  return overlays[item.boothId] ?? "idle";
}

export function placeLabelForFile(
  file: DownloadView,
  item: ItemView,
  probeStatus: RemoteProbeStatus,
): string {
  if (file.state === "local" || !item.remoteConfigured) {
    return FILE_PRESENCE_LABEL[file.state];
  }
  if (probeStatus === "failed") return "未確認";
  if (!item.remoteChecked || probeStatus === "probing" || probeStatus === "idle") return "確認中";
  return FILE_PRESENCE_LABEL[file.state];
}

export function withClientTimeout<T>(promise: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("リモートの確認がタイムアウトしました")), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err: unknown) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}

export function useRemoteProductProbe(items: ItemView[]) {
  const [probes, setProbes] = useState<Record<string, RemoteProductProbe>>({});
  const [overlays, setOverlays] = useState<Record<string, RemoteProbeStatus>>({});
  const runRef = useRef(0);
  const [resetGeneration, setResetGeneration] = useState(cacheGeneration);

  useEffect(() => {
    const reset = () => {
      runRef.current++;
      setProbes({});
      setOverlays({});
      setResetGeneration(cacheGeneration);
    };
    cacheResetListeners.add(reset);
    return () => {
      cacheResetListeners.delete(reset);
      runRef.current++;
    };
  }, []);

  const probeKey = useMemo(() => {
    const ids = items.filter(itemNeedsRemoteProbe).map((item) => item.boothId);
    ids.sort();
    return ids.join(",");
  }, [items]);

  useEffect(
    () =>
      onRemoteProbeInvalidated((boothId) => {
        setProbes((prev) => {
          if (!(boothId in prev)) return prev;
          const next = { ...prev };
          delete next[boothId];
          return next;
        });
        setOverlays((prev) => {
          if (!(boothId in prev)) return prev;
          const next = { ...prev };
          delete next[boothId];
          return next;
        });
      }),
    [],
  );

  const runProbe = useCallback(async (boothIds: string[], runId: number) => {
    if (boothIds.length === 0) return;
    const generation = cacheGeneration;
    try {
      const result = await withClientTimeout(
        api.probeRemoteProducts(boothIds),
        REMOTE_PROBE_CLIENT_TIMEOUT_MS,
      );
      if (runRef.current !== runId || generation !== cacheGeneration) return;
      writeProbeCache(result);
      setProbes((prev) => {
        const next = { ...prev };
        for (const probe of result) next[probe.boothId] = probe;
        return next;
      });
      setOverlays((prev) => {
        const next = { ...prev };
        for (const id of boothIds) next[id] = "ok";
        return next;
      });
    } catch {
      if (runRef.current !== runId || generation !== cacheGeneration) return;
      setOverlays((prev) => {
        const next = { ...prev };
        for (const id of boothIds) next[id] = "failed";
        return next;
      });
    }
  }, []);

  useEffect(() => {
    const needed = items.filter(itemNeedsRemoteProbe);
    if (needed.length === 0) return;

    const fromCache: Record<string, RemoteProductProbe> = {};
    const toFetch: string[] = [];
    for (const item of needed) {
      const cached = readProbeCache(item.boothId);
      if (cached) fromCache[item.boothId] = cached;
      else toFetch.push(item.boothId);
    }
    if (Object.keys(fromCache).length > 0) {
      setProbes((prev) => ({ ...prev, ...fromCache }));
      setOverlays((prev) => {
        const next = { ...prev };
        for (const id of Object.keys(fromCache)) next[id] = "ok";
        return next;
      });
    }
    if (toFetch.length === 0) return;

    const runId = ++runRef.current;
    setOverlays((prev) => {
      const next = { ...prev };
      for (const id of toFetch) next[id] = "probing";
      return next;
    });
    void runProbe(toFetch, runId);
  }, [probeKey, items, runProbe, resetGeneration]);

  const mergedItems = useMemo(
    () =>
      items.map((item) => {
        const probe = probes[item.boothId] ?? readProbeCache(item.boothId);
        return applyRemoteProbe(item, probe);
      }),
    [items, probes],
  );

  const retryProbe = useCallback(
    (boothIds: string[]) => {
      const ids = [...new Set(boothIds.filter(Boolean))];
      if (ids.length === 0) return;
      for (const id of ids) probeCache.delete(id);
      const runId = ++runRef.current;
      setOverlays((prev) => {
        const next = { ...prev };
        for (const id of ids) next[id] = "probing";
        return next;
      });
      void runProbe(ids, runId);
    },
    [runProbe],
  );

  const mergedById = useMemo(() => new Map(mergedItems.map((item) => [item.boothId, item])), [mergedItems]);
  const statusFor = useCallback(
    (boothId: string) => {
      const item = mergedById.get(boothId);
      if (!item) return "ok" as RemoteProbeStatus;
      return remoteProbeStatusFor(item, overlays);
    },
    [mergedById, overlays],
  );

  return { items: mergedItems, statusFor, retryProbe };
}
