import { useEffect, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import type { AvatarRecord, BaseBody, ExtensionLink, Knowledge, Settings } from "../types";
import { errorText } from "../ui";

const emptyAvatar = {
  id: "",
  displayName: "",
  baseBody: "",
  aliases: "",
  notes: "",
  builtin: false,
};

export function SettingsPage() {
  const { revision, refresh, notify, fail } = useShell();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [avatar, setAvatar] = useState(emptyAvatar);
  const [body, setBody] = useState({ id: "", displayName: "", note: "" });
  const [filter, setFilter] = useState("");
  const [status, setStatus] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [link, setLink] = useState<ExtensionLink | null>(null);

  useEffect(() => {
    let cancelled = false;
    api.testOllama().then(
      (result) => {
        if (!cancelled) setModels(result.models);
      },
      () => {
        if (!cancelled) setModels([]);
      },
    );
    Promise.all([api.settings(), api.knowledge(), api.extensionLink()]).then(
      ([nextSettings, nextKnowledge, nextLink]) => {
        if (cancelled) return;
        setSettings(nextSettings);
        setKnowledge(nextKnowledge);
        setLink(nextLink);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision, fail]);

  async function regenerateToken() {
    if (!window.confirm("コードを作り直すと、貼ってあるコードは使えなくなり、結び付いた拡張も外れます。")) return;
    try {
      setLink(await api.regenerateExtensionToken());
      notify("ペアリングコードを作り直しました");
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function setBuiltinBrowser(enabled: boolean) {
    if (!settings) return;
    if (enabled && !settings.builtinBrowser) {
      const accepted = window.confirm(
        "内蔵ブラウザでは、このアプリの中で BOOTH / pixiv にメールアドレスとパスワードでログインします。ログイン情報はアプリ専用のフォルダに保存され、アプリはクッキーを読み出したり外へ送ったりしません。拡張機能でページを書き出すより、アカウントへのリスク（制限や停止を含む）が高くなることがあります。年齢確認を自動で進めることはありません。ページを自動で辿ることもしません。ご自身の判断で使ってください。有効にしますか？",
      );
      if (!accepted) return;
    }
    try {
      const next = { ...settings, builtinBrowser: enabled };
      await api.saveSettings(next);
      if (!enabled) {
        try {
          await api.closeBoothBrowser();
        } catch {
          // 見本データにはブラウザウィンドウがない。
        }
      }
      setSettings(next);
      notify(enabled ? "内蔵ブラウザを有効にしました" : "内蔵ブラウザを無効にしました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function saveSettings() {
    if (!settings) return;
    try {
      await api.saveSettings(settings);
      notify("設定を保存しました");
      refresh();
      try {
        const result = await api.testOllama();
        setModels(result.models);
      } catch {
        setModels([]);
      }
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function testOllama() {
    try {
      const result = await api.testOllama();
      setStatus(
        result.modelReady
          ? `接続できました。設定中のモデルがあります（${result.models.length} 件）`
          : `接続できましたが、設定中のモデルが見つかりません。導入済み: ${result.models.join(", ") || "なし"}`,
      );
    } catch (err) {
      setStatus(errorText(err));
    }
  }

  function selectAvatar(record: AvatarRecord) {
    setAvatar({
      id: record.id,
      displayName: record.displayName,
      baseBody: record.baseBody ?? "",
      aliases: record.aliases.join(", "),
      notes: record.notes,
      builtin: record.isBuiltin,
    });
  }

  async function saveAvatar() {
    try {
      const aliases = avatar.aliases
        .split(/[,、\n]/)
        .map((item) => item.trim())
        .filter(Boolean);
      await api.saveAvatar({
        id: avatar.id || null,
        displayName: avatar.displayName,
        baseBody: avatar.baseBody || null,
        aliases,
        notes: avatar.notes,
      });
      notify("対応表を保存し、紐づけを再計算しました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function removeAvatar() {
    if (!avatar.id || avatar.builtin) return;
    try {
      await api.deleteAvatar(avatar.id);
      setAvatar(emptyAvatar);
      notify("アバターを削除し、紐づけを再計算しました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function saveBody() {
    try {
      await api.saveBaseBody({
        id: body.id || null,
        displayName: body.displayName,
        note: body.note,
      });
      notify("素体を保存しました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function removeBody() {
    if (!body.id) return;
    try {
      await api.deleteBaseBody(body.id);
      setBody({ id: "", displayName: "", note: "" });
      notify("素体を削除しました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function restore() {
    try {
      const added = await api.restoreSeed();
      notify(`初期データの不足分を ${added} 件追加しました`);
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  async function reresolve() {
    try {
      const changed = await api.reresolve();
      notify(`${changed} 件の紐づけを再計算しました`);
      refresh();
    } catch (err) {
      fail(errorText(err));
    }
  }

  const avatars = (knowledge?.avatars ?? []).filter((record) => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return true;
    return [record.displayName, ...record.aliases].join(" ").toLowerCase().includes(needle);
  });

  return (
    <section>
      <div className="page-head">
        <div>
          <h1>設定</h1>
          <p>分類は手元の Ollama だけに頼み、素体の関係はここにある対応表で決めます。</p>
        </div>
      </div>
      {link && (
        <div className="panel" style={{ padding: 16, marginBottom: 16 }}>
          <h2>拡張との接続</h2>
          <p className="meta">
            拡張は今開いている Booth のページだけを読み、この PC の 127.0.0.1:{link.port} に送ります。
            アプリは Booth を取りに行きません。下のコードを拡張のポップアップに一度貼ってください。
            最初に届いた拡張だけを覚えます。コードを作り直すと、その結び付きも外れます。
            違うコードが続くと、しばらく受け付けません。
          </p>
          <label className="field">
            <span>ペアリングコード</span>
            <input readOnly value={link.token} />
          </label>
          <p className="meta">
            {link.listening
              ? `127.0.0.1:${link.port} で待っています。`
              : link.error || "受信を開始できませんでした。"}
            {` アプリが閉じているときは、拡張が vrclibman://import で起動します。`}
          </p>
          <div className="row-actions">
            <button className="button" type="button" onClick={() => void regenerateToken()}>
              コードを作り直す
            </button>
          </div>
        </div>
      )}
      {settings && (
        <div className="panel" style={{ padding: 16, marginBottom: 16 }}>
          <h2>内蔵ブラウザ</h2>
          <p className="meta">
            既定は拡張機能です。内蔵ブラウザは、自分でログインして今開いているライブラリのページだけを取り込む別のモードです。
            自動でページを辿ったり、一括でダウンロードしたりしません。年齢確認も自動では進みません。
            ログイン状態はこのアプリ専用のフォルダに残り、ログアウトでそのフォルダを消します。
          </p>
          <div className="row-actions">
            {settings.builtinBrowser ? (
              <>
                <button className="button" type="button" onClick={() => void setBuiltinBrowser(false)}>
                  内蔵ブラウザを無効にする
                </button>
                <button
                  className="button danger"
                  type="button"
                  onClick={() =>
                    void (async () => {
                      if (!window.confirm("ブラウザを閉じ、このアプリの中の BOOTH のログイン状態を消します。")) return;
                      try {
                        await api.clearBoothSession();
                        notify("セッションを消しました");
                      } catch (err) {
                        fail(errorText(err));
                      }
                    })()
                  }
                >
                  ログアウトしてセッションを消す
                </button>
              </>
            ) : (
              <button className="button primary" type="button" onClick={() => void setBuiltinBrowser(true)}>
                内蔵ブラウザを有効にする
              </button>
            )}
          </div>
        </div>
      )}
      {settings && (
        <div className="panel" style={{ padding: 16, marginBottom: 16 }}>
          <div className="form-grid">
            <label className="field">
              <span>Ollama の URL</span>
              <input
                value={settings.ollamaBaseUrl}
                onChange={(event) => setSettings({ ...settings, ollamaBaseUrl: event.target.value })}
              />
            </label>
            <label className="field">
              <span>モデル名</span>
              <input
                list="ollama-models"
                value={settings.ollamaModel}
                onChange={(event) => setSettings({ ...settings, ollamaModel: event.target.value })}
              />
              <datalist id="ollama-models">
                {models.map((model) => (
                  <option key={model} value={model} />
                ))}
              </datalist>
            </label>
            <label className="field">
              <span>確信度のしきい値</span>
              <input
                type="number"
                min={0}
                max={1}
                step={0.05}
                value={settings.confidenceThreshold}
                onChange={(event) =>
                  setSettings({ ...settings, confidenceThreshold: Number(event.target.value) })
                }
              />
            </label>
            <label className="field">
              <span>最後に調べたフォルダ</span>
              <input value={settings.libraryFolder} readOnly />
            </label>
          </div>
          <div className="row-actions">
            <button className="button primary" type="button" onClick={() => void saveSettings()}>
              設定を保存
            </button>
            <button className="button" type="button" onClick={() => void testOllama()}>
              接続を試す
            </button>
          </div>
          {status && <p className="meta">{status}</p>}
          <p className="meta">
            初期モデルは gemma4:12b です（目安 8GB の VRAM）。軽い代替は qwen3:4b。
            {models.length > 0
              ? ` 導入済み: ${models.join("、")}。上の欄で選べます。`
              : " 「接続を試す」で導入済みのモデルを候補に出します。"}
          </p>
          <p className="meta">
            Ollama は http://127.0.0.1 または localhost だけを受け付けます。温度は 0.1 固定です。要確認は、モデルが自分で付けた数値ではなく、名前が商品文と対応表に結び付いたかで決めます。
          </p>
        </div>
      )}
      <div className="row-actions" style={{ marginBottom: 16 }}>
        <button className="button" type="button" onClick={() => void restore()}>
          初期データを追加
        </button>
        <button className="button" type="button" onClick={() => void reresolve()}>
          対応表を再適用
        </button>
      </div>
      <div className="knowledge">
        <div>
          <input className="search" value={filter} placeholder="対応表を検索" onChange={(event) => setFilter(event.target.value)} />
          <div className="list">
            {avatars.map((record) => (
              <button
                key={record.id}
                className={record.id === avatar.id ? "selected" : ""}
                type="button"
                onClick={() => selectAvatar(record)}
              >
                {record.displayName}
                {record.isBuiltin ? " ・初期" : ""}
              </button>
            ))}
          </div>
          <h2>素体</h2>
          <div className="list">
            {(knowledge?.baseBodies ?? []).map((record) => (
              <button key={record.id} type="button" onClick={() => setBody(record)}>
                {record.displayName}
              </button>
            ))}
          </div>
        </div>
        <div className="stack">
          <div className="panel" style={{ padding: 16 }}>
            <h2 style={{ marginTop: 0 }}>{avatar.id ? "アバターを編集" : "アバターを追加"}</h2>
            <label className="field">
              <span>表示名</span>
              <input value={avatar.displayName} onChange={(event) => setAvatar({ ...avatar, displayName: event.target.value })} />
            </label>
            <label className="field">
              <span>別名（カンマ区切り）</span>
              <input value={avatar.aliases} onChange={(event) => setAvatar({ ...avatar, aliases: event.target.value })} />
            </label>
            <label className="field">
              <span>素体</span>
              <select value={avatar.baseBody} onChange={(event) => setAvatar({ ...avatar, baseBody: event.target.value })}>
                <option value="">なし</option>
                {(knowledge?.baseBodies ?? []).map((record: BaseBody) => (
                  <option key={record.id} value={record.id}>
                    {record.displayName}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span>メモ</span>
              <textarea value={avatar.notes} onChange={(event) => setAvatar({ ...avatar, notes: event.target.value })} />
            </label>
            <div className="row-actions">
              <button className="button primary" type="button" onClick={() => void saveAvatar()}>
                保存
              </button>
              <button className="button" type="button" onClick={() => setAvatar(emptyAvatar)}>
                新規
              </button>
              <button className="button danger" type="button" disabled={!avatar.id || avatar.builtin} onClick={() => void removeAvatar()}>
                削除
              </button>
            </div>
          </div>
          <div className="panel" style={{ padding: 16 }}>
            <h2 style={{ marginTop: 0 }}>{body.id ? "素体を編集" : "素体を追加"}</h2>
            <label className="field">
              <span>表示名</span>
              <input value={body.displayName} onChange={(event) => setBody({ ...body, displayName: event.target.value })} />
            </label>
            <label className="field">
              <span>根拠のメモ</span>
              <textarea value={body.note} onChange={(event) => setBody({ ...body, note: event.target.value })} />
            </label>
            <div className="row-actions">
              <button className="button primary" type="button" onClick={() => void saveBody()}>
                保存
              </button>
              <button className="button" type="button" onClick={() => setBody({ id: "", displayName: "", note: "" })}>
                新規
              </button>
              <button className="button danger" type="button" disabled={!body.id} onClick={() => void removeBody()}>
                削除
              </button>
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
