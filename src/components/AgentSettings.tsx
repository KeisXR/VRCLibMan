import { useEffect, useState } from "react";
import { api } from "../api";
import type { McpStatus } from "../types";
import { errorText } from "../ui";
import type { NotifyOptions } from "../notice";
import { SettingRow } from "./SettingRow";
import { AgentClients } from "./AgentClients";

/** 設定の「Unity」節の中の「AI エージェント連携」。Unity を操作中の AI エージェントがライブラリを検索できる。 */
export function AgentSettings({ onNotify, onFail }: { onNotify: (message: string, action?: undefined, options?: NotifyOptions) => void; onFail: (message: string) => void }) {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    api.mcpStatus().then((next) => { if (live) setStatus(next); }, (err) => onFail(errorText(err)));
    return () => { live = false; };
  }, [onFail]);

  async function save(change: Partial<Pick<McpStatus, "enabled" | "includeAdult" | "allowDownload">>) {
    if (!status) return;
    const next = { ...status, ...change };
    setBusy(true);
    try {
      setStatus(await api.setMcpSettings(next.enabled, next.includeAdult, next.allowDownload));
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function regenerate() {
    if (!window.confirm("接続用のコードを作り直します。つながっている AI エージェントは、次の問い合わせから新しいコードを使います。")) return;
    setBusy(true);
    try {
      await api.regenerateMcpToken();
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
        id="settings-ai-agent"
        label="AI エージェント連携"
        help="Unity を操作中の AI エージェント（Claude Code、Cursor など）が、ライブラリの商品・対応アバター・手元のファイルの場所を調べられます。読み取りだけで、ライブラリを変えたりダウンロードしたりはしません。"
        note={status?.enabled ? "このアプリを起動している間だけ使えます。" : undefined}
      >
        <label className="switch-label"><input aria-label="AI エージェント連携" type="checkbox" disabled={busy || status == null} checked={status?.enabled ?? false} onChange={(e) => void save({ enabled: e.target.checked })} /></label>
      </SettingRow>
      {status?.enabled && <>
        <SettingRow id="settings-ai-agent-adult" label="R-18 の商品も含める" help="オフのときは、年齢制限のある商品を AI エージェントに見せません。オンでも説明文は渡しません。">
          <label className="switch-label"><input aria-label="R-18 の商品も含める" type="checkbox" disabled={busy} checked={status.includeAdult} onChange={(e) => void save({ includeAdult: e.target.checked })} /></label>
        </SettingRow>
        <SettingRow
          id="settings-ai-agent-download"
          label="AI エージェントからのダウンロードを許可"
          help="オンにすると、AI エージェントが会話であなたの同意を得たうえで、持っている商品のファイルを 1 回に 1 つダウンロードできます。画面の「ダウンロード」と同じ条件で受け取り、サイドバーのダウンロード一覧にも出ます。年齢制限のある商品と除外した商品は対象外です。"
          note={status.allowDownload ? "同意は AI エージェントとの会話で取ります。このアプリは確認の画面を出しません。" : undefined}
        >
          <label className="switch-label"><input aria-label="AI エージェントからのダウンロードを許可" type="checkbox" disabled={busy} checked={status.allowDownload} onChange={(e) => void save({ allowDownload: e.target.checked })} /></label>
        </SettingRow>
        <AgentClients onNotify={onNotify} onFail={onFail} />
        <SettingRow id="settings-ai-agent-config" label="接続用のコード" help="AI エージェントは、このアプリが用意したコードで接続します。作り直すと、つながっているエージェントは次の問い合わせから新しいコードを使います。">
          <button className="button secondary" type="button" disabled={busy} onClick={() => void regenerate()}>接続用のコードを作り直す</button>
        </SettingRow>
        <details className="settings-details" id="settings-ai-agent-about">
          <summary>AI エージェントに渡すもの</summary>
          <p>渡すのは、商品名・ショップ・種類とジャンル・対応アバター・説明の冒頭、手元にあるファイルの場所と、zip に入っている unitypackage の名前です。ファイルの場所は、ライブラリとリモートライブラリのフォルダの中にあるものだけ渡します。</p>
          <p>除外した商品、購入金額、BOOTH のログイン状態は渡しません。ブラウザやほかのアプリのページからは使えません。</p>
        </details>
      </>}
    </>
  );
}
