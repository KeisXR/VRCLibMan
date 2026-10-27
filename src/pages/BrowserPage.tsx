import { useEffect, useState } from "react";
import { api, listenProgress } from "../api";
import { useShell } from "../App";
import type { ImportReport, LibraryWalkReport } from "../types";
import { errorText } from "../ui";
import { BrowserChrome } from "../components/BrowserChrome";

export function BrowserPage({ enabled }: { enabled: boolean }) {
  const { refresh, notify, fail, refreshAccount } = useShell();
  const [busy, setBusy] = useState(false);
  const [walking, setWalking] = useState(false);
  const [walkMessage, setWalkMessage] = useState("");
  const [intervalMs, setIntervalMs] = useState(1000);
  const [intervalReady, setIntervalReady] = useState(false);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [walkReport, setWalkReport] = useState<LibraryWalkReport | null>(null);
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
    if (!enabled) {
      setIntervalReady(false);
      return;
    }
    setIntervalReady(false);
    let alive = true;
    let stop = () => {};
    void refreshLog();
    void api.boothPageInterval().then(
      (value) => {
        if (alive) {
          setIntervalMs(value);
          setIntervalReady(true);
        }
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    void listenProgress((progress) => {
      if (alive && progress.job === "browser-library") setWalkMessage(progress.message);
    }).then((unlisten) => {
      if (!alive) unlisten();
      else stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, [enabled]);

  async function changeInterval(value: number) {
    if (busy || !intervalReady || value === intervalMs) return;
    if (value === 250 && intervalMs !== 250 &&
        !window.confirm("0.25秒間隔はBOOTHのアクセス制限やアカウントへの影響が発生し得る高リスク設定です。規約上の安全性は保証されません。変更しますか？")) {
      return;
    }
    try {
      await api.setBoothPageInterval(value);
      setIntervalMs(value);
      notify(`内蔵ブラウザのアクセス間隔を ${value / 1000} 秒に変更しました`);
    } catch (err) {
      fail(errorText(err));
    }
  }

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
          <p>BOOTHのライブラリ、ギフト、無料ダウンロードの商品情報を読み込みます。</p>
        </div>
      </div>
      {!enabled && (
        <p className="note">
          内蔵ブラウザはオフです。設定で有効にできます。
        </p>
      )}
      {enabled && (
        <div className="panel">
          <p className="meta">
            ログインと年齢確認は、開いたブラウザで行ってください。
            「ライブラリ全取得」はライブラリ、ギフト、無料ダウンロードの商品情報を読み込みます。ファイルはダウンロードしません。
            公開の商品情報は1件ずつ、3秒以上あけて確認します。年齢制限の商品は確認しません。
          </p>
          <details className="settings-details">
            <summary>アクセス間隔</summary>
            <label className="field" style={{ maxWidth: 300, marginBottom: 8 }}>
              <span>BOOTHへのアクセス間隔（ライブラリ全取得）</span>
              <select
                value={intervalMs}
                disabled={busy || !intervalReady}
                onChange={(event) => void changeInterval(Number(event.target.value))}
              >
                <option value={1000}>1.0秒（推奨）</option>
                <option value={500}>0.5秒（やや速い）</option>
                <option value={250}>0.25秒（高リスク）</option>
              </select>
            </label>
            <p className="meta">短い間隔はアクセス制限やアカウントへの影響の可能性があります。高速設定でも制限を回避するものではありません。</p>
          </details>
          <div className="row-actions">
            <button className="button primary" type="button" disabled={busy} onClick={() => void run(() => api.openBoothBrowser())}>
              ブラウザを開く
            </button>
            <button className="button" type="button" disabled={busy} onClick={() => void run(() => api.showBoothLibrary())}>
              ライブラリを開く
            </button>
          </div>
          <BrowserChrome embedded onImported={() => { refresh(); refreshAccount(); }} />
          <div className="row-actions" style={{ marginTop: 12 }}>
            <button
              className="button primary"
              type="button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  setReport(null);
                  setWalkReport(null);
                  const result = await api.importBoothPage();
                  setReport(result);
                  notify(`${result.inserted} 件追加、${result.updated} 件更新しました`);
                  refresh();
                  refreshAccount();
                })
              }
            >
              今開いているページをインポート
            </button>
            <button
              className="button"
              type="button"
              disabled={busy}
              onClick={() =>
                void (async () => {
                  setWalking(true);
                  setBusy(true);
                  setWalkReport(null);
                  setReport(null);
                  try {
                    const result = await api.importBoothLibrary();
                    setWalkReport(result);
                    const counts = `追加 ${result.inserted} 件、更新 ${result.updated} 件`;
                    if (result.notice) fail(`${counts}。${result.notice}`);
                    else if (result.cancelled) notify(`中止しました。${counts}`);
                    else notify(counts);
                    refresh();
                    refreshAccount();
                  } catch (err) {
                    fail(errorText(err));
                  } finally {
                    setBusy(false);
                    setWalking(false);
                    void refreshLog();
                  }
                })()
              }
            >
              ライブラリ全取得
            </button>
            <button
              className="button"
              type="button"
              disabled={!walking}
              onClick={() => void api.cancelBoothLibrary().catch((err) => fail(errorText(err)))}
            >
              中止
            </button>
            <button
              className="button danger"
              type="button"
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  if (!window.confirm("ブラウザを閉じ、内蔵ブラウザで開いたすべてのサイトのログイン状態と閲覧データを消します。ブックマークは残ります。")) return;
                  await api.clearBoothSession();
                  notify("内蔵ブラウザの閲覧データを削除しました");
                })
              }
            >
              内蔵ブラウザの閲覧データを削除
            </button>
          </div>
          {walking && walkMessage && <p className="meta">{walkMessage}</p>}
          {walkReport && (
            <>
              <p className="meta">
                追加 {walkReport.inserted} 件、更新 {walkReport.updated} 件
                {walkReport.cancelled ? "。中止しました" : ""}
                {walkReport.notice ? `。${walkReport.notice}` : ""}
              </p>
              <details className="settings-details">
                <summary>読み込み結果の詳細</summary>
                <p className="meta">
                  商品情報 {walkReport.products} 件、変更なし {walkReport.unchanged} 件、
                  ファイル情報 {walkReport.files} 件（ファイル名あり {walkReport.namedFiles} 件、未取得 {walkReport.unnamedFiles} 件）
                </p>
              </details>
            </>
          )}
          {report && (
            <>
              <p className="meta">
                追加 {report.inserted} 件、更新 {report.updated} 件
                {report.skipped > 0 ? `。読み込めなかった商品 ${report.skipped} 件` : ""}
                {report.warnings.length > 0 ? `。${report.warnings.join("。")}` : ""}
              </p>
              <details className="settings-details">
                <summary>読み込み結果の詳細</summary>
                <p className="meta">
                  変更なし {report.unchanged} 件、バリエーション {report.variations} 件、ファイル情報 {report.downloads} 件
                  {report.unnamedDownloads > 0 ? `（ファイル名未取得 ${report.unnamedDownloads} 件）` : ""}
                </p>
              </details>
            </>
          )}
          <details className="settings-details">
            <summary>トラブルシューティング</summary>
            <div className="row-actions" style={{ marginTop: 12 }}>
              <button className="button" type="button" disabled={busy} onClick={() => void refreshLog()}>
                診断情報を更新
              </button>
            </div>
            <p className="meta">
              記録にパスワードやページのクエリは入れません。
              {logPath ? ` ファイルは ${logPath} です。` : ""}
            </p>
            <pre className="browser-log">{logText || "まだ記録はありません"}</pre>
          </details>
        </div>
      )}
    </section>
  );
}
