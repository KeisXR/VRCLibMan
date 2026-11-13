import { useCallback, useEffect, useState } from "react";
import { api, chooseUnityEditor } from "../api";
import type { UnityEditor, UnityStatus } from "../types";
import { errorText } from "../ui";
import type { NotifyOptions } from "../notice";
import { SettingRow } from "./SettingRow";

const SOURCE_LABEL: Record<UnityEditor["source"], string> = {
  hub: "Unity Hub",
  folder: "インストール先",
  manual: "手動で指定",
};

export function unityEditorLabel(editor: UnityEditor): string {
  return `${editor.version || "バージョン不明"}（${SOURCE_LABEL[editor.source]}）`;
}

/** 設定の「Unity」セクションの中身。検出はファイルを読むだけで、Unity は起動しない。 */
export function UnitySettings({ onNotify, onFail }: { onNotify: (message: string, action?: undefined, options?: NotifyOptions) => void; onFail: (message: string) => void }) {
  const [status, setStatus] = useState<UnityStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const detect = useCallback(async (announce: boolean) => {
    setBusy(true);
    try {
      const next = await api.unityEditors();
      setStatus(next);
      if (announce) {
        if (next.editors.length) onNotify(`Unity を ${next.editors.length} 件見つけました`);
        else onNotify("Unity が見つかりませんでした", undefined, { persist: true });
      }
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }, [onNotify, onFail]);

  useEffect(() => { void detect(false); }, [detect]);

  async function select(path: string) {
    setBusy(true);
    try {
      setStatus(await api.setUnityEditor(path));
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function browse() {
    try {
      const path = await chooseUnityEditor();
      if (!path) return;
      await select(path);
    } catch (err) {
      onFail(errorText(err));
    }
  }

  const editors = status?.editors ?? [];
  const selected = status?.selectedPath ?? "";
  const summary = status == null
    ? "確認中…"
    : editors.length
      ? `${editors.length} 件見つかりました`
      : "見つかりませんでした";

  return (
    <>
      <SettingRow id="settings-unity-editor" label="既定の Unity" help="VRChat のプロジェクトで使っているバージョンを選んでください。">
        <select value={selected} disabled={busy || status == null} aria-label="既定の Unity" onChange={(e) => void select(e.target.value)}>
          <option value="">未設定</option>
          {status?.selectedMissing && <option value={selected}>見つかりません（{selected}）</option>}
          {editors.map((editor) => <option key={editor.path} value={editor.path}>{unityEditorLabel(editor)}</option>)}
        </select>
        {selected && <p className={status?.selectedMissing ? "meta warn-text unity-path" : "meta unity-path"}>{status?.selectedMissing ? "保存した Unity が見つかりません。選び直してください。" : selected}</p>}
      </SettingRow>
      <SettingRow id="settings-unity-detect" label="Unity を探す" help="Unity Hub の記録と、既定のインストール先を調べます。見つからないときは「参照…」で Unity 本体を選びます。">
        <div className="inline-control">
          <span className="meta" role="status">{summary}</span>
          <button className="button" type="button" disabled={busy} onClick={() => void detect(true)}>再検出</button>
          <button className="button" type="button" disabled={busy} onClick={() => void browse()}>参照…</button>
        </div>
      </SettingRow>
      <details className="settings-details">
        <summary>Unity の検出について</summary>
        <p>Unity Hub が記録しているエディタ（editors-v2.json など）と、Hub の既定・追加のインストール先を読みます。Unity Hub や Unity は起動しません。</p>
        <p>「参照…」では、Windows は Unity.exe、Mac は Unity.app、Linux は Unity を選びます。</p>
        {status && status.searched.length > 0 && <p>調べた場所：{status.searched.join("、")}</p>}
        {status && !status.hubFound && <p>Unity Hub の設定フォルダが見つかりませんでした。</p>}
      </details>
    </>
  );
}
