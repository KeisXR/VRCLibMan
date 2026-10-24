import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { api, chooseFolder, openExternal, saveLibraryAndScan } from "../api";
import { useShell } from "../App";
import { formatBytes } from "../media";
import { DEFAULT_OLLAMA_MODEL } from "../types";
import type { AppleFmStatus, AvatarRecord, BaseBody, CatalogInfo, ExtensionLink, Knowledge, Settings } from "../types";
import { errorText } from "../ui";

const emptyAvatar = {
  id: "",
  displayName: "",
  baseBody: "",
  aliases: "",
  notes: "",
  builtin: false,
};

function SettingRow({ label, help, children }: { label: string; help?: ReactNode; children: ReactNode }) {
  return (
    <div className="setting-row">
      <div className="setting-copy">
        <strong>{label}</strong>
        {help && <p>{help}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

export function SettingsPage() {
  const { revision, refresh, notify, fail } = useShell();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [savedFolder, setSavedFolder] = useState("");
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [avatar, setAvatar] = useState(emptyAvatar);
  const [body, setBody] = useState({ id: "", displayName: "", note: "" });
  const [filter, setFilter] = useState("");
  const [status, setStatus] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [installedSize, setInstalledSize] = useState<number | null>(null);
  const [link, setLink] = useState<ExtensionLink | null>(null);
  const [apple, setApple] = useState<AppleFmStatus | null>(null);
  const [catalog, setCatalog] = useState<CatalogInfo | null>(null);
  const [pulling, setPulling] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api.testOllama().then(
      (result) => {
        if (cancelled) return;
        setModels(result.models);
        setInstalledSize(result.modelSizeBytes);
      },
      () => {
        if (cancelled) return;
        setModels([]);
        setInstalledSize(null);
      },
    );
    Promise.all([api.settings(), api.knowledge(), api.extensionLink(), api.catalogInfo(), api.appleFmStatus()]).then(
      ([nextSettings, nextKnowledge, nextLink, nextCatalog, nextApple]) => {
        if (cancelled) return;
        setSettings(nextSettings);
        setSavedFolder(nextSettings.libraryFolder);
        setApple(nextApple);
        setCatalog(nextCatalog);
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
        "内蔵ブラウザでは、このアプリの中で BOOTH / pixiv にメールアドレスとパスワードでログインします。ログイン情報はアプリ専用のフォルダに保存され、アプリはクッキーを読み出したり外へ送ったりしません。拡張機能でページを書き出すより、アカウントへのリスク（制限や停止を含む）が高くなることがあります。年齢確認を自動で進めることはありません。ページを辿るのは、ブラウザ画面の「ライブラリ全取得」を押したときだけです。一括のダウンロードはしません。ご自身の判断で使ってください。有効にしますか？",
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
    const folder = settings.libraryFolder.trim();
    const folderChanged = folder !== savedFolder.trim();
    try {
      await api.saveSettings(settings);
      if (folder && folderChanged) {
        const result = await api.scanLibrary(folder);
        setSavedFolder(folder);
        notify(
          `設定を保存し、ライブラリのフォルダを調べました。${result.indexed} 件を調べ、${result.linked} 件を商品と結び付けました${result.truncated ? "。上限に達したため途中です" : ""}`,
        );
      } else {
        setSavedFolder(folder);
        notify("設定を保存しました");
      }
      setStatus("");
      refresh();
      try {
        const result = await api.testOllama();
        setModels(result.models);
        setInstalledSize(result.modelSizeBytes);
      } catch {
        setModels([]);
        setInstalledSize(null);
      }
    } catch (err) {
      const message = errorText(err);
      setStatus(message);
      fail(message);
    }
  }

  async function testOllama() {
    try {
      const result = await api.testOllama();
      setModels(result.models);
      setInstalledSize(result.modelSizeBytes);
      const size = result.modelSizeBytes != null && result.modelSizeBytes > 0 ? formatBytes(result.modelSizeBytes) : "";
      setStatus(
        result.modelReady
          ? `接続できました。設定中のモデルがあります（${result.models.length} 件）${size ? `。サイズは ${size}` : ""}`
          : `接続できましたが、設定中のモデルが見つかりません。導入済み: ${result.models.join(", ") || "なし"}`,
      );
    } catch (err) {
      setInstalledSize(null);
      setStatus(errorText(err));
    }
  }

  async function pullOllamaModel() {
    if (!settings) return;
    setPulling(true);
    setStatus(`${settings.ollamaModel} を取得しています`);
    try {
      await api.saveSettings(settings);
      await api.pullOllamaModel();
      await testOllama();
      notify(`${settings.ollamaModel} を取得しました`);
    } catch (err) {
      const message = errorText(err);
      setStatus(message);
      fail(message);
    } finally {
      setPulling(false);
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

  async function refreshCatalog() {
    setStatus("プロファイル一覧を取得しています");
    try {
      const report = await api.refreshAvatarCatalog();
      notify(`対応表を更新しました。新しく ${report.added} 件、合計 ${report.total} 件です`);
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setStatus("");
    }
  }

  async function reopenSetup() {
    try {
      await api.reopenSetup();
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

  const matched = (knowledge?.avatars ?? []).filter((record) => {
    const needle = filter.trim().toLowerCase();
    if (!needle) return true;
    return [record.displayName, ...record.aliases].join(" ").toLowerCase().includes(needle);
  });
  const avatars = matched.slice(0, 80);

  return (
    <section className="settings-page">
      <div className="page-head">
        <div><h1>設定</h1><p>分類、ファイルの保存先、アプリの動作を管理します。</p></div>
        {settings && <button className="button primary" type="button" onClick={() => void saveSettings()}>設定を保存</button>}
      </div>

      {settings && (
        <div className="settings-sections">
          <section className="settings-section">
            <header><h2>分類モデル</h2><p>商品説明を分類する、端末内のモデルを選びます。</p></header>
            <SettingRow label="分類方法" help={apple?.message || "Ollama はこの PC のローカル接続だけを使用します。"}>
              <select value={settings.llmBackend} onChange={(e) => setSettings({ ...settings, llmBackend: e.target.value === "apple" ? "apple" : "ollama" })}>
                <option value="ollama">Ollama（既定）</option>
                {(apple?.availability === "available" || settings.llmBackend === "apple") && <option value="apple" disabled={apple?.availability !== "available"}>Apple のオンデバイスモデル</option>}
              </select>
            </SettingRow>
            <SettingRow label="Ollama の URL" help="http://127.0.0.1、localhost、または ::1 だけを使用できます。">
              <input value={settings.ollamaBaseUrl} onChange={(e) => setSettings({ ...settings, ollamaBaseUrl: e.target.value })} />
            </SettingRow>
            <SettingRow label="モデル" help={`既定は ${DEFAULT_OLLAMA_MODEL}。軽い代替は qwen3:4b です。`}>
              <div className="inline-control">
                <input list="ollama-models" value={settings.ollamaModel} onChange={(e) => setSettings({ ...settings, ollamaModel: e.target.value })} />
                <datalist id="ollama-models">{models.map((model) => <option key={model} value={model} />)}</datalist>
                <button className="button" type="button" disabled={pulling || settings.llmBackend === "apple"} onClick={() => void pullOllamaModel()}>{pulling ? "取得中…" : "モデルを取得"}</button>
              </div>
            </SettingRow>
            <SettingRow label="確信度のしきい値" help="0〜1 で指定します。要確認は名前と対応表の一致も使って判断します。">
              <input className="short-input" type="number" min={0} max={1} step={0.05} value={settings.confidenceThreshold} onChange={(e) => setSettings({ ...settings, confidenceThreshold: Number(e.target.value) })} />
            </SettingRow>
            <div className="section-actions"><button className="button" type="button" onClick={() => void testOllama()}>接続を確認</button><span className="meta">{installedSize ? `モデルサイズ ${formatBytes(installedSize)}` : models.length ? `導入済み: ${models.join("、")}` : "導入済みモデルは未確認です"}</span></div>
          </section>

          <section className="settings-section">
            <header><h2>ライブラリとダウンロード</h2><p>ファイルの保存先と自動整理を設定します。</p></header>
            <SettingRow label="ライブラリのフォルダ" help="選ぶと保存してスキャンします。ホームやボリュームの直下は選べません。">
              <div className="inline-control"><input value={settings.libraryFolder} placeholder="ダウンロードしたファイルの移動先" onChange={(e) => setSettings({ ...settings, libraryFolder: e.target.value })} /><button className="button" type="button" onClick={() => void (async () => { try { const path = await chooseFolder(); if (!path) return; const next = { ...settings, libraryFolder: path }; setSettings(next); const result = await saveLibraryAndScan(path, next); setSavedFolder(path); setStatus(""); notify(`ライブラリのフォルダを保存し、${result.indexed} 件を調べ、${result.linked} 件を商品と結び付けました${result.truncated ? "。上限に達したため途中です" : ""}`); refresh(); } catch (err) { const message = errorText(err); setStatus(message); fail(message); } })()}>選ぶ</button></div>
            </SettingRow>
            <SettingRow label="リモートライブラリ" help="ネットワークドライブやマウント済みのフォルダを指定します。">
              <div className="inline-control"><input value={settings.remoteLibraryFolder} placeholder="リモートのフォルダ" onChange={(e) => setSettings({ ...settings, remoteLibraryFolder: e.target.value })} /><button className="button" type="button" onClick={() => void chooseFolder().then((path) => { if (path) setSettings({ ...settings, remoteLibraryFolder: path }); }, (err: unknown) => fail(errorText(err)))}>選ぶ</button></div>
            </SettingRow>
            <SettingRow label="ダウンロードを自動整理" help="自分でダウンロードしたファイルだけを商品別にライブラリへ移動します。">
              <label className="switch-label"><input type="checkbox" checked={settings.routeDownloads} onChange={(e) => { const enabled = e.target.checked; if (enabled && !settings.libraryFolder.trim()) { void (async () => { try { const path = await chooseFolder(); if (!path) return; const next = { ...settings, libraryFolder: path, routeDownloads: true }; setSettings(next); const result = await saveLibraryAndScan(path, next); setSavedFolder(path); notify(`ライブラリのフォルダを保存し、${result.indexed} 件を調べ、${result.linked} 件を商品と結び付けました`); refresh(); } catch (err) { fail(errorText(err)); } })(); return; } setSettings({ ...settings, routeDownloads: enabled }); }} />有効にする</label>
            </SettingRow>
            <details className="settings-details"><summary>自動整理について詳しく</summary><p>拡張機能または内蔵ブラウザで選んだ1件だけを Downloads/VRCLibMan/商品ID に保存し、アプリがライブラリへ移動します。リモートとのコピーも操作したファイルだけです。</p></details>
          </section>

          <section className="settings-section">
            <header><h2>ブラウザ連携</h2><p>BOOTH を開く方法を選びます。</p></header>
            <SettingRow label="内蔵ブラウザ" help="既定は拡張機能です。有効化すると確認画面が表示されます。">
              <div className="inline-control"><button className={`button ${settings.builtinBrowser ? "" : "primary"}`} type="button" onClick={() => void setBuiltinBrowser(!settings.builtinBrowser)}>{settings.builtinBrowser ? "無効にする" : "有効にする"}</button>{settings.builtinBrowser && <button className="button danger" type="button" onClick={() => void (async () => { if (!window.confirm("ブラウザを閉じ、このアプリの中の BOOTH のログイン状態を消します。")) return; try { await api.clearBoothSession(); notify("セッションを消しました"); } catch (err) { fail(errorText(err)); } })()}>ログアウトしてデータを消す</button>}</div>
            </SettingRow>
            <details className="settings-details"><summary>内蔵ブラウザについて詳しく</summary><p>ログイン状態はアプリ専用フォルダに保存されます。ページ移動は「ライブラリ全取得」を押したときだけで、一括ダウンロードや年齢確認の自動通過は行いません。</p></details>
          </section>

          <section className="settings-section">
            <header><h2>起動と常駐</h2><p>ウィンドウを閉じた後の動作を設定します。</p></header>
            <SettingRow label="バックグラウンドで動作" help="ウィンドウを閉じても拡張機能からのダウンロードを受け付けます。">
              <label className="switch-label"><input type="checkbox" checked={settings.backgroundMode} onChange={(e) => { const enabled = e.target.checked; setSettings({ ...settings, backgroundMode: enabled, launchAtLogin: enabled ? settings.launchAtLogin : false }); }} />有効にする</label>
            </SettingRow>
            <SettingRow label="ログイン時に起動" help="バックグラウンド動作が有効な場合だけ使用できます。">
              <label className="switch-label"><input type="checkbox" checked={settings.launchAtLogin} disabled={!settings.backgroundMode} onChange={(e) => setSettings({ ...settings, launchAtLogin: e.target.checked })} />有効にする</label>
            </SettingRow>
          </section>

          {link && <section className="settings-section">
            <header><h2>拡張機能の接続</h2><p>拡張機能からこの PC へ安全にデータを送ります。</p></header>
            <SettingRow label="ペアリングコード" help={link.listening ? `127.0.0.1:${link.port} で待機中です。` : link.error || "受信を開始できませんでした。"}>
              <div className="inline-control"><input readOnly value={link.token} /><button className="button" type="button" onClick={() => void regenerateToken()}>作り直す</button></div>
            </SettingRow>
            <details className="settings-details"><summary>接続について詳しく</summary><p>コードを拡張機能へ一度貼り付けます。作り直すと以前のコードと接続は無効になり、アプリが閉じている場合は専用リンクから起動します。</p></details>
          </section>}

          <section className="settings-section">
            <header><h2>データ</h2><p>アバター対応表と初期データを管理します。</p></header>
            {catalog && <SettingRow label="同梱の対応表" help={`${catalog.credit}（${catalog.author}）・${catalog.count} 件・取得日 ${catalog.fetchedOn || "未記録"}${catalog.upstreamUpdated ? `・上流 ${catalog.upstreamUpdated}` : ""}`}>
              <div className="inline-control"><button className="button primary" type="button" onClick={() => void refreshCatalog()}>対応表を更新</button><button className="button" type="button" onClick={() => void openExternal(catalog.sourceUrl)}>出典</button><button className="button" type="button" onClick={() => void openExternal(catalog.termsUrl)}>利用規約</button></div>
            </SettingRow>}
            <SettingRow label="メンテナンス" help="不足している初期データの追加や、商品の紐づけの再計算を行います。">
              <div className="inline-control"><button className="button" type="button" onClick={() => void restore()}>初期データを追加</button><button className="button" type="button" onClick={() => void reresolve()}>対応表を再適用</button><button className="button" type="button" onClick={() => void reopenSetup()}>セットアップを再表示</button></div>
            </SettingRow>
          </section>
        </div>
      )}
      {status && <p className="settings-status">{status}</p>}

      <section className="settings-section knowledge-section">
        <header><h2>対応表を編集</h2><p>アバターの別名と共通素体を編集します。</p></header>
        <div className="knowledge">
          <div><input className="search" value={filter} placeholder="対応表を検索" onChange={(e) => setFilter(e.target.value)} /><p className="meta">{knowledge?.avatars.length ?? 0} 件中 {avatars.length} 件を表示</p><div className="list">{avatars.map((record) => <button key={record.id} className={record.id === avatar.id ? "selected" : ""} type="button" onClick={() => selectAvatar(record)}>{record.displayName}{record.isBuiltin ? " ・初期" : ""}</button>)}</div><h2>素体</h2><div className="list">{(knowledge?.baseBodies ?? []).map((record) => <button key={record.id} type="button" onClick={() => setBody(record)}>{record.displayName}</button>)}</div></div>
          <div className="stack">
            <div className="panel"><h2>{avatar.id ? "アバターを編集" : "アバターを追加"}</h2><label className="field"><span>表示名</span><input value={avatar.displayName} onChange={(e) => setAvatar({ ...avatar, displayName: e.target.value })} /></label><label className="field"><span>別名（カンマ区切り）</span><input value={avatar.aliases} onChange={(e) => setAvatar({ ...avatar, aliases: e.target.value })} /></label><label className="field"><span>素体</span><select value={avatar.baseBody} onChange={(e) => setAvatar({ ...avatar, baseBody: e.target.value })}><option value="">なし</option>{(knowledge?.baseBodies ?? []).map((record: BaseBody) => <option key={record.id} value={record.id}>{record.displayName}</option>)}</select></label><label className="field"><span>メモ</span><textarea value={avatar.notes} onChange={(e) => setAvatar({ ...avatar, notes: e.target.value })} /></label><div className="row-actions"><button className="button primary" type="button" onClick={() => void saveAvatar()}>保存</button><button className="button" type="button" onClick={() => setAvatar(emptyAvatar)}>新規</button><button className="button danger" type="button" disabled={!avatar.id || avatar.builtin} onClick={() => void removeAvatar()}>削除</button></div></div>
            <div className="panel"><h2>{body.id ? "素体を編集" : "素体を追加"}</h2><label className="field"><span>表示名</span><input value={body.displayName} onChange={(e) => setBody({ ...body, displayName: e.target.value })} /></label><label className="field"><span>根拠のメモ</span><textarea value={body.note} onChange={(e) => setBody({ ...body, note: e.target.value })} /></label><div className="row-actions"><button className="button primary" type="button" onClick={() => void saveBody()}>保存</button><button className="button" type="button" onClick={() => setBody({ id: "", displayName: "", note: "" })}>新規</button><button className="button danger" type="button" disabled={!body.id} onClick={() => void removeBody()}>削除</button></div></div>
          </div>
        </div>
      </section>
    </section>
  );
}
