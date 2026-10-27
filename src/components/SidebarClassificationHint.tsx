import { useEffect, useState } from "react";
import { api } from "../api";
import type { ClassificationStatus } from "../types";

export function SidebarClassificationHint({ revision }: { revision: number }) {
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

  return (
    <div className="sidebar-classify-hint">
      <p>
        分類ルールが新しくなりました。再分類をおすすめします（{status.staleCount}件）
      </p>
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
