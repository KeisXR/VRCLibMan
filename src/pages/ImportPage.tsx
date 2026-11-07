import { useEffect, useState } from "react";
import type { ExistingImportPreview } from "../api";
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
  LlmBackend,
} from "../types";
import { errorText } from "../ui";
import { SettingRow } from "../components/SettingRow";
import { ProductExclusions } from "../components/ProductExclusions";
import { UnlinkedFiles } from "../components/UnlinkedFiles";
import { ExtensionInstructions } from "../components/BoothImportSettings";
import { BuiltinBrowserImport } from "../components/BuiltinBrowserControls";
import { OllamaMemoryControls } from "../components/OllamaMemoryControls";

function extractNotice(result: ExtractReport): string {
  const head = result.cancelled
    ? "分類を中断しました"
    : `${result.processed} 件を分類しました${result.failed > 0 ? `（${result.failed} 件失敗）` : ""}`;
  if (typeof result.boothSettled === "number" && typeof result.modelUsed === "number") {
    const withheld = result.cloudWithheld ? `。年齢制限の ${result.cloudWithheld} 件はクラウドに送らず未分類のままです` : "";
    return `${head}。カテゴリとタグで ${result.boothSettled} 件、モデルで ${result.modelUsed} 件${withheld}`;
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
  const [existingRoot, setExistingRoot] = useState("");
  const [existingPreview, setExistingPreview] = useState<ExistingImportPreview | null>(null);
  const [selectedExisting, setSelectedExisting] = useState<Record<string, number>>({});
  const [existingCopy, setExistingCopy] = useState(false);
  const [existingLimit, setExistingLimit] = useState(50);
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
  const [llmBackend, setLlmBackend] = useState<LlmBackend>("ollama");
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
        notify(extractNotice(extracted), undefined, { persist: extracted.failed > 0 });
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

  async function findExisting() {
    try {
      const path = isTauri() ? await chooseFolder() : "/見本/Downloads";
      if (!path) return;
      setBusy(true);
      setExistingPreview(null);
      const result = await api.previewExistingImport(path);
      setExistingRoot(path);
      setExistingPreview(result);
      const chosen: Record<string, number> = {};
      const targets = new Set<string>();
      for (const e of result.entries) {
        const c = e.choices[0];
        if (e.status !== "matched" || !c) continue;
        const key = JSON.stringify([c.boothId, c.variation, c.filename]);
        if (!targets.has(key)) { chosen[e.path] = 0; targets.add(key); }
      }
      setSelectedExisting(chosen);
      setExistingLimit(50);
    } catch (err) { fail(errorText(err)); } finally { setBusy(false); }
  }

  async function registerExisting() {
    if (!existingPreview) return;
    const selections = existingPreview.entries.flatMap((e) => {
      const c = e.choices[selectedExisting[e.path]];
      return c ? [{ path: e.path, boothId: c.boothId, variation: c.variation, filename: c.filename, size: e.size }] : [];
    });
    try {
      setBusy(true);
      const result = await api.registerExistingImport(existingRoot, selections, existingCopy);
      notify(`${result.registered} 件を登録しました${result.errors.length ? `（${result.errors.length} 件失敗）` : ""}`, undefined, { persist: result.errors.length > 0 });
      setExistingPreview(null);
      refresh();
      if (result.errors.length) fail(result.errors.join("\n"));
    } catch (err) { fail(errorText(err)); } finally { setBusy(false); }
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
        undefined,
        { persist: result.truncated },
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
      notify(extractNotice(result), undefined, { persist: result.failed > 0 });
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
            <h2 id="import-booth">1. BOOTH から読み込む</h2>
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
          {builtinBrowser ? (
            <BuiltinBrowserImport />
          ) : (
            <SettingRow label="内蔵ブラウザ" help="無効です。設定で読み込み方を変更できます。">
              <a className="button" href="#/settings">読み込み方を選ぶ</a>
            </SettingRow>
          )}
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
            <h2 id="import-classification">2. 分類</h2>
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
            <h2 id="import-existing-files">3. 手元のファイル</h2>
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
          <SettingRow label="ダウンロード済みファイルを登録" help="Downloads や別のフォルダを選び、ZIP・Unitypackage を BOOTH の配布ファイル名と照合します。元の場所に登録します。">
            <button className="button" type="button" disabled={busy} onClick={() => void findExisting()}>取り込み元を選ぶ</button>
          </SettingRow>
          {existingPreview && <div className="panel">
            <h3>照合結果</h3>
            <p className="meta">{existingRoot} ・{existingPreview.entries.length} 件 {existingPreview.truncated && "（上限に達したため途中です）"}</p>
            <p>配布ファイル名・サイズで照合しました。要確認のファイルは商品を選択してください。バージョン表記は保持し、ブラウザの連番だけを補助的に除いています。</p>
            {existingPreview.entries.length === 0 && <p>対象ファイルはありません。商品・ファイル情報を先にBOOTHから読み込んでください。</p>}
            <ul className="file-list">{existingPreview.entries.slice(0, existingLimit).map((entry) => <li key={entry.path}>
              <div>
                <strong>{entry.filename}</strong> — {entry.status === "matched" ? "一致" : entry.status === "registered" ? "登録済み" : entry.status === "review" ? "要確認" : "候補なし"}
                <p className="meta">{entry.path} ・{entry.size.toLocaleString()} bytes</p>
                {entry.choices.length > 0 && <select aria-label={`${entry.filename} の商品`} disabled={busy} value={selectedExisting[entry.path] ?? -1} onChange={(event) => setSelectedExisting((prev) => {
                  const next = { ...prev }; const value = Number(event.target.value);
                  if (value < 0) delete next[entry.path]; else next[entry.path] = value;
                  return next;
                })}>
                  <option value={-1}>取り込まない</option>
                  {entry.choices.map((c, i) => <option key={i} value={i} disabled={c.registered || (c.sizeBytes != null && c.sizeBytes !== entry.size)}>
                    {c.title} / {c.variation || "通常"} / {c.filename}{c.registered ? "（登録済み）" : c.sizeBytes != null && c.sizeBytes !== entry.size ? "（サイズ不一致）" : ""}
                  </option>)}
                </select>}
              </div>
            </li>)}</ul>
            {existingLimit < existingPreview.entries.length && <button type="button" onClick={() => setExistingLimit((n) => n + 50)}>さらに50件表示</button>}
            <label className="field"><span>取り込み方法</span><select value={existingCopy ? "copy" : "register"} disabled={busy} onChange={(e) => setExistingCopy(e.target.value === "copy")}>
              <option value="register">元の場所のまま登録</option><option value="copy" disabled={!libraryFolder}>ライブラリへコピー（元のファイルを残す）</option>
            </select></label>
            {existingCopy && <p className="meta">コピー先: {libraryFolder}。同名ファイルは上書きしません。</p>}
            <button className="button primary" type="button" disabled={busy || Object.keys(selectedExisting).length === 0} onClick={() => void registerExisting()}>{busy ? "取り込み中…" : `選択した ${Object.keys(selectedExisting).length} 件を取り込む`}</button>
          </div>}
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
          <UnlinkedFiles />
        </section>
        <section className="settings-section">
          <header>
            <h2>その他</h2>
          </header>
          <details className="settings-details">
            <summary>JSON ファイルから読み込む</summary>
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
                        `商品 ${id} を無料商品として登録しました。「未分類を分類」で分類できます`,
                      );
                      setFreeUrl("");
                      refresh();
                    },
                    (err: unknown) => fail(errorText(err)),
                  )
                  .finally(() => setBusy(false));
              }}
            >
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
