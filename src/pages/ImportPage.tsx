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
  ExtractReport,
  ImportReport,
  StagedDownload,
  IntegrationStatus,
  JobProgress,
} from "../types";
import { errorText } from "../ui";
import { SettingRow } from "../components/SettingRow";
import { ProductExclusions } from "../components/ProductExclusions";
import { ExtensionInstructions } from "../components/BoothImportSettings";
import { OllamaMemoryControls } from "../components/OllamaMemoryControls";

function extractNotice(result: ExtractReport): string {
  const head = result.cancelled
    ? "分類を中断しました"
    : `${result.processed} 件を分類しました${result.failed > 0 ? `（${result.failed} 件失敗）` : ""}`;
  if (typeof result.boothSettled === "number" && typeof result.modelUsed === "number") {
    return `${head}。カテゴリとタグで ${result.boothSettled} 件、モデルで ${result.modelUsed} 件`;
  }
  return head;
}

function StaleReclassifyRow({ busy, onExtract }: { busy: boolean; onExtract: () => void }) {
  const { revision } = useShell();
  const [count, setCount] = useState(0);
  useEffect(() => {
    void api.classificationStatus().then((status) => setCount(status.staleCount)).catch(() => setCount(0));
  }, [revision]);
  if (count === 0) return null;
  return (
    <SettingRow
      label={`古い分類 ${count} 件`}
      help="分類ルールの更新後、過去の分類結果を見直すときに使います。"
    >
      <button className="button primary" type="button" disabled={busy} onClick={onExtract}>
        古い分類を再分類（{count}件）
      </button>
    </SettingRow>
  );
}

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
  const [classificationErrors, setClassificationErrors] = useState<string[]>([]);
  const [extractStats, setExtractStats] = useState<{ boothSettled: number; modelUsed: number } | null>(null);
  const [receiverStatus, setReceiverStatus] = useState("確認中");
  const [staged, setStaged] = useState<StagedDownload[]>([]);
  const [llmBackend, setLlmBackend] = useState<"ollama" | "apple">("ollama");
  const [ollamaModel, setOllamaModel] = useState("");

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
            : next.paired
              ? "ペアリング済み"
              : "ペアリングすると、拡張機能から商品情報を送れます",
        );
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    api.settings().then(
      (value) => {
        if (!cancelled) {
          setLibraryFolder(value.libraryFolder);
          setLlmBackend(value.llmBackend);
          setOllamaModel(value.ollamaModel);
        }
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
        setClassificationErrors([]);
        setExtracting(true);
        const extracted = await api.extractProducts("unclassified");
        notify(extractNotice(extracted));
        setClassificationErrors(extracted.errors);
        if (typeof extracted.boothSettled === "number" && typeof extracted.modelUsed === "number") {
          setExtractStats({ boothSettled: extracted.boothSettled, modelUsed: extracted.modelUsed });
        }
        if (extracted.failed > 0) fail(`${extracted.failed} 件の商品を分類できませんでした。分類結果の詳細を確認してください。`);
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
        `${result.indexed} 件を調べ、${result.linked} 件を商品と結び付けました${result.truncated ? "。上限に達したため途中です" : ""}`,
      );
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  async function extract(target: "unclassified" | "stale" | "all") {
    if (
      target === "all" &&
      !window.confirm(
        "すべて再分類すると、既存の分類結果を上書きすることがあります。手動で固定した情報は維持します。続けますか？",
      )
    )
      return;
    if (target === "stale") {
      const status = await api.classificationStatus();
      if (
        !window.confirm(
          `古い分類ルールで分類された ${status.staleCount} 件を再分類します。時間がかかり、GPU メモリを使います。手動で固定したカテゴリや対応アバターは維持します。続けますか？`,
        )
      )
        return;
    }
    setBusy(true);
    setExtracting(true);
    setClassificationErrors([]);
    setExtractStats(null);
    setProgress(null);
    try {
      const result = await api.extractProducts(target);
      notify(extractNotice(result));
      setClassificationErrors(result.errors);
      if (typeof result.boothSettled === "number" && typeof result.modelUsed === "number") {
        setExtractStats({ boothSettled: result.boothSettled, modelUsed: result.modelUsed });
      }
      if (result.failed > 0) fail(`${result.failed} 件の商品を分類できませんでした。分類結果の詳細を確認してください。`);
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
            手元のファイルと結び付ける、の順に進めます。
          </p>
        </div>
      </div>
      {!isTauri() && (
        <p className="note">
          見本データを表示しています。実際の分類やファイル操作はデスクトップアプリで利用できます。
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
          <details className="settings-details"><summary>商品を管理・除外</summary><ProductExclusions /></details>
          {integration?.mode !== "browser" && <><SettingRow label="拡張機能" help={receiverStatus}>
            <a className="button" href="#/settings">
              読み込み方とペアリングの設定
            </a>
          </SettingRow>
          <details className="settings-details">
            <summary>拡張機能の使い方・受信履歴</summary>
            <ExtensionInstructions />
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
          </details></>}
          {integration?.mode !== "extension" && <>
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
              {builtinBrowser ? "ブラウザ画面へ" : "読み込み方を選ぶ"}
            </a>
          </SettingRow>
          {integration?.browserImport && (
            <details className="settings-details"><summary>内蔵ブラウザの受信履歴</summary><p className="section-note">
              内蔵ブラウザの最終読み込み:{" "}
              {new Date(integration.browserImport.at).toLocaleString("ja-JP")}{" "}
              ・{integration.browserImport.products} 件
            </p></details>
          )}
          </>}
          {report && (
            <div className="section-body">
              <strong>直前の JSON インポート結果</strong>
              <p>{report.inserted} 件追加、{report.updated} 件更新しました。</p>
              {report.warnings.length > 0 && <p className="warn-text">{report.warnings.slice(0, 3).join(" ")}</p>}
              <details className="settings-details"><summary>読み込み結果の詳細</summary><p
                className={
                  report.variations === 0 && report.downloads === 0
                    ? "meta warn-text"
                    : "meta"
                }
              >
                追加 {report.inserted} / 更新 {report.updated} / 変化なし{" "}
                {report.unchanged} / スキップ {report.skipped}
                {" / "}バリエーション {report.variations} / ファイル情報{" "}
                {report.downloads}
                {report.unnamedDownloads > 0
                  ? `（名前なし ${report.unnamedDownloads}）`
                  : ""}
              </p></details>
            </div>
          )}
        </section>
        <section className="settings-section">
          <header>
            <h2>2. 分類</h2>
            <p>BOOTHのカテゴリとタグで決まる商品は先に分類し、決まらない商品だけ分類モデルへ渡します。</p>
          </header>
          <SettingRow
            label={
              unextracted == null
                ? "未分類の件数を確認中"
                : `未分類 ${unextracted} 件`
            }
            help="分類モデルは設定で選べます。"
          >
            <div className="row-actions">
              <button
                className="button primary"
                type="button"
                disabled={busy || unextracted === 0}
                onClick={() => void extract("unclassified")}
              >
                未分類を分類
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
          {extractStats && (
            <p className="meta">
              カテゴリとタグで {extractStats.boothSettled} 件、モデルで {extractStats.modelUsed} 件
            </p>
          )}
          <StaleReclassifyRow busy={busy} onExtract={() => void extract("stale")} />
          <details className="settings-details">
            <summary>すべて再分類</summary>
            <p>
              既存の分類結果を上書きすることがあります。手動で固定した情報は維持します。
            </p>
            <button
              className="button"
              type="button"
              disabled={busy}
              onClick={() => void extract("all")}
            >
              すべて再分類
            </button>
          </details>
          {classificationErrors.length > 0 && (
            <details className="settings-details">
              <summary>分類結果の詳細</summary>
              <ul>{classificationErrors.map((message, index) => <li key={index}>{message}</li>)}</ul>
            </details>
          )}
          {llmBackend === "ollama" && (
            <SettingRow label="読み込み状態" help="同じ PC のほかのアプリが同じモデルを使っている場合も影響します。">
              <OllamaMemoryControls
                key={`${revision}-${extracting ? "run" : "idle"}`}
                model={ollamaModel}
                enabled
              />
            </SettingRow>
          )}
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
              "ファイルを保存しているフォルダを選んでください。"
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
                ダウンロード後に自動整理されなかったファイルです。自動整理と保存先を設定すると、ライブラリへ移動できます。
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
          </header>
          <details className="settings-details">
            <summary>JSON ファイルから読み込む</summary>
            <p>
              拡張機能で保存した JSON ファイルを読み込みます。
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
                読み込み後に未分類の商品を分類する
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
                        `商品 ${id} を無料商品として登録しました。確認画面の未分類に表示されます`,
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
                商品 URL を登録します。商品情報は自動取得しません。商品名や説明を読み込むには、拡張機能から商品ページを送ってください。
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
                無料商品を登録
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
