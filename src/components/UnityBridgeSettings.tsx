import { useEffect, useState } from "react";
import { api, revealInFolder } from "../api";
import type { UnityBridgeStatus } from "../types";
import { errorText } from "../ui";
import type { NotifyOptions } from "../notice";
import { SettingRow } from "./SettingRow";

/**
 * 設定の「Unity」節の中の「Unity の中から使う」（docs/unity-projects.md の段階 4）。
 * オンにすると、同梱のエディタ拡張をフォルダに用意し、Unity の窓からの問い合わせを受ける。
 */
export function UnityBridgeSettings({ onNotify, onFail }: { onNotify: (message: string, action?: undefined, options?: NotifyOptions) => void; onFail: (message: string) => void }) {
  const [status, setStatus] = useState<UnityBridgeStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    api.unityBridgeStatus().then((next) => { if (live) setStatus(next); }, (err) => onFail(errorText(err)));
    return () => { live = false; };
  }, [onFail]);

  async function toggle(enabled: boolean) {
    setBusy(true);
    try {
      setStatus(await api.setUnityBridge(enabled));
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function regenerate() {
    if (!window.confirm("接続用のコードを作り直します。開いている Unity の窓は、次の操作から新しいコードを使います。")) return;
    setBusy(true);
    try {
      await api.regenerateUnityBridgeToken();
      onNotify("接続用のコードを作り直しました");
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <SettingRow
        id="settings-unity-bridge"
        label="Unity の中から使う"
        help="Unity のメニュー「Tools > VRCLibMan」に窓を出し、開いているプロジェクトに入っている商品と、まだ入っていない対応アセットを見て、Import 画面で取り込めます。"
        note={status?.enabled ? "このアプリを起動している間だけ使えます。" : undefined}
      >
        <label className="switch-label"><input aria-label="Unity の中から使う" type="checkbox" disabled={busy || status == null} checked={status?.enabled ?? false} onChange={(e) => void toggle(e.target.checked)} /></label>
      </SettingRow>
      {status?.enabled && <>
        {status.error && <p className="note-strong">{status.error}</p>}
        {status.folder && (
          <SettingRow
            id="settings-unity-bridge-folder"
            label="エディタ拡張のフォルダ"
            help={`ALCOM の設定の「パッケージ」→「ユーザーパッケージ」の「追加」で、このフォルダを選びます。そのあと、プロジェクトの「管理」で「VRCLibMan」（${status.version}）を追加してください。アプリを更新すると、このフォルダも新しい版になります。`}
            note={status.folder}
          >
            <button className="button secondary" type="button" onClick={() => void revealInFolder(status.folder).catch((err: unknown) => onFail(errorText(err)))}>フォルダを表示</button>
          </SettingRow>
        )}
        <SettingRow id="settings-unity-bridge-code" label="接続用のコード" help="Unity の窓は、このアプリが用意したコードで接続します。作り直すと、開いている Unity の窓は次の操作から新しいコードを使います。">
          <button className="button secondary" type="button" disabled={busy} onClick={() => void regenerate()}>接続用のコードを作り直す</button>
        </SettingRow>
        <details className="settings-details" id="settings-unity-bridge-about">
          <summary>Unity の窓でできること</summary>
          <p>入っている商品と、版や改変の知らせ、結び付けたアバターに対応していてまだ入っていない商品を並べます。判定はこのアプリの「プロジェクト」と同じです。</p>
          <p>「取り込む…」で手元のファイルの中のパッケージを選ぶと、足りないシェーダーやツールを確かめてから、Unity の Import 画面を開きます。取り込むかどうかは Import 画面で決めます。このアプリはプロジェクトのフォルダに書き込みません。</p>
          <p>ダウンロードはこのアプリで行います。ブラウザやほかのアプリのページからは使えません。</p>
        </details>
      </>}
    </>
  );
}
