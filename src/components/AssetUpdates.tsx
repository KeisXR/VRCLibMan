import { useCallback, useEffect, useState } from "react";
import { api, assetUpdates, openExternal } from "../api";
import { useShell } from "../App";
import type { AssetUpdateConfigure, AssetUpdateStatus, ProductOption } from "../types";
import { errorText } from "../ui";

export function AssetUpdates() {
  const { fail } = useShell();
  const [status, setStatus] = useState<AssetUpdateStatus | null>(null);
  const [products, setProducts] = useState<ProductOption[]>([]);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState("");
  const [pending, setPending] = useState(false);
  const [checking, setChecking] = useState(false);
  const load = useCallback(() => assetUpdates.status().then(setStatus), []);
  useEffect(() => {
    let alive = true;
    void Promise.all([assetUpdates.status(), api.productOptions()]).then(([s, p]) => {
      if (alive) { setStatus(s); setProducts(p); }
    }, (err: unknown) => { if (alive) fail(errorText(err)); });
    return () => { alive = false; };
  }, [fail]);
  useEffect(() => {
    if (pending || checking) return;
    let alive = true;
    const timer = window.setInterval(() => { if (alive) void load().catch(() => {}); }, 5000);
    return () => { alive = false; window.clearInterval(timer); };
  }, [pending, checking, load]);
  async function configure(input: AssetUpdateConfigure) {
    setPending(true);
    try { setStatus(await assetUpdates.configure(input)); }
    catch (err) { fail(errorText(err)); }
    finally { setPending(false); }
  }
  async function check() {
    setChecking(true);
    try { setStatus(await assetUpdates.check()); }
    catch (err) { fail(errorText(err)); }
    finally { setChecking(false); }
  }
  const options = products.filter((p) => !status?.rows.some((r) => r.boothId === p.boothId) && `${p.title} ${p.boothId}`.toLowerCase().includes(query.toLowerCase())).slice(0, 50);
  return <section className="settings-section" id="settings-asset-updates">
    <header><h2>アセットの更新候補</h2><p>許可した商品の公開情報を比較します。有料ファイルの更新や、同じ名前・番号のファイルの差し替えは判定できません。</p></header>
    <div className="section-body asset-updates">
      <p>BOOTH から商品説明と無料ファイル一覧を取得します。初回は比較の基準を保存し、次回から変更を表示します。ダウンロード・適用は行いません。</p>
      <label className="switch-label"><input type="checkbox" checked={status?.automatic ?? false} disabled={!status || pending} onChange={(e) => void configure({ automatic: e.target.checked })} />アプリ起動中、許可した商品を1日1回自動確認する</label>
      <div className="inline-control">
        <input aria-label="更新確認する商品を検索" placeholder="商品名・BOOTH ID で検索" value={query} onChange={(e) => { setQuery(e.target.value); setSelected(""); }} />
        <select aria-label="更新確認する商品" value={selected} onChange={(e) => setSelected(e.target.value)}><option value="">登録済みの商品を選択</option>{options.map((p) => <option key={p.boothId} value={p.boothId}>{p.title}</option>)}</select>
        <button type="button" className="button" disabled={!selected || pending || !status} onClick={() => { void configure({ boothId: selected, enabled: true }); setSelected(""); }}>この商品の確認を許可</button>
      </div>
      <div className="inline-control">
        <button type="button" className="button" disabled={pending || checking || status?.running || !status?.rows.some((r) => r.enabled)} onClick={() => void check()}>{checking || status?.running ? "確認中…" : "許可した商品を今すぐ確認"}</button>
        {(checking || status?.running) && <button type="button" className="button" disabled={pending} onClick={() => void configure({ automatic: false })}>確認を停止</button>}
      </div>
      {status && !status.rows.length && <p>確認を許可した商品はありません。</p>}
      <ul className="asset-update-list">{status?.rows.map((r) => <li key={r.boothId}>
        <label className="switch-label"><input type="checkbox" checked={r.enabled} disabled={pending} onChange={(e) => void configure({ boothId: r.boothId, enabled: e.target.checked })} />{r.title}</label>
        <p role="status">{r.error ? `確認失敗：${r.error}` : r.changes.length ? `更新候補あり：${r.changes.join("・")}` : r.checkedAt ? "比較できる公開情報に変更なし" : "未確認"}</p>
        {r.error && r.changes.length > 0 && <p>前回の更新候補：{r.changes.join("・")}</p>}
        <p>最終確認：{r.checkedAt ? new Date(r.checkedAt).toLocaleString() : "未確認"}</p>
        <div className="inline-control"><button type="button" className="button" onClick={() => void openExternal(`https://booth.pm/ja/items/${r.boothId}`).catch((err: unknown) => fail(errorText(err)))}>BOOTH で確認</button>{r.changes.length > 0 && <button type="button" className="button" disabled={pending || checking || status.running} onClick={() => void configure({ boothId: r.boothId, acknowledge: true })}>確認済みにする</button>}</div>
      </li>)}</ul>
      <p className="muted">オフにすると以後の自動確認を停止します。実行中の通信は完了することがあります。ログイン・年齢確認が必要な商品は確認失敗として扱います。通信失敗時は1時間以上空けます。</p>
    </div>
  </section>;
}
