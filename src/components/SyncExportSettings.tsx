import { useState } from "react";
import { exportSyncData } from "../api";
import type { Settings } from "../types";
import { errorText } from "../ui";
import { SettingRow } from "./SettingRow";

/** 端末名と同期データの書き出し。取り込みと共有フォルダへの自動書き込みはここに置かない。 */
export function SyncExportSettings({
  settings,
  onChange,
  onBlur,
  beforeExport,
  onNotify,
  onFail,
}: {
  settings: Settings;
  onChange: (patch: Partial<Settings>, debounceMs?: number) => void;
  onBlur: () => void;
  beforeExport: () => Promise<Settings | null>;
  onNotify: (message: string) => void;
  onFail: (message: string) => void;
}) {
  const [busy, setBusy] = useState(false);

  async function exportData() {
    setBusy(true);
    try {
      const saved = await beforeExport();
      const result = await exportSyncData(saved?.deviceName ?? settings.deviceName);
      if (result === "cancelled") return;
      const { products, links, avatars, fileLinks, config } = result.counts;
      onNotify(`エクスポートしました（商品 ${products} 件、リンク ${links} 件、アバター ${avatars} 件、ファイル ${fileLinks} 件、設定 ${config} 件）`);
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <SettingRow label="この端末の名前" help="同期データに記録する名前です。ほかの端末から見分けるために使います。">
        <input aria-label="この端末の名前" value={settings.deviceName} maxLength={64} onChange={(e) => onChange({ deviceName: e.target.value }, 300)} onBlur={onBlur} />
      </SettingRow>
      <SettingRow label="同期データ" help="分類、対応アバター、所持アバター、ファイルとの対応、端末に依存しない設定を JSON に書き出します。商品名とファイル名を含みます。パス、APIキー、説明文、サムネイルは含みません。">
        <button className="button" type="button" disabled={busy} onClick={() => void exportData()}>エクスポート</button>
      </SettingRow>
    </>
  );
}
