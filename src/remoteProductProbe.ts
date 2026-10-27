import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { FILE_PRESENCE_LABEL } from "./media";
import type { DownloadView, ItemView, RemoteProductProbe } from "./types";

export const REMOTE_PROBE_CACHE_MS = 60_000;
export const REMOTE_PROBE_CLIENT_TIMEOUT_MS = 10_000;

export type RemoteProbeStatus = "idle" | "probing" | "ok" | "failed";

const probeCache = new Map<string, { probe: RemoteProductProbe; expires: number }>();

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
  probeCache.clear();
}

export function applyRemoteProbe(item: ItemView, probe: RemoteProductProbe | null): ItemView {
  if (!probe) return item;
  return {
    ...item,
    remoteChecked: probe.remoteChecked,
    remoteReachable: probe.remoteReachable,
    downloads: probe.downloads,
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

  const probeKey = useMemo(() => {
    const ids = items.filter(itemNeedsRemoteProbe).map((item) => item.boothId);
    ids.sort();
    return ids.join(",");
  }, [items]);

  const runProbe = useCallback(async (boothIds: string[], runId: number) => {
    if (boothIds.length === 0) return;
    try {
      const result = await withClientTimeout(
        api.probeRemoteProducts(boothIds),
        REMOTE_PROBE_CLIENT_TIMEOUT_MS,
      );
      if (runRef.current !== runId) return;
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
      if (runRef.current !== runId) return;
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
  }, [probeKey, items, runProbe]);

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

  const statusFor = useCallback(
    (boothId: string) => {
      const item = mergedItems.find((row) => row.boothId === boothId);
      if (!item) return "ok" as RemoteProbeStatus;
      return remoteProbeStatusFor(item, overlays);
    },
    [mergedItems, overlays],
  );

  return { items: mergedItems, statusFor, retryProbe };
}
