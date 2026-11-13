import { useEffect, useState } from "react";
import { api, listenProgress } from "../api";
import { useShell } from "../App";
import type { LibraryWalkReport } from "../types";
import { errorText } from "../ui";
import { SettingRow } from "./SettingRow";

/** Import page actions for the built-in browser: open, import the current page, and walk the library. */
export function BuiltinBrowserImport() {
  const { refresh, notify, fail, refreshAccount } = useShell();
  const [busy, setBusy] = useState(false);
  const [walking, setWalking] = useState(false);
  const [walkMessage, setWalkMessage] = useState("");
  const [walkReport, setWalkReport] = useState<LibraryWalkReport | null>(null);

  useEffect(() => {
    let alive = true;
    let stop = () => {};
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
  }, []);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function walk() {
    setWalking(true);
    setBusy(true);
    setWalkReport(null);
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
    }
  }

  return (
    <>
      <SettingRow id="import-builtin-browser" label="内蔵ブラウザ" note="BOOTH へのログインと年齢確認は、開いたブラウザで行います。">
        <div className="row-actions">
          <button className="button secondary" type="button" disabled={busy} onClick={() => void run(() => api.openBoothBrowser())}>
            ブラウザを開く
          </button>
          <button className="button secondary" type="button" disabled={busy} onClick={() => void run(() => api.showBoothLibrary())}>
            ライブラリを開く
          </button>
        </div>
      </SettingRow>
      <SettingRow
        id="import-library-walk"
        label="商品情報を取得"
        help="「ライブラリ全取得」はライブラリ、ギフト、無料ダウンロードを読み込みます。"
        note="ファイルはダウンロードしません。"
      >
        <div className="row-actions">
        <button className="button primary" type="button" disabled={busy} onClick={() => void walk()}>
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
        </div>
      </SettingRow>
      {((walking && walkMessage) || walkReport) && <div className="section-body">
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
      </div>}
    </>
  );
}

/** Settings for the built-in browser's library walk: access interval and diagnostics. */
export function BuiltinBrowserAccessSettings() {
  const { notify, fail } = useShell();
  const [intervalMs, setIntervalMs] = useState(1000);
  const [intervalReady, setIntervalReady] = useState(false);
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
    let alive = true;
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
    return () => {
      alive = false;
    };
  }, []);

  async function changeInterval(value: number) {
    if (!intervalReady || value === intervalMs) return;
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

  return (
    <div className="section-body builtin-browser-access">
      <p className="meta">
        ログインと年齢確認は、開いたブラウザで行ってください。
        インポート画面の「ライブラリ全取得」はライブラリ、ギフト、無料ダウンロードの商品情報を読み込みます。ファイルはダウンロードしません。
        公開の商品情報は1件ずつ、3秒以上あけて確認します。年齢制限の商品は確認しません。
      </p>
      <label className="field" id="settings-booth-interval" style={{ maxWidth: 300, marginBottom: 8 }}>
        <span>BOOTHへのアクセス間隔（ライブラリ全取得・購入履歴）</span>
        <select
          value={intervalMs}
          disabled={!intervalReady}
          onChange={(event) => void changeInterval(Number(event.target.value))}
        >
          <option value={1000}>1.0秒（推奨）</option>
          <option value={500}>0.5秒（やや速い）</option>
          <option value={250}>0.25秒（高リスク）</option>
        </select>
      </label>
      <p className="meta">短い間隔はアクセス制限やアカウントへの影響の可能性があります。高速設定でも制限を回避するものではありません。</p>
      <details className="settings-details" id="settings-browser-troubleshooting" onToggle={(event) => { if (event.currentTarget.open) void refreshLog(); }}>
        <summary>内蔵ブラウザのトラブルシューティング</summary>
        <div className="row-actions" style={{ marginTop: 12 }}>
          <button className="button" type="button" onClick={() => void refreshLog()}>
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
  );
}

