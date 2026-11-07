import { sameReport } from "./browserPanel";
import type { BrowserPanelRect } from "./types";

export type PanelReport = { area: BrowserPanelRect | null; visible: boolean };

/** One ordered queue survives a panel's unmount so its final hide is retried too. */
export function createPanelReporter(send: (report: PanelReport) => Promise<unknown>, retryMs = 500) {
  let desired: PanelReport | null = null;
  let acknowledged: PanelReport | null = null;
  let running = false;
  let failed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  async function drain() {
    if (running || !desired || sameReport(acknowledged, desired)) return;
    running = true;
    const sent = desired;
    try {
      await send(sent);
      acknowledged = sent;
      failed = false;
    } catch {
      if (!failed) console.warn("[VRCLibMan] Panel placement not acknowledged; retrying");
      failed = true;
      acknowledged = null;
      // Retain the latest desired state, not the failed rectangle. A close can arrive
      // while a show is pending, and an old completion must not undo that close.
      timer = setTimeout(() => { timer = null; void drain(); }, retryMs);
    } finally {
      running = false;
      if (!timer) void drain();
    }
  }

  return (report: PanelReport) => {
    desired = report;
    if (!timer) void drain();
  };
}
