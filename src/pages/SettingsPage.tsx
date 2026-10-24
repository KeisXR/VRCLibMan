import { useEffect, useRef, useState } from "react";
import { api, appVersion, chooseFolder, openExternal } from "../api";
import { useShell } from "../App";
import { formatBytes } from "../media";
import { DEFAULT_OLLAMA_MODEL } from "../types";
import type { AppleFmStatus, AvatarRecord, BaseBody, CatalogInfo, IntegrationStatus, Knowledge } from "../types";
import { errorText } from "../ui";
import { SettingRow } from "../components/SettingRow";
import { ModelPicker } from "../components/ModelPicker";
import { BoothImportSettings } from "../components/BoothImportSettings";
import { MODEL_CHOICES, modelDownloadPrompt } from "../models";
import { useSettingsAutosave } from "../useSettingsAutosave";
import { extensionVersionMessage } from "../versions";

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
  const lastScannedFolder = useRef<string | null>(null);
  const { settings, phase: savePhase, error: saveError, change: changeSettings, hydrate, ticket, flush } = useSettingsAutosave((saved, patch) => {
    refresh();
    if (patch.libraryFolder !== undefined && saved.libraryFolder !== lastScannedFolder.current) {
      lastScannedFolder.current = saved.libraryFolder;
      if (saved.libraryFolder) void api.scanLibrary(saved.libraryFolder).then((report) => {
        notify(`ライブラリのフォルダを調べ、${report.indexed} 件中 ${report.linked} 件を結び付けました${report.truncated ? "（上限で中断）" : ""}`);
        refresh();
      }, (err: unknown) => fail(`設定は保存しましたが、フォルダを調べられませんでした。${errorText(err)}`));
    }
  });
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [avatar, setAvatar] = useState(emptyAvatar);
  const [body, setBody] = useState({ id: "", displayName: "", note: "" });
  const [filter, setFilter] = useState("");
  const [status, setStatus] = useState("");
  const [ollamaStatus, setOllamaStatus] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [installedSize, setInstalledSize] = useState<number | null>(null);
  const [integration, setIntegration] = useState<IntegrationStatus | null>(null);
  const [version, setVersion] = useState("");
  const [modelsChecked, setModelsChecked] = useState(false);
  const [apple, setApple] = useState<AppleFmStatus | null>(null);
  const [catalog, setCatalog] = useState<CatalogInfo | null>(null);
  const [pulling, setPulling] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (settings?.llmBackend !== "ollama") return;
    api.testOllama().then(
      (result) => {
        if (cancelled) return;
        setModels(result.models);
        setModelsChecked(true);
        setInstalledSize(result.modelSizeBytes);
      },
      () => {
        if (cancelled) return;
        setModels([]);
        setModelsChecked(false);
        setInstalledSize(null);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision, settings?.llmBackend]);

  useEffect(() => {
    let cancelled = false;
    const settingsTicket = ticket();
    Promise.all([api.settings(), api.knowledge(), api.integrationStatus(), api.catalogInfo(), api.appleFmStatus(), appVersion()]).then(
      ([nextSettings, nextKnowledge, nextIntegration, nextCatalog, nextApple, nextVersion]) => {
        if (cancelled) return;
        hydrate(nextSettings, settingsTicket);
        if (lastScannedFolder.current == null) lastScannedFolder.current = nextSettings.libraryFolder;
        setApple(nextApple);
        setCatalog(nextCatalog);
        setKnowledge(nextKnowledge);
        setIntegration(nextIntegration);
        setVersion(nextVersion);
      },
      (err: unknown) => {
        if (!cancelled) fail(errorText(err));
      },
    );
    return () => {
      cancelled = true;
    };
  }, [revision, fail, hydrate, ticket]);

  function flushSettings() { void flush().catch(() => {}); }

  async function chooseLibrary() {
    try {
      const folder = await chooseFolder();
      if (folder) changeSettings({ libraryFolder: folder });
    } catch (err) { fail(errorText(err)); }
  }

  async function changeRouting(enabled: boolean) {
    if (!settings) return;
    if (enabled && !settings.libraryFolder.trim()) {
      try {
        const folder = await chooseFolder();
        if (folder) changeSettings({ libraryFolder: folder, routeDownloads: true });
      } catch (err) { fail(errorText(err)); }
    } else changeSettings({ routeDownloads: enabled });
  }

  async function testOllama() {
    try {
      await flush();
      const result = await api.testOllama();
      setModels(result.models);
      setModelsChecked(true);
      setInstalledSize(result.modelSizeBytes);
      const size = result.modelSizeBytes != null && result.modelSizeBytes > 0 ? formatBytes(result.modelSizeBytes) : "";
      setOllamaStatus(
        result.modelReady
          ? `接続できました。設定中のモデルがあります（${result.models.length} 件）${size ? `。サイズは ${size}` : ""}`
          : `接続できましたが、設定中のモデルが見つかりません。導入済み: ${result.models.join(", ") || "なし"}`,
      );
    } catch (err) {
      setInstalledSize(null);
      setModelsChecked(false);
      setOllamaStatus(errorText(err));
    }
  }

  async function pullOllamaModel() {
    try {
      const saved = await flush();
      if (!saved || !window.confirm(modelDownloadPrompt(saved.ollamaModel))) return;
      setPulling(true);
      setOllamaStatus(`${saved.ollamaModel} 取得中`);
      await api.pullOllamaModel(saved.ollamaModel);
      await testOllama();
      notify(`${saved.ollamaModel} を取得しました`);
    } catch (err) {
      const message = errorText(err);
      setOllamaStatus(message);
      fail(message);
    } finally { setPulling(false); }
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
    setStatus("プロファイル一覧を取得中");
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
        <div><h1>設定</h1><p>分類、ファイルの保存先、アプリの動作を管理します。変更は自動で保存・適用します。</p></div>
        {settings && <p className={savePhase === "error" ? "settings-save-state warn-text" : "settings-save-state meta"} role="status" aria-live="polite">{savePhase === "saving" ? "保存中…" : savePhase === "error" ? "未保存の変更があります" : "変更は自動保存されます"}</p>}
      </div>

      {settings && (
        <div className="settings-sections">
          <section className="settings-section">
            <header><h2>分類モデル</h2><p>商品説明を分類する、端末内のモデルを選びます。</p></header>
            <SettingRow label="分類方法" help={apple?.message || "Ollama はこの PC のローカル接続だけを使用します。"}>
              <select value={settings.llmBackend} onChange={(e) => changeSettings({ llmBackend: e.target.value === "apple" ? "apple" : "ollama" })}>
                <option value="ollama">Ollama（既定）</option>
                {(apple?.availability === "available" || settings.llmBackend === "apple") && <option value="apple" disabled={apple?.availability !== "available"}>Apple のオンデバイスモデル</option>}
              </select>
            </SettingRow>
            {settings.llmBackend === "ollama" && (
              <>
                <SettingRow label="Ollama の URL" help="http://127.0.0.1、localhost、または ::1 だけを使用できます。">
                  <input value={settings.ollamaBaseUrl} onChange={(e) => changeSettings({ ollamaBaseUrl: e.target.value }, 300)} onBlur={flushSettings} />
                </SettingRow>
                <SettingRow label="モデル" help={`推奨・既定は ${DEFAULT_OLLAMA_MODEL}。軽量な代替は qwen3.5:4b です。`}>
                  <ModelPicker onBlur={flushSettings} value={settings.ollamaModel} models={models} checked={modelsChecked} disabled={pulling} onChange={(value) => changeSettings({ ollamaModel: value }, MODEL_CHOICES.some((choice) => choice.name === value) ? 0 : 300)} />
                  <button className="button" type="button" disabled={pulling || !settings.ollamaModel.trim()} onClick={() => void pullOllamaModel()}>{pulling ? "取得中…" : "モデルを取得"}</button>
                </SettingRow>
              </>
            )}
            <SettingRow label="確信度のしきい値" help="0〜1 で指定します。要確認は名前と対応表の一致も使って判断します。">
              <input className="short-input" type="number" min={0} max={1} step={0.05} value={settings.confidenceThreshold} onChange={(e) => changeSettings({ confidenceThreshold: Number(e.target.value) }, 300)} onBlur={flushSettings} />
            </SettingRow>
            {settings.llmBackend === "ollama" && <div className="section-actions"><button className="button" type="button" onClick={() => void testOllama()}>接続を確認</button><span className="meta">{installedSize ? `モデルサイズ ${formatBytes(installedSize)}` : models.length ? `導入済み: ${models.join("、")}` : "導入済みモデル：未確認"}</span></div>}
            <details className="settings-details">
              <summary>分類モデルについて詳しく</summary>
              {settings.llmBackend === "apple" ? <p>Apple のオンデバイスモデルで分類します。macOS 27.2 以上で利用可能な場合に使用できます。</p> : <>
                <p>今の設定は {settings.ollamaModel}、分類は Ollama です。温度は 0.1 固定です。</p>
                <p>「モデルを取得」は、押したときだけ Ollama からモデルを1回ダウンロードします。数 GB になることがあります。「接続を確認」で導入済みのモデルを候補に出します。</p>
              </>}
              <p>要確認は、モデルが自分で付けた数値ではなく、名前が商品文と対応表に結び付いたかで決めます。</p>
            </details>
          </section>

          <section className="settings-section">
            <header><h2>ライブラリとダウンロード</h2><p>ファイルの保存先と自動整理を設定します。</p></header>
            <SettingRow label="ライブラリのフォルダ" help="インポート画面と同じフォルダです。選ぶと保存してスキャンします。ホームやボリュームの直下は選べません。">
              <div className="inline-control"><input value={settings.libraryFolder} placeholder="ダウンロードしたファイルの移動先" onChange={(e) => changeSettings({ libraryFolder: e.target.value, ...(!e.target.value.trim() ? { routeDownloads: false } : {}) }, 300)} onBlur={flushSettings} /><button className="button" type="button" onClick={() => void chooseLibrary()}>選ぶ</button></div>
            </SettingRow>
            <SettingRow label="リモートライブラリ" help="ネットワークドライブやマウント済みのフォルダを指定します。">
              <div className="inline-control"><input value={settings.remoteLibraryFolder} placeholder="リモートのフォルダ" onChange={(e) => changeSettings({ remoteLibraryFolder: e.target.value }, 300)} onBlur={flushSettings} /><button className="button" type="button" onClick={() => void chooseFolder().then((path) => { if (path) changeSettings({ remoteLibraryFolder: path }); }, (err: unknown) => fail(errorText(err)))}>選ぶ</button></div>
            </SettingRow>
            <SettingRow label="ダウンロードを自動整理" help="自分でダウンロードしたファイルだけを商品別にライブラリへ移動します。">
              <label className="switch-label"><input type="checkbox" checked={settings.routeDownloads} onChange={(e) => void changeRouting(e.target.checked)} />有効にする</label>
            </SettingRow>
            <details className="settings-details"><summary>自動整理について詳しく</summary><p>自動整理は既定でオフです。オンにするときは、移動先のライブラリフォルダが必要です。オンにすると、拡張は自分で押したダウンロードだけを、この PC のダウンロードフォルダの VRCLibMan/商品ID に置き、アプリがライブラリへ移します。内蔵ブラウザを有効にしているときは、未ダウンロードの行を押したその1件も、同じ場所へダウンロードします。</p><p>それ以外の場所にあるファイルは移動しません。アプリから一括ではダウンロードしません。リモートの確認は画面を出してから行い、送る・取ってくるは押したファイルだけをコピーします。コピー中は画面の上に表示します。切れている共有先では待たずに未ダウンロードとして表示します。</p></details>
          </section>

          <section className="settings-section">
            <header><h2>ブラウザ連携</h2><p>BOOTH を開く方法を選びます。</p></header>
          </section>

          <section className="settings-section">
            <header><h2>起動と常駐</h2><p>ウィンドウを閉じた後の動作を設定します。</p></header>
            <SettingRow label="バックグラウンドで動作" help="ウィンドウを閉じても拡張機能からのダウンロードを受け付けます。">
              <label className="switch-label"><input type="checkbox" checked={settings.backgroundMode} onChange={(e) => { const enabled = e.target.checked; changeSettings({ backgroundMode: enabled, launchAtLogin: enabled ? settings.launchAtLogin : false }); }} />有効にする</label>
            </SettingRow>
            <SettingRow label="ログイン時に起動" help="バックグラウンド動作が有効な場合だけ使用できます。">
              <label className="switch-label"><input type="checkbox" checked={settings.launchAtLogin} disabled={!settings.backgroundMode} onChange={(e) => changeSettings({ launchAtLogin: e.target.checked })} />有効にする</label>
            </SettingRow>
            <details className="settings-details"><summary>起動と常駐について詳しく</summary><p>既定はオフです。オフのときはウィンドウを閉じるとアプリも終了し、トレイは残りません。オンのときはウィンドウを破棄してメモリを放し、プロセスと 17321 の受け付けだけ残します。トレイ（Windows）またはメニューバー（macOS）から開き直すか、終了できます。</p><p>ログイン時の起動は、バックグラウンドをオンにしたときだけ使えます。バックグラウンドをオフにすると、ログイン項目もオフになります。両方オンのとき、ログインから起動したプロセスはウィンドウを出さず、トレイまたはメニューバーから開けます。</p></details>
          </section>

          <section className="settings-section">
            <header><h2>BOOTH の読み込み方</h2><p>拡張機能・内蔵ブラウザ・併用を選び、その場で保存します。</p></header>
            <div className="section-body"><BoothImportSettings onFail={fail} onChange={() => refresh()} /></div>
            {settings.builtinBrowser && <div className="section-actions"><button className="button danger" type="button" onClick={() => void (async () => { if (!window.confirm("ブラウザを閉じ、このアプリの中の BOOTH のログイン状態を消します。")) return; try { await api.clearBoothSession(); notify("セッションを消しました"); } catch (err) { fail(errorText(err)); } })()}>ログアウトしてセッションを消す</button></div>}
          </section>

          <section className="settings-section">
            <header><h2>データ</h2><p>バージョン、アバター対応表と初期データを確認します。</p></header>
            <SettingRow label="バージョン" help="アプリと拡張機能は別のバージョン番号です。外部への更新確認はしません。">
              <div className="version-info"><p>アプリ v{version || "確認中"}</p>
                {integration && <><p>想定する拡張 v{integration.expectedExtensionVersion}</p><p>最後に通信した拡張 {integration.extensionVersion ? `v${integration.extensionVersion}` : "未確認"}</p><p className={integration.extensionVersion && integration.extensionVersion !== integration.expectedExtensionVersion ? "warn-text" : "meta"}>{extensionVersionMessage(integration.expectedExtensionVersion, integration.extensionVersion)}</p></>}
              </div>
            </SettingRow>
            {catalog && <SettingRow label="同梱の対応表" help={`${catalog.credit}（${catalog.author}）・${catalog.count} 件・取得日 ${catalog.fetchedOn || "未記録"}${catalog.upstreamUpdated ? `・上流 ${catalog.upstreamUpdated}` : ""}`}>
              <div className="inline-control"><button className="button primary" type="button" onClick={() => void refreshCatalog()}>対応表を更新</button><button className="button" type="button" onClick={() => void openExternal(catalog.sourceUrl)}>出典</button><button className="button" type="button" onClick={() => void openExternal(catalog.termsUrl)}>利用規約</button></div>
            </SettingRow>}
            {catalog && <details className="settings-details"><summary>対応表について詳しく</summary><p>定期的には取りに行きません。「対応表を更新」を押したときだけ GitHub から1回取得します。規約ではコピー、保存、二次利用、商用利用が許されています。正確性の保証はなく、アバターの著作権は各作者にあります。共通素体シート（スペキュラム合金さん）はライセンスが不明なので入れていません。</p></details>}
            <SettingRow label="メンテナンス" help="不足している初期データの追加や、商品の紐づけの再計算を行います。">
              <div className="inline-control"><button className="button" type="button" onClick={() => void restore()}>初期データを追加</button><button className="button" type="button" onClick={() => void reresolve()}>対応表を再適用</button><button className="button" type="button" onClick={() => void reopenSetup()}>セットアップを再表示</button></div>
            </SettingRow>
            <details className="settings-details privacy-notice">
              <summary>プライバシー</summary>
              <p>本アプリには、利用者の情報を開発者へ送信する機能はありません。</p>
              <p>アカウント情報・購入情報・設定は端末内で扱います。分類には同じ端末の Ollama または Apple のオンデバイスモデルを使用します。</p>
              <p>BOOTH／pixiv でのログインや閲覧には、そのサービスとの通信を行います。内蔵ブラウザのログイン状態は端末内に保存し、選択したファイルのダウンロード時には BOOTH の Cookie を利用します。</p>
              <p>ログイン状態は「ログアウトしてセッションを消す」で削除できます。ライブラリ情報や設定は、この操作では削除されません。</p>
            </details>
          </section>
        </div>
      )}
      {saveError && <p className="settings-status warn-text" role="alert">設定を保存できませんでした。{saveError}</p>}
      {status && <p className="settings-status">{status}</p>}
      {settings?.llmBackend === "ollama" && ollamaStatus && <p className="settings-status">{ollamaStatus}</p>}

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
