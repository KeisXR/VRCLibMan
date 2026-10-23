import { useEffect, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import type { ImportReport } from "../types";
import { errorText } from "../ui";

export function BrowserPage({ enabled }: { enabled: boolean }) {
  const { refresh, notify, fail } = useShell();
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [logPath, setLogPath] = useState("");
  const [logText, setLogText] = useState("");

  async function refreshLog() {
    try {
      const diagnostic = await api.boothBrowserLog();
      setLogPath(diagnostic.path);
      setLogText(diagnostic.text);
    } catch (err) {
      setLogText(errorText(err));
    }
  }

  useEffect(() => {
    if (!enabled) return;
    void refreshLog();
  }, [enabled]);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
      if (enabled) void refreshLog();
    }
  }

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>ブラウザ</h1>
          <p>自分で BOOTH にログインし、今見えているライブラリのページだけを取り込みます。</p>
        </div>
      </div>
      {!enabled && (
        <p className="note">
          内蔵ブラウザはオフです。設定で有効にすると使えます。既定の取り込みは拡張機能のままです。
        </p>
      )}
      {enabled && (
        <div className="panel">
          <p className="meta">
            ブラウザは別ウィンドウで開きます。ログイン、ページ移動、年齢確認は自分で行ってください。
            この画面はページを自動で辿りません。ダウンロードの記録は、次の段階です。
          </p>
          <div className="row-actions">
            <button className="button primary" type="button" disabled={busy} onClick={() => void run(() => api.openBoothBrowser())}>
              ブラウザを開く
            </button>
            <button className="button" type="button" disabled={busy} onClick={() => void run(() => api.showBoothLibrary())}>
              ライブラリを開く
            </button>
            <button className="button" type="button" disabled={busy} onClick={() => void run(() => api.browseBoothHistory(false))}>
              戻る
            </button>
            <button className="button" type="button" disabled={busy} onClick={() => void run(() => api.browseBoothHistory(true))}>
              進む
            </button>
          </div>
          <div className="row-actions" style={{ marginTop: 12 }}>
            <button
              className="button primary"
              type="button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  const result = await api.importBoothPage();
                  setReport(result);
                  notify(`${result.inserted} 件追加、${result.updated} 件更新しました`);
                  refresh();
                })
              }
            >
              今開いているページを取り込む
            </button>
            <button
              className="button danger"
              type="button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  if (!window.confirm("ブラウザを閉じ、このアプリの中の BOOTH のログイン状態を消します。")) return;
                  await api.clearBoothSession();
                  notify("セッションを消しました");
                })
              }
            >
              ログアウトしてセッションを消す
            </button>
          </div>
          {report && (
            <p className="meta">
              新規 {report.inserted}、更新 {report.updated}、変更なし {report.unchanged}
              、バリエーション {report.variations}、ダウンロード {report.downloads}
              {report.skipped > 0 ? `、スキップ ${report.skipped}` : ""}
              {report.warnings.length > 0 ? `。${report.warnings[0]}` : ""}
            </p>
          )}
          <div className="row-actions" style={{ marginTop: 12 }}>
            <button className="button" type="button" disabled={busy} onClick={() => void refreshLog()}>
              診断を更新
            </button>
          </div>
          <p className="meta">
            記録にパスワードやページのクエリは入れません。
            {logPath ? ` ファイルは ${logPath} です。` : ""}
          </p>
          <pre className="browser-log">{logText || "まだ記録はありません"}</pre>
        </div>
      )}
    </section>
  );
}
