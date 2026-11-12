import { useCallback, useEffect, useRef, useState } from "react";
import { api, assetUpdates, openExternal } from "../api";
import { useShell } from "../App";
import { assetUpdateListStartsOpen, assetUpdateListSummary } from "../assetUpdatesUi";
import type { AssetUpdateConfigure, AssetUpdateStatus, ProductOption } from "../types";
import { errorText } from "../ui";

export function AssetUpdates() {
  const { fail } = useShell();
  const [status, setStatus] = useState<AssetUpdateStatus | null>(null);
  const [products, setProducts] = useState<ProductOption[]>([]);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState("");
  const [listQuery, setListQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [page, setPage] = useState(0);
  const [pending, setPending] = useState(false);
  const [checking, setChecking] = useState(false);
  const [listOpen, setListOpen] = useState(false);
  const statusRequest = useRef(0);
  const checkRequest = useRef(0);
  const observedRunning = useRef(false);
  const load = useCallback(async () => {
    const request = ++statusRequest.current;
    const next = await assetUpdates.status();
    if (request !== statusRequest.current) return;
    setStatus(next);
    if (next.running) observedRunning.current = true;
    else if (observedRunning.current) setChecking(false);
  }, []);
  useEffect(() => {
    let alive = true;
    void Promise.all([assetUpdates.status(), api.productOptions()]).then(([s, p]) => {
      if (!alive) return;
      setStatus(s);
      setProducts(p);
      setListOpen(assetUpdateListStartsOpen(s.rows.length));
    }, (err: unknown) => { if (alive) fail(errorText(err)); });
    return () => { alive = false; };
  }, [fail]);
  useEffect(() => {
    if (pending) return;
    let alive = true;
    const timer = window.setInterval(() => { if (alive) void load().catch(() => {}); }, 5000);
    return () => { alive = false; window.clearInterval(timer); };
  }, [pending, load]);
  async function configure(input: AssetUpdateConfigure) {
    statusRequest.current++;
    setPending(true);
    try { setStatus(await assetUpdates.configure(input)); }
    catch (err) { fail(errorText(err)); }
    finally { setPending(false); }
  }
  async function check() {
    const request = ++checkRequest.current;
    observedRunning.current = false;
    setChecking(true);
    try {
      const next = await assetUpdates.check();
      if (request === checkRequest.current) { statusRequest.current++; setStatus(next); }
    }
    catch (err) { if (request === checkRequest.current) fail(errorText(err)); }
    finally { if (request === checkRequest.current) setChecking(false); }
  }
  const options = products.filter((p) => !status?.rows.some((r) => r.boothId === p.boothId) && `${p.title} ${p.boothId}`.toLowerCase().includes(query.toLowerCase())).slice(0, 50);
  const matching = (status?.rows ?? []).filter((r) => `${r.title} ${r.boothId}`.toLowerCase().includes(listQuery.toLowerCase()) && (filter === "all" || (filter === "changed" ? r.changes.length > 0 : Boolean(r.error))));
  const lastPage = Math.max(0, Math.ceil(matching.length / 50) - 1);
  const currentPage = Math.min(page, lastPage);
  const visible = matching.slice(currentPage * 50, (currentPage + 1) * 50);
  const summary = assetUpdateListSummary(status);
  return <section className="settings-section" id="settings-asset-updates">
    <header><h2>アセットの更新設定</h2><p>公開情報を比較して変更を確認します。有料ファイルの更新や、同じ名前・番号のファイル差し替えは検出できません。</p></header>
    <div className="section-body asset-updates">
      <p>「今すぐ確認」では、内蔵ブラウザが有効なら BOOTH ライブラリから最新のファイル情報も読み直して商品カードへ反映し、あわせて商品説明と無料ファイル一覧を比較します。ファイルの自動ダウンロードや適用は行いません。</p>
      <label className="switch-label"><input type="checkbox" checked={status?.allProducts ?? false} disabled={!status || pending} onChange={(e) => void configure({ allProducts: e.target.checked })} />全商品をまとめて確認対象にする（今後追加する商品も含む）</label>
      <p>登録済み {status?.totalProducts ?? 0} 件中、{status?.rows.filter((r) => r.enabled).length ?? 0} 件が確認対象です。個別のチェックで対象から外せます。全体をオフにすると全商品の許可を解除します。</p>
      <label className="switch-label"><input type="checkbox" checked={status?.automatic ?? false} disabled={!status || pending} onChange={(e) => void configure({ automatic: e.target.checked })} />アプリ起動中、許可した商品を1日1回自動確認する</label>
      <div className="inline-control">
        <input aria-label="更新確認する商品を検索" placeholder="商品名・BOOTH ID で検索" value={query} onChange={(e) => { setQuery(e.target.value); setSelected(""); }} />
        <select aria-label="更新確認する商品" value={selected} onChange={(e) => setSelected(e.target.value)}><option value="">登録済みの商品を選択</option>{options.map((p) => <option key={p.boothId} value={p.boothId}>{p.title}</option>)}</select>
        <button type="button" className="button" disabled={!selected || pending || !status} onClick={() => { void configure({ boothId: selected, enabled: true }); setSelected(""); }}>確認対象に追加</button>
      </div>
      <div className="inline-control">
        <button type="button" className="button" disabled={pending || checking || status?.running || !status?.rows.some((r) => r.enabled)} onClick={() => void check()}>{checking || status?.running ? "確認中…" : "今すぐ確認"}</button>
        {(checking || status?.running) && <button type="button" className="button" disabled={pending} onClick={() => void configure({ automatic: false })}>自動確認をオフ</button>}
      </div>
      {status && !status.rows.length && <p role="status">{summary}</p>}
      {Boolean(status?.rows.length) && <div className="asset-update-summary">
        <p role="status">{summary}</p>
        <button
          type="button"
          className="button"
          aria-expanded={listOpen}
          aria-controls="asset-update-list-panel"
          onClick={() => setListOpen((open) => !open)}
        >{listOpen ? "一覧を閉じる" : "一覧を表示"}</button>
      </div>}
      {Boolean(status?.rows.length) && listOpen && <div id="asset-update-list-panel" className="asset-update-panel">
        <div className="inline-control">
          <input aria-label="確認対象の商品を絞り込む" placeholder="確認対象の商品名・BOOTH ID" value={listQuery} onChange={(e) => { setListQuery(e.target.value); setPage(0); }} />
          <select aria-label="確認結果で絞り込む" value={filter} onChange={(e) => { setFilter(e.target.value); setPage(0); }}><option value="all">すべての結果</option><option value="changed">更新あり</option><option value="error">確認失敗</option></select>
          <span>{matching.length} 件</span>
        </div>
        <ul className="asset-update-list">{visible.map((r) => <li key={r.boothId}>
          <label className="switch-label"><input type="checkbox" checked={r.enabled} disabled={pending} onChange={(e) => void configure({ boothId: r.boothId, enabled: e.target.checked })} /><span className="asset-update-title" title={r.title}>{r.title}</span></label>
          <p role="status">{r.error ? `確認失敗：${r.error}` : r.changes.length ? `更新あり：${r.changes.join("・")}` : r.checkedAt ? "比較できる公開情報に変更なし" : "未確認"}</p>
          {r.error && r.changes.length > 0 && <p>前回の更新：{r.changes.join("・")}</p>}
          <p>最終確認：{r.checkedAt ? new Date(r.checkedAt).toLocaleString() : "未確認"}</p>
          <div className="inline-control"><button type="button" className="button" onClick={() => void openExternal(`https://booth.pm/ja/items/${r.boothId}`).catch((err: unknown) => fail(errorText(err)))}>BOOTH で確認</button>{r.changes.length > 0 && <button type="button" className="button" disabled={pending || checking || status?.running} onClick={() => void configure({ boothId: r.boothId, acknowledge: true })}>確認済みにする</button>}</div>
        </li>)}</ul>
        {matching.length > 50 && <div className="inline-control">
          <button type="button" className="button" disabled={currentPage === 0} onClick={() => setPage(currentPage - 1)}>前の50件</button>
          <span>{currentPage + 1} / {lastPage + 1} ページ</span>
          <button type="button" className="button" disabled={currentPage === lastPage} onClick={() => setPage(currentPage + 1)}>次の50件</button>
        </div>}
      </div>}
      <p className="muted">オフにすると以後の自動確認を停止します。実行中の通信は完了することがあります。ログイン・年齢確認が必要な商品は確認失敗として扱います。通信失敗時は1時間以上空けます。</p>
    </div>
  </section>;
}
