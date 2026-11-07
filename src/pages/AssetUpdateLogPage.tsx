import { useEffect, useState } from "react";
import { assetUpdates, isTauri, openBoothLink } from "../api";
import { useShell } from "../App";
import { formatLogTime } from "../classificationLog";
import type { AssetUpdateFile, AssetUpdateRecord, AssetUpdateStatus } from "../types";
import { errorText } from "../ui";

function Files({ files }: { files: AssetUpdateFile[] }) {
  if (!files.length) return <>なし</>;
  return <ul>{files.map(file => <li key={JSON.stringify(file)}>
    {file.filename}{file.variation && `（${file.variation}）`}
    {file.downloadableId && <div><code>https://booth.pm/downloadables/{file.downloadableId}</code></div>}
  </li>)}</ul>;
}

export function AssetUpdateLogPage() {
  const { fail, revision } = useShell();
  const [status, setStatus] = useState<AssetUpdateStatus | null>(null);
  const [records, setRecords] = useState<AssetUpdateRecord[] | null>(null);
  const [reload, setReload] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    let alive = true;
    let request = 0;
    let stop = () => {};
    const refresh = async () => {
      const id = ++request;
      try {
        const [next, history] = await Promise.all([assetUpdates.status(), assetUpdates.history()]);
        if (alive && id === request) { setStatus(next); setRecords(history); setError(null); }
      } catch (err) { if (alive && id === request) setError(errorText(err)); }
    };
    void refresh();
    if (isTauri()) void import("@tauri-apps/api/event").then(({ listen }) => listen("asset-updates-changed", () => void refresh())).then(unlisten => { if (alive) stop = unlisten; else unlisten(); });
    return () => { alive = false; stop(); };
  }, [revision, reload]);
  const candidates = status?.rows.filter(row => row.enabled && row.changes.length) ?? [];
  const titles = new Map(status?.rows.map(row => [row.boothId, row.title]));
  const openProduct = (id: string) => void openBoothLink(`https://booth.pm/ja/items/${id}`).catch(err => fail(errorText(err)));
  async function acknowledge(boothId: string) {
    setSaving(true);
    try { setStatus(await assetUpdates.configure({ boothId, acknowledge: true })); setReload(n => n + 1); }
    catch (err) { fail(errorText(err)); }
    finally { setSaving(false); }
  }
  return <section className="log-page">
    <div className="page-head"><div><h1>アセット更新記録</h1>
      <p className="meta">VRCLibManが差分を見つけた日時を表示します。BOOTHでの公開日時とは異なります。</p></div></div>
    {status?.running && <p role="status">アセットの更新を確認中… {status.progress ? `${status.progress.done} / ${status.progress.total} 件` : "確認の準備中です。"}</p>}
    {error && <div className="banner error" role="alert">{error} <button className="button" type="button" onClick={() => setReload(n => n + 1)}>再読み込み</button></div>}
    {candidates.length > 0 && <details open>
      <summary>未確認の更新候補（{candidates.length}件）</summary>
      <ul className="log-entries">{candidates.map(row => <li className="log-entry" key={row.boothId}>
        <strong>{row.title}</strong><p className="meta">{row.changes.join("・")}</p>
        <div className="row-actions"><button type="button" className="button" onClick={() => openProduct(row.boothId)}>BOOTHで確認</button>
          <button type="button" className="button" disabled={saving || status?.running} onClick={() => void acknowledge(row.boothId)}>確認済みにする</button></div>
      </li>)}</ul>
    </details>}
    <h2>検出履歴</h2>
    {records === null && !error && <p role="status">読み込み中…</p>}
    {records?.length === 0 && <p className="meta">更新記録はまだありません。以前の候補は上の一覧で確認できます。</p>}
    <ul className="log-entries">{records?.map((record, index) => <li className="log-entry" key={`${record.boothId}:${record.detectedAt}:${index}`}>
      <div className="log-entry-head"><strong>{titles.get(record.boothId) ?? `商品 ${record.boothId}`}</strong>
        <button className="button" type="button" onClick={() => openProduct(record.boothId)}>BOOTHで確認</button></div>
      <p className="meta"><time dateTime={record.detectedAt}>{formatLogTime(record.detectedAt)}</time> ・{record.source === "library" ? "ライブラリの読み込み" : "公開情報の確認"}</p>
      {record.descriptionChanged && <p>商品説明が変更されました。</p>}
      {(record.removed.length > 0 || record.added.length > 0) && <dl className="log-changes">
        <dt>変更前（外れたファイル）</dt><dd><Files files={record.removed} /></dd>
        <dt>変更後（追加されたファイル）</dt><dd><Files files={record.added} /></dd>
      </dl>}
    </li>)}</ul>
  </section>;
}
