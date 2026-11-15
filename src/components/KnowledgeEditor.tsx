import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { api } from "../api";
import type { AvatarRecord, BaseBody, Knowledge } from "../types";
import { errorText } from "../ui";

/** 一覧に一度に並べる上限。同梱の対応表は 900 件を超えるので、残りは検索で絞って出す。 */
const LIST_LIMIT = 200;

type Mode = "avatars" | "bodies";
type AvatarFilter = "all" | "owned" | "custom" | "nobody";

interface AvatarDraft {
  id: string;
  displayName: string;
  baseBody: string;
  aliases: string[];
  notes: string;
  builtin: boolean;
}

interface BodyDraft {
  id: string;
  displayName: string;
  note: string;
}

const NEW_AVATAR: AvatarDraft = { id: "", displayName: "", baseBody: "", aliases: [], notes: "", builtin: false };
const NEW_BODY: BodyDraft = { id: "", displayName: "", note: "" };

function avatarDraft(record: AvatarRecord): AvatarDraft {
  return {
    id: record.id,
    displayName: record.displayName,
    baseBody: record.baseBody ?? "",
    aliases: [...record.aliases],
    notes: record.notes,
    builtin: record.isBuiltin,
  };
}

function bodyDraft(record: BaseBody): BodyDraft {
  return { id: record.id, displayName: record.displayName, note: record.note };
}

function sameAvatar(a: AvatarDraft, b: AvatarDraft) {
  return a.displayName.trim() === b.displayName.trim()
    && a.baseBody === b.baseBody
    && a.notes === b.notes
    && a.aliases.join("\n") === b.aliases.join("\n");
}

function sameBody(a: BodyDraft, b: BodyDraft) {
  return a.displayName.trim() === b.displayName.trim() && a.note === b.note;
}

/** 区切り（カンマ・読点・改行）で分け、空を除く。 */
function splitAliases(text: string) {
  return text.split(/[,、，\n]/).map((item) => item.trim()).filter(Boolean);
}

function matches(needle: string, ...values: (string | null | undefined)[]) {
  if (!needle) return true;
  return values.join(" ").toLowerCase().includes(needle);
}

interface Props {
  knowledge: Knowledge;
  onChanged: () => void;
  notify: (message: string) => void;
  fail: (message: string) => void;
}

/** 設定の「対応表を編集」。アバターと素体を切り替えて、一覧から選んだ 1 件を右で編集する。 */
export function KnowledgeEditor({ knowledge, onChanged, notify, fail }: Props) {
  const [mode, setMode] = useState<Mode>("avatars");
  const [filter, setFilter] = useState("");
  const [avatarFilter, setAvatarFilter] = useState<AvatarFilter>("all");
  const [avatarBase, setAvatarBase] = useState<AvatarDraft>(NEW_AVATAR);
  const [avatar, setAvatar] = useState<AvatarDraft>(NEW_AVATAR);
  const [aliasInput, setAliasInput] = useState("");
  const [bodyBase, setBodyBase] = useState<BodyDraft>(NEW_BODY);
  const [body, setBody] = useState<BodyDraft>(NEW_BODY);
  const [memberQuery, setMemberQuery] = useState("");
  const [busy, setBusy] = useState(false);

  const bodies = knowledge.baseBodies;
  const bodyName = useMemo(() => new Map(bodies.map((record) => [record.id, record.displayName])), [bodies]);
  const membersOf = useMemo(() => {
    const map = new Map<string, AvatarRecord[]>();
    for (const record of knowledge.avatars) {
      if (!record.baseBody) continue;
      const list = map.get(record.baseBody) ?? [];
      list.push(record);
      map.set(record.baseBody, list);
    }
    return map;
  }, [knowledge.avatars]);

  const avatarDirty = !sameAvatar(avatar, avatarBase) || aliasInput.trim() !== "";
  const bodyDirty = !sameBody(body, bodyBase);
  const dirty = mode === "avatars" ? avatarDirty : bodyDirty;

  // 対応表を読み直したとき（素体の出し入れや別画面の変更）、未編集なら開いている項目を新しい値にそろえる。
  useEffect(() => {
    if (avatar.id && !avatarDirty) {
      const record = knowledge.avatars.find((item) => item.id === avatar.id);
      const next = record ? avatarDraft(record) : NEW_AVATAR;
      setAvatar(next);
      setAvatarBase(next);
    }
    if (body.id && !bodyDirty) {
      const record = bodies.find((item) => item.id === body.id);
      const next = record ? bodyDraft(record) : NEW_BODY;
      setBody(next);
      setBodyBase(next);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [knowledge]);

  function leaveAllowed() {
    return !dirty || window.confirm("保存していない変更があります。破棄して移動しますか？");
  }

  function openAvatar(next: AvatarDraft) {
    if (!leaveAllowed()) return;
    setMode("avatars");
    setAvatar(next);
    setAvatarBase(next);
    setAliasInput("");
  }

  function openBody(next: BodyDraft) {
    if (!leaveAllowed()) return;
    setMode("bodies");
    setBody(next);
    setBodyBase(next);
    setMemberQuery("");
  }

  function switchMode(next: Mode) {
    if (next === mode || !leaveAllowed()) return;
    if (mode === "avatars") { setAvatar(avatarBase); setAliasInput(""); } else setBody(bodyBase);
    setMode(next);
    setFilter("");
  }

  function addAliases(text: string) {
    const added = splitAliases(text);
    if (!added.length) { setAliasInput(""); return; }
    const seen = new Set(avatar.aliases.map((item) => item.toLowerCase()));
    const fresh = added.filter((item) => {
      const key = item.toLowerCase();
      if (seen.has(key) || key === avatar.displayName.trim().toLowerCase()) return false;
      seen.add(key);
      return true;
    });
    setAvatar({ ...avatar, aliases: [...avatar.aliases, ...fresh] });
    setAliasInput("");
  }

  function aliasKey(event: KeyboardEvent<HTMLInputElement>) {
    if (event.nativeEvent.isComposing) return;
    if (event.key === "Enter" || event.key === "," || event.key === "、") {
      event.preventDefault();
      addAliases(aliasInput);
    } else if (event.key === "Backspace" && !aliasInput && avatar.aliases.length) {
      setAvatar({ ...avatar, aliases: avatar.aliases.slice(0, -1) });
    }
  }

  async function run(task: () => Promise<void>) {
    setBusy(true);
    try { await task(); } catch (err) { fail(errorText(err)); } finally { setBusy(false); }
  }

  function saveAvatar() {
    const aliases = [...avatar.aliases];
    for (const alias of splitAliases(aliasInput)) if (!aliases.includes(alias)) aliases.push(alias);
    void run(async () => {
      const id = await api.saveAvatar({
        id: avatar.id || null,
        displayName: avatar.displayName,
        baseBody: avatar.baseBody || null,
        aliases,
        notes: avatar.notes,
      });
      const saved = { ...avatar, id, displayName: avatar.displayName.trim(), aliases };
      setAvatar(saved);
      setAvatarBase(saved);
      setAliasInput("");
      notify(avatar.id ? "対応表を保存し、紐づけを再計算しました" : "アバターを追加し、紐づけを再計算しました");
      onChanged();
    });
  }

  function removeAvatar() {
    if (!avatar.id || avatar.builtin) return;
    if (!window.confirm("「" + avatarBase.displayName + "」を対応表から削除します。このアバターに結び付いた商品の対応は計算し直します。元に戻せません。続けますか？")) return;
    void run(async () => {
      await api.deleteAvatar(avatar.id);
      setAvatar(NEW_AVATAR);
      setAvatarBase(NEW_AVATAR);
      notify("アバターを削除し、紐づけを再計算しました");
      onChanged();
    });
  }

  function saveBody() {
    void run(async () => {
      const id = await api.saveBaseBody({ id: body.id || null, displayName: body.displayName, note: body.note });
      const saved = { ...body, id, displayName: body.displayName.trim() };
      setBody(saved);
      setBodyBase(saved);
      notify(body.id ? "素体を保存しました" : "素体を追加しました。続けてアバターを入れられます");
      onChanged();
    });
  }

  function removeBody() {
    if (!body.id) return;
    if (!window.confirm("素体「" + bodyBase.displayName + "」を削除します。この素体に入っているアバターは素体なしになり、共通素体を通した衣装の表示が無くなります。初期データの素体は「デバッグ → 初期データを追加」で戻せます。続けますか？")) return;
    void run(async () => {
      await api.deleteBaseBody(body.id);
      setBody(NEW_BODY);
      setBodyBase(NEW_BODY);
      notify("素体を削除しました");
      onChanged();
    });
  }

  /** 素体の側からアバターを出し入れする。アバターの保存と同じく、その場で紐づけを計算し直す。 */
  function setMembership(record: AvatarRecord, baseBody: string | null) {
    void run(async () => {
      await api.saveAvatar({
        id: record.id,
        displayName: record.displayName,
        baseBody,
        aliases: record.aliases,
        notes: record.notes,
      });
      setMemberQuery("");
      notify(baseBody
        ? "「" + record.displayName + "」を「" + (bodyName.get(baseBody) ?? body.displayName) + "」に入れました"
        : "「" + record.displayName + "」を素体から外しました");
      onChanged();
    });
  }

  const needle = filter.trim().toLowerCase();
  const matchedAvatars = knowledge.avatars
    .filter((record) => {
      if (avatarFilter === "owned" && !record.owned) return false;
      if (avatarFilter === "custom" && record.isBuiltin) return false;
      if (avatarFilter === "nobody" && record.baseBody) return false;
      return matches(needle, record.displayName, ...record.aliases, record.baseBody ? bodyName.get(record.baseBody) : "");
    })
    .sort((a, b) => Number(b.owned) - Number(a.owned));
  const shownAvatars = matchedAvatars.slice(0, LIST_LIMIT);
  const matchedBodies = bodies.filter((record) =>
    matches(needle, record.displayName, record.note, ...(membersOf.get(record.id) ?? []).map((item) => item.displayName)));
  const shown = mode === "avatars" ? shownAvatars.length : matchedBodies.length;
  const total = mode === "avatars" ? knowledge.avatars.length : bodies.length;

  const siblings = avatar.baseBody ? (membersOf.get(avatar.baseBody) ?? []).filter((item) => item.id !== avatar.id) : [];
  const members = body.id ? (membersOf.get(body.id) ?? []) : [];
  const memberNeedle = memberQuery.trim().toLowerCase();
  const candidates = memberNeedle
    ? knowledge.avatars
      .filter((record) => record.baseBody !== body.id && matches(memberNeedle, record.displayName, ...record.aliases))
      .sort((a, b) => Number(b.owned) - Number(a.owned))
      .slice(0, 8)
    : [];

  return (
    <div className="knowledge">
      <div className="knowledge-browser">
        <div className="tabs knowledge-tabs" role="tablist" aria-label="編集する対象">
          <button type="button" role="tab" aria-selected={mode === "avatars"} className={"tab" + (mode === "avatars" ? " active" : "")} onClick={() => switchMode("avatars")}>
            アバター <span className="tab-count">{knowledge.avatars.length}</span>
          </button>
          <button type="button" role="tab" aria-selected={mode === "bodies"} className={"tab" + (mode === "bodies" ? " active" : "")} onClick={() => switchMode("bodies")}>
            素体 <span className="tab-count">{bodies.length}</span>
          </button>
        </div>
        <input
          className="search"
          type="search"
          value={filter}
          aria-label={mode === "avatars" ? "アバターを検索" : "素体を検索"}
          placeholder={mode === "avatars" ? "名前・別名・素体名で検索" : "素体名・アバター名で検索"}
          onChange={(event) => setFilter(event.target.value)}
        />
        {mode === "avatars" && (
          <select className="search knowledge-filter" aria-label="表示するアバター" value={avatarFilter} onChange={(event) => setAvatarFilter(event.target.value as AvatarFilter)}>
            <option value="all">すべてのアバター</option>
            <option value="owned">持っているアバター</option>
            <option value="custom">自分で追加したアバター</option>
            <option value="nobody">素体が未設定</option>
          </select>
        )}
        <div className="knowledge-list-head">
          <p className="meta">{total} 件中 {shown} 件を表示</p>
          <button className="button" type="button" onClick={() => (mode === "avatars" ? openAvatar(NEW_AVATAR) : openBody(NEW_BODY))}>
            ＋ 追加
          </button>
        </div>
        <div className="list knowledge-list" aria-label={mode === "avatars" ? "アバター" : "素体"}>
          {mode === "avatars" && shownAvatars.map((record) => (
            <button key={record.id} type="button" aria-current={record.id === avatar.id || undefined} className={record.id === avatar.id ? "selected" : ""} onClick={() => openAvatar(avatarDraft(record))}>
              <span className="knowledge-item-name">
                {record.displayName}
                {record.owned && <span className="badge recommend">所持</span>}
                {!record.isBuiltin && <span className="badge">追加</span>}
              </span>
              <span className="knowledge-item-sub">{record.baseBody ? bodyName.get(record.baseBody) ?? "素体なし" : "素体なし"}{record.aliases.length ? "・" + record.aliases.slice(0, 3).join("、") : ""}</span>
            </button>
          ))}
          {mode === "bodies" && matchedBodies.map((record) => (
            <button key={record.id} type="button" aria-current={record.id === body.id || undefined} className={record.id === body.id ? "selected" : ""} onClick={() => openBody(bodyDraft(record))}>
              <span className="knowledge-item-name">{record.displayName}</span>
              <span className="knowledge-item-sub">{(membersOf.get(record.id) ?? []).length} 体：{(membersOf.get(record.id) ?? []).slice(0, 4).map((item) => item.displayName).join("、") || "まだ入っていません"}</span>
            </button>
          ))}
          {shown === 0 && <p className="meta knowledge-empty">見つかりません。</p>}
          {mode === "avatars" && matchedAvatars.length > LIST_LIMIT && <p className="meta knowledge-empty">ほかに {matchedAvatars.length - LIST_LIMIT} 件あります。検索で絞り込んでください。</p>}
        </div>
      </div>

      {mode === "avatars" ? (
        <div className="panel knowledge-editor">
          <header className="knowledge-editor-head">
            <h2>{avatar.id ? avatarBase.displayName : "アバターを追加"}</h2>
            {avatarDirty && <span className="knowledge-dirty">未保存の変更があります</span>}
          </header>
          <label className="field"><span>表示名</span><input value={avatar.displayName} maxLength={80} onChange={(event) => setAvatar({ ...avatar, displayName: event.target.value })} /></label>
          <div className="field">
            <span id="knowledge-alias-label">別名</span>
            <div className="alias-editor">
              {avatar.aliases.map((alias) => (
                <span className="alias-chip" key={alias}>
                  {alias}
                  <button type="button" aria-label={"別名「" + alias + "」を外す"} onClick={() => setAvatar({ ...avatar, aliases: avatar.aliases.filter((item) => item !== alias) })}>×</button>
                </span>
              ))}
              <input
                aria-labelledby="knowledge-alias-label"
                value={aliasInput}
                placeholder={avatar.aliases.length ? "別名を足す" : "例：Lime、らいむ"}
                onChange={(event) => setAliasInput(event.target.value)}
                onKeyDown={aliasKey}
                onBlur={() => { if (aliasInput.trim()) addAliases(aliasInput); }}
                onPaste={(event) => {
                  const pasted = event.clipboardData.getData("text");
                  if (/[,、，\n]/.test(pasted)) { event.preventDefault(); addAliases(aliasInput + pasted); }
                }}
              />
            </div>
            <p className="meta">商品名や説明にこの名前があると、このアバターに結び付けます。Enter か読点で区切って足します。</p>
          </div>
          <label className="field">
            <span>素体</span>
            <select value={avatar.baseBody} onChange={(event) => setAvatar({ ...avatar, baseBody: event.target.value })}>
              <option value="">なし</option>
              {bodies.map((record) => <option key={record.id} value={record.id}>{record.displayName}</option>)}
            </select>
          </label>
          {avatar.baseBody && (
            <div className="knowledge-siblings">
              <span className="meta">同じ素体</span>
              {siblings.map((record) => (
                <button key={record.id} type="button" className="alias-chip link" onClick={() => openAvatar(avatarDraft(record))}>{record.displayName}</button>
              ))}
              {!siblings.length && <span className="meta">ほかにありません</span>}
              <button type="button" className="text-link" onClick={() => { const record = bodies.find((item) => item.id === avatar.baseBody); if (record) openBody(bodyDraft(record)); }}>素体を開く</button>
            </div>
          )}
          <label className="field"><span>メモ</span><textarea value={avatar.notes} maxLength={400} onChange={(event) => setAvatar({ ...avatar, notes: event.target.value })} /></label>
          <div className="row-actions">
            <button className="button primary" type="button" disabled={busy || !avatar.displayName.trim() || (!!avatar.id && !avatarDirty)} onClick={saveAvatar}>{avatar.id ? "保存" : "追加"}</button>
            <button className="button" type="button" disabled={busy || !avatarDirty} onClick={() => { setAvatar(avatarBase); setAliasInput(""); }}>変更を取り消す</button>
            {avatar.id && !avatar.builtin && <button className="button danger" type="button" disabled={busy} onClick={removeAvatar}>削除</button>}
            {avatar.id && avatar.builtin && <span className="meta">初期データのアバターは削除できません</span>}
          </div>
        </div>
      ) : (
        <div className="panel knowledge-editor">
          <header className="knowledge-editor-head">
            <h2>{body.id ? bodyBase.displayName : "素体を追加"}</h2>
            {bodyDirty && <span className="knowledge-dirty">未保存の変更があります</span>}
          </header>
          <p className="meta">同じ素体に入れたアバターどうしは、片方向けの衣装をもう片方の詳細にも出します。髪は対象外です。</p>
          <label className="field"><span>表示名</span><input value={body.displayName} maxLength={80} onChange={(event) => setBody({ ...body, displayName: event.target.value })} /></label>
          <label className="field"><span>根拠のメモ</span><textarea value={body.note} maxLength={400} placeholder="例：作者が共通素体と案内している" onChange={(event) => setBody({ ...body, note: event.target.value })} /></label>
          <div className="row-actions">
            <button className="button primary" type="button" disabled={busy || !body.displayName.trim() || (!!body.id && !bodyDirty)} onClick={saveBody}>{body.id ? "保存" : "追加"}</button>
            <button className="button" type="button" disabled={busy || !bodyDirty} onClick={() => setBody(bodyBase)}>変更を取り消す</button>
            {body.id && <button className="button danger" type="button" disabled={busy} onClick={removeBody}>削除</button>}
          </div>
          <div className="knowledge-members">
            <h3>この素体のアバター</h3>
            {body.id ? <>
              <p className="meta">入れる・外すは押したときに保存します。</p>
              <div className="knowledge-member-list">
                {members.map((record) => (
                  <span className="alias-chip" key={record.id}>
                    <button type="button" className="text-link" onClick={() => openAvatar(avatarDraft(record))}>{record.displayName}</button>
                    <button type="button" aria-label={"「" + record.displayName + "」を素体から外す"} disabled={busy} onClick={() => setMembership(record, null)}>×</button>
                  </span>
                ))}
                {!members.length && <span className="meta">まだ入っていません。</span>}
              </div>
              <input className="search knowledge-member-search" type="search" value={memberQuery} aria-label="入れるアバターを検索" placeholder="入れるアバターを名前で探す" onChange={(event) => setMemberQuery(event.target.value)} />
              {candidates.length > 0 && (
                <div className="list knowledge-candidates">
                  {candidates.map((record) => (
                    <button key={record.id} type="button" disabled={busy} onClick={() => setMembership(record, body.id)}>
                      <span className="knowledge-item-name">＋ {record.displayName}{record.owned && <span className="badge recommend">所持</span>}</span>
                      {record.baseBody && <span className="knowledge-item-sub">今は「{bodyName.get(record.baseBody)}」。入れるとこちらへ移ります</span>}
                    </button>
                  ))}
                </div>
              )}
              {memberNeedle && !candidates.length && <p className="meta">入れられるアバターが見つかりません。</p>}
            </> : <p className="meta">素体を追加すると、アバターを入れられます。</p>}
          </div>
        </div>
      )}
    </div>
  );
}
