import { useEffect, useState } from "react";
import { api, chooseFolder, isTauri, saveLibraryAndScan } from "../api";
import { useShell } from "../App";
import type { ImportReport, StagedDownload } from "../types";
import { errorText } from "../ui";

export function ImportPage() {
  const { revision, refresh, notify, fail } = useShell();
  const [libraryFolder, setLibraryFolder] = useState("");
  const [over, setOver] = useState(false);
  const [extractAfter, setExtractAfter] = useState(isTauri());
  const [report, setReport] = useState<ImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [freeUrl, setFreeUrl] = useState("");
  const [staged, setStaged] = useState<StagedDownload[]>([]);

  useEffect(() => {
    let cancelled = false;
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
      <form
        className="panel"
        style={{ padding: 14, marginTop: 16 }}
        onSubmit={(event) => {
          event.preventDefault();
          const url = freeUrl.trim();
          if (!url) return;
          setBusy(true);
          void api.addFreeProduct(url).then(
            (id) => {
              notify(`無料の商品 ${id} を追加しました。確認タブの未抽出に出ます`);
              setFreeUrl("");
              refresh();
            },
            (err: unknown) => fail(errorText(err)),
          ).finally(() => setBusy(false));
        }}
      >
        <h2 style={{ marginTop: 0 }}>ライブラリにない無料商品</h2>
        <p className="meta">
          商品ページの URL を貼ると項目だけ作ります。ページは取りに行きません。拡張でそのページを送るか、無料のファイルを自分でダウンロードしたときも、同じ項目に足せます。
        </p>
        <label className="field">
          <span>商品 URL</span>
          <input
            value={freeUrl}
            placeholder="https://booth.pm/ja/items/1234567"
            onChange={(event) => setFreeUrl(event.target.value)}
          />
        </label>
        <button className="button" type="submit" disabled={busy || freeUrl.trim() === ""}>
          無料の項目を追加
        </button>
      </form>
      {staged.length > 0 && (
        <div className="panel">
          <h2>ダウンロードフォルダに残っているファイル</h2>
          <p className="meta">
            アプリが止まっている間に Downloads/VRCLibMan へ置かれたファイルです。振り分けがオンで、ライブラリのフォルダがあるときだけ移します。安全に特定できないファイルは動かしません。
          </p>
          <ul className="file-list">
            {staged.map((item) => (
              <li key={item.path}>
                <span>
                  {item.boothId} / {item.filename}
                  <span className="meta"> {item.reason}</span>
                </span>
                <button type="button" disabled={busy} onClick={() => void routeStaged(item.path)}>
                  このファイルを移す
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      {report && (
        <p className={report.variations === 0 && report.downloads === 0 ? "meta warn-text" : "meta"}>
          追加 {report.inserted} / 更新 {report.updated} / 変化なし {report.unchanged} / スキップ {report.skipped}
          {" / "}バリエーション {report.variations} / ダウンロード {report.downloads}
          {report.warnings.length > 0 ? `。${report.warnings.slice(0, 3).join(" ")}` : ""}
        </p>
      )}
      <p className="meta">
        フォルダは設定のライブラリのフォルダと同じです。選ぶと、その場で保存して中を調べます。
        {libraryFolder ? ` 今のフォルダは ${libraryFolder} です。` : " まだ選んでいません。"}
      </p>
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
