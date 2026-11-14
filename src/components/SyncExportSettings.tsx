import { useState } from "react";
import { exportSyncData } from "../api";
import type { Settings } from "../types";
import { errorText } from "../ui";
import { SettingRow } from "./SettingRow";

/** 端末名とデータの書き出し。取り込みと共有フォルダへの自動書き込みはここに置かない。 */
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
      const { products, links, avatars, fileLinks, config, thumbnails, heroImages, purchaseOrders } = result.counts;
      onNotify(`書き出しました（商品 ${products} 件、リンク ${links} 件、アバター ${avatars} 件、ファイル ${fileLinks} 件、設定 ${config} 件、サムネイル ${thumbnails} 件、ヒーロー画像 ${heroImages} 件、購入履歴の注文 ${purchaseOrders ?? 0} 件）`);
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <SettingRow id="settings-device-name" label="この端末の名前" help="書き出す ZIP に記録する名前です。ほかの PC から書き出したものと見分けるために使います。">
        <input aria-label="この端末の名前" value={settings.deviceName} maxLength={64} onChange={(e) => onChange({ deviceName: e.target.value }, 300)} onBlur={onBlur} />
      </SettingRow>
      <SettingRow
        id="settings-data-export"
        label="データの書き出し"
        help="商品情報、サムネイル、見出し画像、分類、対応アバター、所持アバター、ファイルとの対応、購入履歴（注文、金額、ラベル）、端末に依存しない設定を ZIP に書き出します。商品名、説明文、画像、ファイル名を含みます。パス、APIキー、ログイン情報は含みません。"
        note="この版には、書き出した ZIP をアプリに取り込む機能はまだありません。別の PC へ移すときは、アプリを終了してアプリデータのフォルダをコピーしてください（使い方ガイドの「データの保存とバックアップ」）。"
      >
        <button className="button" type="button" disabled={busy} onClick={() => void exportData()}>ZIP に書き出す</button>
      </SettingRow>
    </>
  );
}
