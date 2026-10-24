import { useEffect, useState } from "react";
import {
  api,
  chooseFolder,
  isTauri,
  listenProgress,
  saveLibraryAndScan,
} from "../api";
import { useShell } from "../App";
import type {
  ImportReport,
  StagedDownload,
  IntegrationStatus,
  JobProgress,
} from "../types";
import { errorText } from "../ui";
import { SettingRow } from "../components/SettingRow";
import { ExtensionInstructions } from "../components/BoothImportSettings";

export function ImportPage() {
  const { revision, refresh, notify, fail, builtinBrowser } = useShell();
  const [libraryFolder, setLibraryFolder] = useState("");
  const [over, setOver] = useState(false);
  const [extractAfter, setExtractAfter] = useState(isTauri());
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [freeUrl, setFreeUrl] = useState("");
  const [integration, setIntegration] = useState<IntegrationStatus | null>(
    null,
  );
  const [unextracted, setUnextracted] = useState<number | null>(null);
  const [progress, setProgress] = useState<JobProgress | null>(null);
  const [extracting, setExtracting] = useState(false);
  const [receiverStatus, setReceiverStatus] = useState("確認中");
  const [staged, setStaged] = useState<StagedDownload[]>([]);

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      api.integrationStatus(),
      api.stats(),
      api.extensionLink(),
    ]).then(
      ([next, stats, link]) => {
        if (cancelled) return;
        setIntegration(next);
        setUnextracted(stats.unextracted);
        setReceiverStatus(
          !link.listening
            ? link.error || "受信できません"
            : next.extensionLastSeenAt
              ? "拡張から通信を受信済み（現在の接続状態は未確認）"
              : next.paired
                ? "ペアリング済み・通信は未確認"
                : "受信待機中・未ペアリング",
        );
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    api.settings().then(
      (value) => {
        if (!cancelled) setLibraryFolder(value.libraryFolder);
      },
      () => undefined,
    );
    api.stagedDownloads().then(
      (value) => {
        if (!cancelled) setStaged(value);
      },
      () => {
        if (!cancelled) setStaged([]);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision]);

  useEffect(() => {
    let alive = true;
    let stop = () => {};
    void listenProgress((next) => {
      if (alive && next.job === "extract") setProgress(next);
    }).then((unlisten) => {
      if (!alive) unlisten();
      else stop = unlisten;
    });
    return () => {
      alive = false;
      stop();
    };
  }, []);

  async function importText(json: string) {
    setBusy(true);
    try {
      const result = await api.importExport(json);
      setReport(result);
      notify(`${result.inserted} 件追加、${result.updated} 件更新しました`);
      refresh();
      if (extractAfter && result.inserted + result.updated > 0) {
        setExtracting(true);
        const extracted = await api.extractProducts(false);
        notify(
          extracted.cancelled
            ? "抽出を中断しました"
            : `${extracted.processed} 件抽出し、${extracted.failed} 件失敗しました`,
        );
        if (extracted.errors.length > 0) fail(extracted.errors.join(" / "));
        refresh();
      }
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
      setExtracting(false);
      setProgress(null);
    }
  }

  async function onFile(file: File | undefined) {
    if (!file) return;
    try {
      await importText(await file.text());
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function scan() {
    try {
      const path = libraryFolder || (await chooseFolder());
      if (!path) return;
      setBusy(true);
      const result = await saveLibraryAndScan(path);
      setLibraryFolder(path);
      notify(
        `設定と同じライブラリのフォルダを保存し、${result.indexed} 件を調べ、${result.linked} 件を商品と結び付けました${result.truncated ? "。上限に達したため途中です" : ""}`,
      );
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function extract(force: boolean) {
    if (
      force &&
      !window.confirm(
        "すべて再分類すると、既存の分類結果を上書きすることがあります。手動で固定した情報は維持します。続けますか？",
      )
    )
      return;
    setBusy(true);
    setExtracting(true);
    setProgress(null);
    try {
      const result = await api.extractProducts(force);
      notify(
        result.cancelled
          ? "抽出を中断しました"
          : `${result.processed} 件抽出し、${result.failed} 件失敗しました`,
      );
      if (result.errors.length > 0) fail(result.errors.join(" / "));
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
      setExtracting(false);
      setProgress(null);
    }
  }

  async function routeStaged(path: string) {
    setBusy(true);
    try {
      await api.routeStagedDownload(path);
      notify("ファイルをライブラリへ移しました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function cache() {
    setBusy(true);
    try {
      const saved = await api.cacheThumbnails();
      notify(`${saved} 件のサムネイルを保存しました`);
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="import-page">
      <div className="page-head">
        <div>
          <h1>インポート</h1>
          <p>
            BOOTH から読み込む → 分類 →
            手元のファイルと結び付ける、の順に進めます。ユーザーが開いたページや、ボタンで指定した範囲だけを読み込みます。
          </p>
        </div>
      </div>
      {!isTauri() && (
        <p className="note">
          いまはブラウザ確認用の見本です。抽出、フォルダスキャン、Ollama、サムネイル保存はデスクトップアプリで動きます。
        </p>
      )}
      <div className="settings-sections">
        <section className="settings-section">
          <header>
            <h2>1. BOOTH から読み込む</h2>
            <p>
              拡張か内蔵ブラウザで商品情報を送ります。受け取った商品は、次の「分類」で整理できます。
            </p>
          </header>
          <SettingRow label="拡張機能" help={receiverStatus}>
            <a className="button" href="#/settings">
              読み込み方とペアリングの設定
            </a>
          </SettingRow>
          {integration?.extensionLastSeenAt && (
            <p className="section-note">
              最後の通信:{" "}
              {new Date(integration.extensionLastSeenAt).toLocaleString(
                "ja-JP",
              )}
            </p>
          )}
          {integration?.extensionImport && (
            <p className="section-note">
              最後の受信:{" "}
              {new Date(integration.extensionImport.at).toLocaleString("ja-JP")}{" "}
              ・{integration.extensionImport.products} 件（追加{" "}
              {integration.extensionImport.inserted} / 更新{" "}
              {integration.extensionImport.updated} / 変化なし{" "}
              {integration.extensionImport.unchanged}）
            </p>
          )}
          <details className="settings-details">
            <summary>拡張の使い方</summary>
            <ExtensionInstructions />
          </details>
          <SettingRow
            label="内蔵ブラウザ"
            help={
              builtinBrowser
                ? "有効です。別ウィンドウで BOOTH にログインして読み込みます。"
                : "無効です。設定で読み込み方を変更できます。"
            }
          >
            <a
              className="button"
              href={builtinBrowser ? "#/browser" : "#/settings"}
            >
              {builtinBrowser ? "内蔵ブラウザを開く" : "読み込み方を選ぶ"}
            </a>
          </SettingRow>
          {integration?.browserImport && (
            <p className="section-note">
              内蔵ブラウザの最終読み込み:{" "}
              {new Date(integration.browserImport.at).toLocaleString("ja-JP")}{" "}
              ・{integration.browserImport.products} 件
            </p>
          )}
          {report && (
            <div className="section-body">
              <strong>直前の JSON インポート結果</strong>
              <p
                className={
                  report.variations === 0 && report.downloads === 0
                    ? "meta warn-text"
                    : "meta"
                }
              >
                追加 {report.inserted} / 更新 {report.updated} / 変化なし{" "}
                {report.unchanged} / スキップ {report.skipped}
                {" / "}バリエーション {report.variations} / ダウンロード{" "}
                {report.downloads}
                {report.unnamedDownloads > 0
                  ? `（名前なし ${report.unnamedDownloads}）`
                  : ""}
                {report.warnings.length > 0
                  ? `。${report.warnings.slice(0, 3).join(" ")}`
                  : ""}
              </p>
            </div>
          )}
        </section>
        <section className="settings-section">
          <header>
            <h2>2. 分類</h2>
            <p>商品説明からアバター名とカテゴリを整理します。</p>
          </header>
          <SettingRow
            label={
              unextracted == null
                ? "未抽出の件数を確認中"
                : `未抽出 ${unextracted} 件`
            }
            help="分類モデルは設定で選べます。"
          >
            <div className="row-actions">
              <button
                className="button primary"
                type="button"
                disabled={busy || unextracted === 0}
                onClick={() => void extract(false)}
              >
                未抽出を分類
              </button>
              <button
                className="button"
                type="button"
                disabled={!extracting}
                onClick={() =>
                  void api
                    .cancelExtract()
                    .catch((err: unknown) => fail(errorText(err)))
                }
              >
                中断
              </button>
            </div>
          </SettingRow>
          {progress && (
            <div className="section-body" role="status">
              {progress.message}（{progress.current} / {progress.total}）
            </div>
          )}
          <details className="settings-details">
            <summary>すべて再分類</summary>
            <p>
              既存の分類結果を上書きすることがあります。実行前に確認します。手動で固定した情報は維持します。
            </p>
            <button
              className="button"
              type="button"
              disabled={busy}
              onClick={() => void extract(true)}
            >
              すべて再分類
            </button>
          </details>
        </section>
        <section className="settings-section">
          <header>
            <h2>3. 手元のファイル</h2>
            <p>
              ライブラリのフォルダを調べ、ダウンロード済みのファイルを商品と結び付けます。
            </p>
          </header>
          <SettingRow
            label="ライブラリのフォルダ"
            help={
              libraryFolder ||
              "まだ選んでいません。設定と同じフォルダを使います。"
            }
          >
            <button
              className="button"
              type="button"
              disabled={busy}
              onClick={() => void scan()}
            >
              フォルダを調べる
            </button>
          </SettingRow>
          {staged.length > 0 && (
            <div className="panel">
              <h2>ダウンロードフォルダに残っているファイル</h2>
              <p className="meta">
                アプリが止まっている間に Downloads/VRCLibMan
                へ置かれたファイルです。自動整理がオンで、ライブラリのフォルダがあるときだけ移します。安全に特定できないファイルは動かしません。
              </p>
              <ul className="file-list">
                {staged.map((item) => (
                  <li key={item.path}>
                    <span>
                      {item.boothId} / {item.filename}
                      <span className="meta"> {item.reason}</span>
                    </span>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void routeStaged(item.path)}
                    >
                      このファイルを移す
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </section>
        <section className="settings-section">
          <header>
            <h2>その他</h2>
            <p>予備の読み込みや補助操作です。</p>
          </header>
          <details className="settings-details">
            <summary>JSON ファイルから読み込む</summary>
            <p>
              拡張から直接届かなかったときの予備です。拡張で保存した自分の JSON
              を読み込みます。
            </p>{" "}
            <div
              className={over ? "drop over" : "drop"}
              onDragOver={(event) => {
                event.preventDefault();
                setOver(true);
              }}
              onDragLeave={() => setOver(false)}
              onDrop={(event) => {
                event.preventDefault();
                setOver(false);
                if (!busy) void onFile(event.dataTransfer.files[0]);
              }}
            >
              <p>書き出した JSON をここにドロップ</p>
              <label className="button">
                ファイルを選ぶ
                <input
                  type="file"
                  disabled={busy}
                  accept="application/json,.json"
                  hidden
                  onChange={(event) => void onFile(event.target.files?.[0])}
                />
              </label>
            </div>
            <label className="field" style={{ marginTop: 16 }}>
              <span>
                <input
                  type="checkbox"
                  checked={extractAfter}
                  disabled={busy}
                  onChange={(event) => setExtractAfter(event.target.checked)}
                />{" "}
                この JSON
                をインポートしたあと、未抽出の商品を設定中のモデルで分類する
              </span>
            </label>
          </details>
          <details className="settings-details">
            <summary>ライブラリにない無料商品を追加</summary>{" "}
            <form
              className="panel"
              style={{ padding: 14, marginTop: 16 }}
              onSubmit={(event) => {
                event.preventDefault();
                const url = freeUrl.trim();
                if (!url) return;
                setBusy(true);
                void api
                  .addFreeProduct(url)
                  .then(
                    (id) => {
                      notify(
                        `無料の商品 ${id} を追加しました。確認タブの未抽出に出ます`,
                      );
                      setFreeUrl("");
                      refresh();
                    },
                    (err: unknown) => fail(errorText(err)),
                  )
                  .finally(() => setBusy(false));
              }}
            >
              <h2 style={{ marginTop: 0 }}>ライブラリにない無料商品</h2>
              <p className="meta">
                商品ページの URL
                を貼ると項目だけ作ります。ページは取りに行きません。拡張でそのページを送るか、無料のファイルを自分でダウンロードしたときも、同じ項目に足せます。
              </p>
              <label className="field">
                <span>商品 URL</span>
                <input
                  value={freeUrl}
                  placeholder="https://booth.pm/ja/items/1234567"
                  onChange={(event) => setFreeUrl(event.target.value)}
                />
              </label>
              <button
                className="button"
                type="submit"
                disabled={busy || freeUrl.trim() === ""}
              >
                無料の項目を追加
              </button>
            </form>
          </details>
          <SettingRow
            label="サムネイル"
            help="登録済み商品のサムネイルを手元に保存します。"
          >
            <button
              className="button"
              type="button"
              disabled={busy}
              onClick={() => void cache()}
            >
              サムネイルを保存
            </button>
          </SettingRow>
        </section>
      </div>
    </section>
  );
}
