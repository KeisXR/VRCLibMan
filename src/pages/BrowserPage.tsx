import { useEffect, useState } from "react";
import { api, listenProgress } from "../api";
import { useShell } from "../App";
import type { ImportReport, LibraryWalkReport } from "../types";
import { errorText } from "../ui";

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
    if (!enabled) return;
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
          <p>自分で BOOTH にログインし、今のページか、ライブラリとギフトを1ページずつ取り込みます。</p>
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
            ブラウザは別ウィンドウで開きます。ログインと年齢確認は自分で行ってください。
            「ライブラリ全取得」はライブラリのあとギフトを、1ページずつ間隔を空けて読みます。ファイルはダウンロードしません。
          </p>
          <label className="field" style={{ maxWidth: 300, marginBottom: 8 }}>
            <span>BOOTHへのアクセス間隔（ライブラリ全取得）</span>
            <select
              value={intervalMs}
              disabled={busy || !intervalReady}
              onChange={(event) => void changeInterval(Number(event.target.value))}
            >
              <option value={1000}>1.0秒（推奨・初期値）</option>
              <option value={500}>0.5秒（やや速い）</option>
              <option value={250}>0.25秒（高リスク）</option>
            </select>
          </label>
          <p className="meta">短い間隔はアクセス制限やアカウントへの影響の可能性があります。高速設定でも制限を回避するものではありません。</p>
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
                  refreshAccount();
                })
              }
            >
              今開いているページを取り込む
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
                  try {
                    const result = await api.importBoothLibrary();
                    setWalkReport(result);
                    const counts = `商品 ${result.products}、ファイル ${result.files}、名前あり ${result.namedFiles}、名前なし ${result.unnamedFiles}`;
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
                  if (!window.confirm("ブラウザを閉じ、このアプリの中の BOOTH のログイン状態を消します。")) return;
                  await api.clearBoothSession();
                  notify("セッションを消しました");
                })
              }
            >
              ログアウトしてセッションを消す
            </button>
          </div>
          {walking && walkMessage && <p className="meta">{walkMessage}</p>}
          {walkReport && (
            <p className="meta">
              商品 {walkReport.products}、ファイル {walkReport.files}、名前あり {walkReport.namedFiles}、名前なし{" "}
              {walkReport.unnamedFiles}
              {walkReport.cancelled ? "。中止しました" : ""}
              {walkReport.notice ? `。${walkReport.notice}` : ""}
              。新規 {walkReport.inserted}、更新 {walkReport.updated}、変更なし {walkReport.unchanged}
            </p>
          )}
          {report && (
            <p className="meta">
              新規 {report.inserted}、更新 {report.updated}、変更なし {report.unchanged}
              、バリエーション {report.variations}、ダウンロード {report.downloads}
              {report.unnamedDownloads > 0 ? `（名前なし ${report.unnamedDownloads}）` : ""}
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
