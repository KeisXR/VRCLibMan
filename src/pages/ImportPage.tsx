import { useState } from "react";
import { api, chooseFolder, isTauri } from "../api";
import { useShell } from "../App";
import type { ImportReport } from "../types";
import { errorText } from "../ui";

export function ImportPage() {
  const { refresh, notify, fail } = useShell();
  const [over, setOver] = useState(false);
  const [extractAfter, setExtractAfter] = useState(isTauri());
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState(false);

  async function importText(json: string) {
    setBusy(true);
    try {
      const result = await api.importExport(json);
      setReport(result);
      notify(`${result.inserted} 件追加、${result.updated} 件更新しました`);
      refresh();
      if (extractAfter && result.inserted + result.updated > 0) {
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
    }
  }

  async function onFile(file: File | undefined) {
    if (!file) return;
    const text = await file.text();
    await importText(text);
  }

  async function scan() {
    try {
      const path = await chooseFolder();
      if (!path) return;
      setBusy(true);
      const result = await api.scanLibrary(path);
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

  async function extract(force: boolean) {
    setBusy(true);
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
    <section>
      <div className="page-head">
        <div>
          <h1>取り込み</h1>
          <p>
            拡張が今開いている Booth のページを読み、起動中のこのアプリへ送ります。アプリは Booth を取りに行きません。届かないときは JSON を保存し、ここにドロップできます。
          </p>
        </div>
      </div>
      {!isTauri() && (
        <p className="note">
          いまはブラウザ確認用の見本です。抽出、フォルダスキャン、Ollama、サムネイル保存はデスクトップアプリで動きます。
        </p>
      )}
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
          void onFile(event.dataTransfer.files[0]);
        }}
      >
        <p>書き出した JSON をここにドロップ</p>
        <label className="button">
          ファイルを選ぶ
          <input
            type="file"
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
            onChange={(event) => setExtractAfter(event.target.checked)}
          />{" "}
          取り込んだあと、未抽出の商品を Ollama で分類する
        </span>
      </label>
      {report && (
        <p className="meta">
          追加 {report.inserted} / 更新 {report.updated} / 変化なし {report.unchanged} / スキップ {report.skipped}
          {report.warnings.length > 0 ? `。${report.warnings.slice(0, 3).join(" ")}` : ""}
        </p>
      )}
      <div className="row-actions">
        <button className="button primary" type="button" disabled={busy} onClick={() => void extract(false)}>
          未抽出を分類
        </button>
        <button className="button" type="button" disabled={busy} onClick={() => void extract(true)}>
          すべて再分類
        </button>
        <button className="button" type="button" disabled={busy} onClick={() => void api.cancelExtract()}>
          中断
        </button>
        <button className="button" type="button" disabled={busy} onClick={() => void scan()}>
          フォルダを調べる
        </button>
        <button className="button" type="button" disabled={busy} onClick={() => void cache()}>
          サムネイルを保存
        </button>
      </div>
    </section>
  );
}
