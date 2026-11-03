import { useEffect, useState } from "react";
import { api } from "../api";
import type { ClassificationStatus } from "../types";
import { ReclassifyIcon } from "./navIcons";

export function SidebarClassificationHint({ revision, compact = false }: { revision: number; compact?: boolean }) {
  const [status, setStatus] = useState<ClassificationStatus | null>(null);
  const [dismissedVersion, setDismissedVersion] = useState(0);

  useEffect(() => {
    void api.classificationStatus().then(setStatus).catch(() => setStatus(null));
    void api.settings().then((settings) => setDismissedVersion(settings.classificationDismissedVersion));
  }, [revision]);

  if (!status || status.staleCount === 0 || dismissedVersion >= status.classifierVersion) {
    return null;
  }

  async function dismiss() {
    const next = await api.updateSettings({
      classificationDismissedVersion: status!.classifierVersion,
    });
    setDismissedVersion(next.classificationDismissedVersion);
  }

  const message = `分類ルールが新しくなりました。再分類をおすすめします（${status.staleCount}件）`;
  if (compact) {
    // The icon rail has no room for the card; the link keeps the message as its name.
    return (
      <a className="rail-icon-button attention" href="#/import" aria-label={`${message}。再分類へ`} title={message}>
        <ReclassifyIcon />
      </a>
    );
  }

  return (
    <div className="sidebar-classify-hint">
      <p>{message}</p>
      <div className="sidebar-classify-actions">
        <a className="button primary" href="#/import">
          再分類へ
        </a>
        <button type="button" className="button" onClick={() => void dismiss()}>
          あとで
        </button>
      </div>
    </div>
  );
}
