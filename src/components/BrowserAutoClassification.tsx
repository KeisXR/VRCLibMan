import { useEffect, useRef, useState } from "react";
import { api, isTauri } from "../api";
import { useShell } from "../App";
import { GENRE_LABEL, genresForKind } from "../types";
import type { GenreKind } from "../types";
import { errorText } from "../ui";

type Item = { boothId: string; title: string };
export function BrowserAutoClassification() {
  const { revision, refresh, fail, notify } = useShell();
  const [items, setItems] = useState<Item[]>([]);
  const [deferred, setDeferred] = useState(false);
  const [kind, setKind] = useState<GenreKind | "avatar">("asset");
  const [genre, setGenre] = useState("outfit");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const dialog = useRef<HTMLDialogElement>(null);
  const current = items[0];
  useEffect(() => {
    let alive = true;
    void api.browserAutoPending().then(value => { if (alive) setItems(value); }, err => { if (alive) fail(errorText(err)); });
    return () => { alive = false; };
  }, [revision, reload, fail]);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true; let stop = () => {};
    void import("@tauri-apps/api/event").then(({ listen }) => listen("browser-auto-pending", () => {
      if (alive) setReload(v => v + 1);
    })).then(unlisten => { if (alive) stop = unlisten; else unlisten(); });
    return () => { alive = false; stop(); };
  }, []);
  useEffect(() => { setKind("asset"); setGenre("outfit"); setName(""); setSaveError(null); }, [current?.boothId]);
  useEffect(() => {
    if (current && !deferred) { if (!dialog.current?.open) dialog.current?.showModal(); }
    else if (dialog.current?.open) dialog.current.close();
  }, [current, deferred]);
  async function save() {
    if (!current || busy) return;
    setBusy(true);
    setSaveError(null);
    try {
      if (kind === "avatar") {
        const result = await api.registerOwnedBody(current.boothId, name.trim() || null);
        if (result.kind === "needsName") { setName(result.suggestedName); throw new Error("アバター名を入力して、もう一度確定してください。"); }
      } else { await api.moveProductGenre(current.boothId, kind, genre); }
      await api.browserAutoFinish(current.boothId);
      setItems(value => value.filter(item => item.boothId !== current.boothId));
      notify("分類を確定しました"); refresh();
    } catch (err) { setSaveError(errorText(err)); fail(errorText(err)); } finally { setBusy(false); }
  }
  return <>
    {items.length > 0 && deferred && <button type="button" className="button browser-auto-resume" onClick={() => setDeferred(false)}>取得した商品の分類（{items.length}件）</button>}
    <dialog ref={dialog} className="browser-auto-dialog" aria-labelledby="browser-auto-title" onCancel={event => { event.preventDefault(); if (!busy) setDeferred(true); }}>
      {current && <>
        <h2 id="browser-auto-title">取得した商品の分類</h2><p>{current.title}</p>
        <p className="meta">残り{items.length}件。分類先を選んで確定すると、以後の再ダウンロードでは確認しません。</p>
        <label className="field"><span>種類</span><select autoFocus disabled={busy} value={kind} onChange={event => {
          const next = event.target.value as typeof kind; setKind(next); setGenre(next === "world" ? "world" : "outfit");
        }}><option value="asset">アセット</option><option value="world">ワールド</option><option value="avatar">アバター本体</option></select></label>
        {kind === "avatar" ? <label className="field"><span>アバター名（自動で決まらない場合）</span><input disabled={busy} value={name} onChange={event => setName(event.target.value)} /></label>
          : <label className="field"><span>分類先</span><select disabled={busy} value={genre} onChange={event => setGenre(event.target.value)}>{genresForKind(kind).map(value => <option key={value} value={value}>{GENRE_LABEL[value] ?? value}</option>)}</select></label>}
        {saveError && <p className="banner error" role="alert">{saveError}</p>}
        <div className="row-actions"><button className="button primary" type="button" disabled={busy} onClick={() => void save()}>{busy ? "保存中…" : "分類を確定"}</button><button type="button" className="button" disabled={busy} onClick={() => setDeferred(true)}>後で分類する</button></div>
        <p className="meta">後回しにしても登録は残ります。「取得した商品の分類」から再開できます。</p>
      </>}
    </dialog>
  </>;
}
