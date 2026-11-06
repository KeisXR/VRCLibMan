import { useEffect, useRef } from "react";
import { assetUpdates, isTauri } from "./api";
import type { NoticeAction, NotifyOptions } from "./notice";

export function useAssetUpdateNotice(notify: (message: string, action?: NoticeAction, options?: NotifyOptions) => void) {
  const seen = useRef(new Set<string>());
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let stop = () => {};
    async function update() {
      try {
        const status = await assetUpdates.status();
        if (!alive) return;
        const rows = status.rows.filter((r) => r.changes.length && !seen.current.has(`${r.boothId}:${r.revision}:${r.changes.join(",")}`));
        for (const row of rows) seen.current.add(`${row.boothId}:${row.revision}:${row.changes.join(",")}`);
        if (rows.length) notify(`${rows.length} 件のアセットに更新候補があります`, { label: "更新設定を開く", run: () => { window.location.hash = "#/settings"; window.setTimeout(() => document.getElementById("settings-asset-updates")?.scrollIntoView(), 300); } });
        for (const key of seen.current) if (!status.rows.some((r) => r.changes.length && key === `${r.boothId}:${r.revision}:${r.changes.join(",")}`)) seen.current.delete(key);
      } catch { /* The settings screen reports read failures; do not repeat background banners. */ }
    }
    void import("@tauri-apps/api/event").then(({ listen }) => listen("asset-updates-changed", () => void update())).then((unlisten) => { if (alive) stop = unlisten; else unlisten(); });
    void update();
    const timer = window.setInterval(() => void update(), 60000);
    return () => { alive = false; stop(); window.clearInterval(timer); };
  }, [notify]);
}
