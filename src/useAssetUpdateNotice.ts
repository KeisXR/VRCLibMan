import { useEffect, useRef } from "react";
import { assetUpdates, isTauri } from "./api";
import type { NoticeAction, NotifyOptions } from "./notice";

export function useAssetUpdateNotice(notify: (message: string, action?: NoticeAction, options?: NotifyOptions) => void) {
  const seen = useRef(new Set<string>());
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let stop = () => {};
    let request = 0;
    async function update() {
      const id = ++request;
      try {
        const status = await assetUpdates.status();
        if (!alive || id !== request || status.running) return;
        const keys = new Set(status.rows.filter(r => r.enabled && r.changes.length).map(r => `${r.boothId}:${r.candidateKey ?? r.changes.join(",")}`));
        const count = [...keys].filter(key => !seen.current.has(key)).length;
        seen.current = keys;
        if (count) notify(`${count} 件のアセットに更新候補があります`, { label: "アセット更新記録を開く", run: () => { window.location.hash = "#/asset-update-log"; } });
      } catch { /* The settings screen reports read failures; do not repeat background banners. */ }
    }
    void import("@tauri-apps/api/event").then(({ listen }) => listen("asset-updates-changed", () => void update())).then((unlisten) => { if (alive) stop = unlisten; else unlisten(); });
    void update();
    const timer = window.setInterval(() => void update(), 60000);
    return () => { alive = false; stop(); window.clearInterval(timer); };
  }, [notify]);
}
