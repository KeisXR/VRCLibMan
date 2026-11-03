import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { SyntheticEvent } from "react";
import { api, isTauri } from "../api";
import { useShell } from "../App";
import { postponeShown, promptOpen, reconcilePostponed, showsResume } from "../browserAutoPrompt";
import { GENRE_LABEL, genresForKind } from "../types";
import type { GenreKind } from "../types";
import { errorText } from "../ui";

type Item = { boothId: string; title: string };

export function BrowserAutoClassification() {
  const { revision, refresh, fail, notify } = useShell();
  const [items, setItems] = useState<Item[]>([]);
  const [postponedIds, setPostponedIds] = useState<readonly string[]>([]);
  const [kind, setKind] = useState<GenreKind | "avatar">("asset");
  const [genre, setGenre] = useState("outfit");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const dialog = useRef<HTMLDialogElement>(null);
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const current = items[0];
  const itemIds = items.map((item) => item.boothId);
  const open = promptOpen(itemIds, postponedIds);

  useEffect(() => {
    let alive = true;
    void api.browserAutoPending().then(value => { if (alive) setItems(value); }, err => { if (alive) fail(errorText(err)); });
    return () => { alive = false; };
  }, [revision, reload, fail]);

  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let stop = () => {};
    void import("@tauri-apps/api/event").then(({ listen }) => listen("browser-auto-pending", () => {
      if (alive) setReload(value => value + 1);
    })).then(unlisten => { if (alive) stop = unlisten; else unlisten(); });
    return () => { alive = false; stop(); };
  }, []);

  useEffect(() => {
    setPostponedIds((prev) => reconcilePostponed(prev, items.map((item) => item.boothId)));
  }, [items]);

  useEffect(() => { setKind("asset"); setGenre("outfit"); setName(""); setSaveError(null); }, [current?.boothId]);

  useLayoutEffect(() => {
    const element = dialog.current;
    if (!element) return;
    if (open) {
      if (!element.open) element.showModal();
    } else if (element.open) element.close();
  }, [open]);

  function postpone() {
    const ids = itemsRef.current.map((item) => item.boothId);
    if (ids.length === 0) return;
    setPostponedIds((prev) => postponeShown(prev, ids));
  }

  function onDialogClose(event: SyntheticEvent<HTMLDialogElement>) {
    // 取り外されたダイアログの close では後回しにしない（開発時の二重マウントなど）。
    if (!event.currentTarget.isConnected) return;
    postpone();
  }

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
    {showsResume(itemIds, postponedIds) && <button type="button" className="button browser-auto-resume" onClick={() => setPostponedIds([])}>取得した商品の分類（{items.length}件）</button>}
    <dialog
      ref={dialog}
      className="browser-auto-dialog"
      aria-labelledby="browser-auto-title"
      closedby="closerequest"
      onCancel={event => { event.preventDefault(); if (!busy) postpone(); }}
      onClose={onDialogClose}
    >
      {current && <>
        <h2 id="browser-auto-title">取得した商品の分類</h2><p>{current.title}</p>
        <p className="meta">残り{items.length}件。確定すると、再ダウンロード時の確認を省略します。</p>
        <label className="field"><span>種類</span><select autoFocus disabled={busy} value={kind} onChange={event => {
          const next = event.target.value as typeof kind; setKind(next); setGenre(next === "world" ? "world" : "outfit");
        }}><option value="asset">アセット</option><option value="world">ワールド</option><option value="avatar">アバター本体</option></select></label>
        {kind === "avatar" ? <label className="field"><span>アバター名（自動で決まらない場合）</span><input disabled={busy} value={name} onChange={event => setName(event.target.value)} /></label>
          : <label className="field"><span>分類先</span><select disabled={busy} value={genre} onChange={event => setGenre(event.target.value)}>{genresForKind(kind).map(value => <option key={value} value={value}>{GENRE_LABEL[value] ?? value}</option>)}</select></label>}
        {saveError && <p className="banner error" role="alert">{saveError}</p>}
        <div className="row-actions"><button className="button primary" type="button" disabled={busy} onClick={() => void save()}>{busy ? "保存中…" : "分類を確定"}</button><button type="button" className="button" disabled={busy} onClick={postpone}>後で分類する</button></div>
        <p className="meta">後回しにしても商品は登録されたままです。新しい商品が届くと再表示します。左下の「取得した商品の分類」からも再開できます。</p>
      </>}
    </dialog>
  </>;
}
