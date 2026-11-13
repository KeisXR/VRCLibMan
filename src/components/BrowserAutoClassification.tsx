import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { SyntheticEvent } from "react";
import { api, isTauri } from "../api";
import { useShell } from "../App";
import { postponeShown, promptOpen, reconcilePostponed, showsResume } from "../browserAutoPrompt";
import { GENRE_LABEL, genresForKind } from "../types";
import type { GenreKind, GenreSuggestion, Knowledge } from "../types";
import { errorText } from "../ui";

type Item = { boothId: string; title: string };
type Kind = GenreKind | "avatar";

/** 候補の出どころを、確定前の注記として出す文。 */
export const SUGGESTION_SOURCE_LABEL: Record<NonNullable<GenreSuggestion["source"]>, string> = {
  classified: "自動の分類の結果を候補にしています。",
  booth: "BOOTH のカテゴリとタグから候補を選んでいます。",
  rules: "タイトルと説明から候補を選んでいます。",
};

/** 確定できるか。種類と、アセット・ワールドなら分類先を本人が選んでいる（候補でもよい）こと。 */
export function autoClassificationReady(kind: Kind | "", genre: string): boolean {
  if (kind === "") return false;
  return kind === "avatar" || genre !== "";
}

export function BrowserAutoClassification() {
  const { revision, refresh, fail, notify } = useShell();
  const [items, setItems] = useState<Item[]>([]);
  const [postponedIds, setPostponedIds] = useState<readonly string[]>([]);
  // 決め打ちの初期値（衣装）は置かない。候補が無ければ本人が選ぶまで確定できない（確定は手動の固定になるため）。
  const [kind, setKind] = useState<Kind | "">("");
  const [genre, setGenre] = useState("");
  const [suggestion, setSuggestion] = useState<GenreSuggestion | null>(null);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [preferredId, setPreferredId] = useState<string | null>(null);
  const [avatars, setAvatars] = useState<Knowledge["avatars"]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [unresolved, setUnresolved] = useState<string[]>([]);
  const [filter, setFilter] = useState("");
  const [loadingAvatars, setLoadingAvatars] = useState(false);
  const [avatarError, setAvatarError] = useState<string | null>(null);
  const [avatarReload, setAvatarReload] = useState(0);
  const dialog = useRef<HTMLDialogElement>(null);
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const current = items[0];
  const itemIds = items.map((item) => item.boothId);
  const open = promptOpen(itemIds, postponedIds);

  useEffect(() => {
    let alive = true;
    void api.browserAutoPending().then(value => { if (alive) setItems(value.sort((a, b) => Number(b.boothId === preferredId) - Number(a.boothId === preferredId))); }, err => { if (alive) fail(errorText(err)); });
    return () => { alive = false; };
  }, [revision, reload, preferredId, fail]);

  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let stop = () => {};
    void import("@tauri-apps/api/event").then(({ listen }) => Promise.all([
      listen("browser-auto-pending", () => { if (alive) setReload(value => value + 1); }),
      listen<string>("extension-import-prompt", event => {
        if (!alive) return;
        setPreferredId(event.payload); setPostponedIds([]); setReload(value => value + 1);
      }),
    ])).then(stops => { const unlisten = () => stops.forEach(fn => fn()); if (alive) stop = unlisten; else unlisten(); });
    return () => { alive = false; stop(); };
  }, []);

  useEffect(() => {
    setPostponedIds((prev) => reconcilePostponed(prev, items.map((item) => item.boothId)));
  }, [items]);

  useEffect(() => {
    setKind(""); setGenre(""); setSuggestion(null); setName(""); setSaveError(null);
    if (!current) return;
    let alive = true;
    void api.productGenreSuggestion(current.boothId).then(
      (value) => {
        if (!alive) return;
        setSuggestion(value);
        if (value.kind) { setKind(value.kind); setGenre(value.genre ?? ""); }
      },
      // 候補を読めなくても、本人が選べば確定できる。
      () => undefined,
    );
    return () => { alive = false; };
  }, [current?.boothId]);

  useEffect(() => {
    if (!current) return;
    let alive = true;
    setSelected([]); setUnresolved([]); setFilter(""); setAvatarError(null); setLoadingAvatars(true);
    void Promise.all([api.knowledge(), api.productForEdit(current.boothId), api.productAvatarSuggestions(current.boothId)])
      .then(([knowledge, product, suggestions]) => {
        if (!alive) return;
        setAvatars(knowledge.avatars);
        setSelected([...new Set([...product.linkedAvatarIds, ...suggestions.avatarIds])]);
        setUnresolved(suggestions.unresolvedNames);
      }, err => { if (alive) setAvatarError(errorText(err)); })
      .finally(() => { if (alive) setLoadingAvatars(false); });
    return () => { alive = false; };
  }, [current?.boothId, avatarReload]);

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
      } else if (kind !== "" && genre) { await api.moveProductGenre(current.boothId, kind, genre, kind === "asset" ? selected : undefined); }
      else throw new Error("種類と分類先を選んでください。");
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
        <p className="meta" role="status">
          {suggestion?.source ? `${SUGGESTION_SOURCE_LABEL[suggestion.source]}違っていたら選び直してください。` : "種類と分類先を選んでください。"}
        </p>
        <label className="field"><span>種類</span><select autoFocus disabled={busy} value={kind} onChange={event => {
          const next = event.target.value as Kind | "";
          setKind(next);
          // 候補と同じ種類に戻したときだけ候補の分類先を入れ、それ以外は選び直してもらう。
          setGenre(next !== "" && next === suggestion?.kind ? suggestion.genre ?? "" : "");
        }}>
          {kind === "" && <option value="">選んでください</option>}
          <option value="asset">アセット</option><option value="world">ワールド</option><option value="avatar">アバター本体</option>
        </select></label>
        {kind === "avatar" ? <label className="field"><span>アバター名（自動で決まらない場合）</span><input disabled={busy} value={name} onChange={event => setName(event.target.value)} /></label>
          : kind !== "" && <label className="field"><span>分類先</span><select disabled={busy} value={genre} onChange={event => setGenre(event.target.value)}>
            {genre === "" && <option value="">選んでください</option>}
            {genresForKind(kind).map(value => <option key={value} value={value}>{GENRE_LABEL[value] ?? value}</option>)}
          </select></label>}
        {kind === "asset" && <div className="field">
          <span>対応アバター</span>
          <p className="meta">ページの対応表記から自動入力します。不要なものは×で外し、検索して追加できます。</p>
          {loadingAvatars && <p role="status">ページの対応アバターを読み取り中…</p>}
          {avatarError && <p className="banner error" role="alert">{avatarError}<button type="button" className="button" onClick={() => setAvatarReload(value => value + 1)}>再読み込み</button></p>}
          <div className="pills">{selected.map(id => <span className="auto-avatar-chip" key={id}>
            {avatars.find(a => a.id === id)?.displayName ?? id}
            <button type="button" disabled={busy} aria-label={`${avatars.find(a => a.id === id)?.displayName ?? id}を外す`} onClick={() => setSelected(value => value.filter(item => item !== id))}>×</button>
          </span>)}</div>
          {!loadingAvatars && !avatarError && selected.length === 0 && <p className="meta">対応アバターは未設定です。</p>}
          {unresolved.length > 0 && <p className="meta">対応表にない名前：{unresolved.join("、")}（自動では追加しません）</p>}
          <input aria-label="対応アバターを検索" placeholder="名前・別名で検索して追加" disabled={busy || loadingAvatars} value={filter} onChange={event => setFilter(event.target.value)} />
          {filter.trim() && <div className="auto-avatar-results">{avatars.filter(a => !selected.includes(a.id) && [a.displayName, ...a.aliases].some(n => n.toLocaleLowerCase().includes(filter.trim().toLocaleLowerCase()))).slice(0,30).map(a => <button className="button" type="button" disabled={busy} key={a.id} onClick={() => {setSelected(value => [...value, a.id]); setFilter("");}}>{a.displayName}を追加</button>)}</div>}
        </div>}
        {saveError && <p className="banner error" role="alert">{saveError}</p>}
        <div className="row-actions"><button className="button primary" type="button" disabled={busy || !autoClassificationReady(kind, genre) || (kind === "asset" && (loadingAvatars || !!avatarError))} onClick={() => void save()}>{busy ? "保存中…" : "分類を確定"}</button><button type="button" className="button" disabled={busy} onClick={postpone}>後で分類する</button></div>
        <p className="meta">後回しにしても商品は登録されたままです。新しい商品が届くと再表示します。左下の「取得した商品の分類」からも再開できます。</p>
      </>}
    </dialog>
  </>;
}
