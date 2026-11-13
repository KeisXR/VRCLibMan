import { useEffect, useRef, useState } from "react";
import { api, appVersion, chooseFolder, openExternal } from "../api";
import { useShell } from "../App";
import type { AvatarRecord, BaseBody, CatalogInfo, IntegrationStatus, Knowledge, RemoteSyncStatus } from "../types";
import { errorText } from "../ui";
import { SettingRow } from "../components/SettingRow";
import { BoothImportSettings } from "../components/BoothImportSettings";
import { BuiltinBrowserAccessSettings } from "../components/BuiltinBrowserControls";
import { useSettingsAutosave } from "../useSettingsAutosave";
import { extensionVersionMessage } from "../versions";
import { AppUpdater } from "../components/AppUpdater";
import { ClassificationFeedback } from "../components/ClassificationFeedback";
import { AssetUpdates } from "../components/AssetUpdates";
import { UnitySettings } from "../components/UnitySettings";
import { SettingsNav } from "../components/SettingsNav";
import { SyncExportSettings } from "../components/SyncExportSettings";
import { RemoteSyncReport } from "../components/RemoteSyncReport";
import { AVATAR_HERO_CHOICES, AVATAR_HERO_BASE, normalizeAvatarHero } from "../avatarHero";
import { readWithTimeout } from "../ipcRecovery";

const REPOSITORY_URL = "https://github.com/KeisXR/VRCLibMan";

const emptyAvatar = {
  id: "",
  displayName: "",
  baseBody: "",
  aliases: "",
  notes: "",
  builtin: false,
};

export function SettingsPage() {
  const { revision, refresh, notify, fail, workInProgress } = useShell();
  const lastScannedFolder = useRef<string | null>(null);
  const { settings, phase: savePhase, error: saveError, change: changeSettings, hydrate, ticket, flush } = useSettingsAutosave((saved, patch) => {
    refresh();
    if (patch.libraryFolder !== undefined && saved.libraryFolder !== lastScannedFolder.current) {
      lastScannedFolder.current = saved.libraryFolder;
      if (saved.libraryFolder) void api.scanLibrary(saved.libraryFolder).then((report) => {
        notify(`ライブラリのフォルダを調べ、${report.indexed} 件中 ${report.linked} 件を結び付けました${report.truncated ? "（上限で中断）" : ""}`, undefined, { persist: report.truncated });
        refresh();
      }, (err: unknown) => fail(`設定は保存しましたが、フォルダを調べられませんでした。${errorText(err)}`));
    }
  });
  const [knowledge, setKnowledge] = useState<Knowledge | null>(null);
  const [avatar, setAvatar] = useState(emptyAvatar);
  const [body, setBody] = useState({ id: "", displayName: "", note: "" });
  const [filter, setFilter] = useState("");
  const [status, setStatus] = useState("");
  const [integration, setIntegration] = useState<IntegrationStatus | null>(null);
  const [version, setVersion] = useState("");
  const [catalog, setCatalog] = useState<CatalogInfo | null>(null);
  const [remoteSync, setRemoteSync] = useState<RemoteSyncStatus | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [loadingSettings, setLoadingSettings] = useState(true);
  const [loadErrors, setLoadErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    if (!settings?.remoteAutoSync) return;
    let cancelled = false;
    const update = () => void api.remoteSyncStatus().then(
      (next) => { if (!cancelled) setRemoteSync(next); },
      (err: unknown) => {
        if (!cancelled) {
          setRemoteSync({
            running: false, copied: 0, skipped: 0, missing: 0, missingFiles: [], conflicts: 0, conflictFiles: [],
            sourceMissing: 0, sourceMissingFiles: [], failed: 0, failures: [], lastCompletedAt: null, error: errorText(err),
          });
        }
      },
    );
    setRemoteSync(null);
    update();
    const timer = window.setInterval(update, 3000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [settings?.remoteAutoSync, settings?.remoteLibraryFolder]);

  useEffect(() => {
    let cancelled = false;
    const settingsTicket = ticket();
    setLoadingSettings(true);
    setLoadErrors({});
    function load<T>(label: string, read: () => Promise<T>, apply: (value: T) => void) {
      void readWithTimeout(read, label).then(
        (value) => { if (!cancelled) apply(value); },
        (err: unknown) => { if (!cancelled) setLoadErrors((errors) => ({ ...errors, [label]: errorText(err) })); },
      ).finally(() => { if (!cancelled && label === "一般設定") setLoadingSettings(false); });
    }
    load("一般設定", api.settings, (nextSettings) => {
      hydrate(nextSettings, settingsTicket);
      if (lastScannedFolder.current == null) lastScannedFolder.current = nextSettings.libraryFolder;
    });
    load("対応表", api.knowledge, setKnowledge);
    load("連携状態", api.integrationStatus, setIntegration);
    load("カタログ情報", api.catalogInfo, setCatalog);
    load("アプリの版", appVersion, setVersion);
    return () => {
      cancelled = true;
    };
  }, [revision, loadAttempt, hydrate, ticket]);

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
    if (!window.confirm(`「${avatar.displayName}」を対応表から削除します。このアバターに結び付いた商品の対応は計算し直します。元に戻せません。続けますか？`)) return;
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
    if (!window.confirm(`素体「${body.displayName}」を削除します。この素体に入っているアバターは素体なしになり、共通素体を通した衣装の表示が無くなります。初期データの素体は「デバッグ → 初期データを追加」で戻せます。続けますか？`)) return;
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
        <div><h1>設定</h1><p>一般設定は自動保存されます。対応表の編集は保存ボタンで確定してください。</p></div>
        {settings && <p className={savePhase === "error" ? "settings-save-state warn-text" : "settings-save-state meta"} role="status" aria-live="polite">{savePhase === "saving" ? "保存中…" : savePhase === "error" ? "未保存の設定があります" : "一般設定は保存済み"}</p>}
      </div>

      {loadingSettings && !settings && <p className="meta" role="status">設定を読み込んでいます…</p>}
      {Object.keys(loadErrors).length > 0 && <div className="panel" role="alert">
        <p>一部の情報を読み込めませんでした。読み込み済みの設定は引き続き使えます。</p>
        <ul>{Object.entries(loadErrors).map(([label, message]) => <li key={label}>{label}：{message}</li>)}</ul>
        <button className="button" type="button" disabled={loadingSettings} onClick={() => setLoadAttempt((value) => value + 1)}>もう一度読み込む</button>
      </div>}

      {settings && (
        <div className="settings-layout">
          <SettingsNav />
          <div className="settings-sections">
          <section className="settings-section" id="settings-library">
            <header><h2>ライブラリとダウンロード</h2><p>ファイルの保存先と自動整理を設定します。</p></header>
            <SettingRow id="settings-library-folder" label="ライブラリのフォルダ" help="ファイルの保存先を選ぶと、自動でスキャンします。">
              <div className="inline-control"><input value={settings.libraryFolder} placeholder="ダウンロードしたファイルの移動先" onChange={(e) => changeSettings({ libraryFolder: e.target.value, ...(!e.target.value.trim() ? { routeDownloads: false } : {}) }, 300)} onBlur={flushSettings} /><button className="button" type="button" onClick={() => void chooseLibrary()}>選ぶ</button></div>
            </SettingRow>
            <SettingRow id="settings-remote-library" label="リモートライブラリ" help="ネットワークドライブやマウント済みのフォルダを指定します。">
              <div className="inline-control"><input value={settings.remoteLibraryFolder} placeholder="リモートのフォルダ" onChange={(e) => changeSettings({ remoteLibraryFolder: e.target.value, remoteAutoSync: false }, 300)} onBlur={flushSettings} /><button className="button" type="button" onClick={() => void chooseFolder().then((path) => { if (path) changeSettings({ remoteLibraryFolder: path, remoteAutoSync: false }); }, (err: unknown) => fail(errorText(err)))}>選ぶ</button></div>
            </SettingRow>
            <SettingRow id="settings-remote-auto-sync" label="リモートへ自動同期" help="アプリ起動中、登録済みの手元のファイルを約1分ごとにリモートへコピーします。">
              <label className="switch-label"><input aria-label="リモートへ自動同期" type="checkbox" checked={settings.remoteAutoSync} disabled={!settings.remoteLibraryFolder.trim()} onChange={(e) => changeSettings({ remoteAutoSync: e.target.checked })} /></label>
            </SettingRow>
            {settings.remoteAutoSync && <RemoteSyncReport status={remoteSync} onRetry={() => void api.retryRemoteSync().catch((err: unknown) => fail(errorText(err)))} />}
            <details className="settings-details" id="settings-remote-about"><summary>リモートライブラリについて</summary><p>自動同期は既定でオフです。オンにすると、登録済みのファイルをリモートの商品IDフォルダへコピーします。保存先を変更すると自動同期はオフになります。除外した商品と未登録のファイルは対象外です。同名ファイルは上書きせず、ローカルとリモートのファイルを削除しません。同じ名前で大きさの違うファイルがリモートにあるときは、競合として一覧に出し、コピーしません。同じ名前で同じ大きさなら同じファイルとみなします。</p><p>コピーしたファイルとリモートで見つけたファイルは記録します。その後リモートで名前が変わったり消えたりしても、自動ではコピーし直しません。同期サービスが同じフォルダを作り続けないようにするためです。</p><p>接続できないときは後で再試行します。オフにしても、処理中の1件は完了する場合があります。リモートから取得する場合は「ローカルへコピー」を使ってください。</p></details>
            <SettingRow id="settings-route-downloads" label="ダウンロードを自動整理" help="自分でダウンロードしたファイルだけを商品別にライブラリへ移動します。">
              <label className="switch-label"><input aria-label="ダウンロードを自動整理" type="checkbox" checked={settings.routeDownloads} onChange={(e) => void changeRouting(e.target.checked)} /></label>
            </SettingRow>
            <details className="settings-details" id="settings-route-about"><summary>自動整理について詳しく</summary><p>有効にするには、移動先のライブラリフォルダが必要です。拡張機能や内蔵ブラウザでダウンロードしたファイルを、ダウンロードフォルダの VRCLibMan 以下から商品別に移動します。</p><p>ほかの場所にあるファイルは移動しません。内蔵ブラウザからのダウンロードも、選んだ1件ずつ行います。</p></details>
          </section>

          <section className="settings-section" id="settings-unity">
            <header><h2>Unity</h2><p>unitypackage を開くときに使う Unity を選びます。</p></header>
            <UnitySettings onNotify={notify} onFail={fail} />
          </section>

          <section className="settings-section" id="settings-startup">
            <header><h2>起動と常駐</h2><p>ウィンドウを閉じた後の動作を設定します。</p></header>
            {!settings.backgroundSupported && <div className="section-body"><p>この環境ではバックグラウンド動作とログイン時の起動は利用できません。ウィンドウを閉じるとアプリも終了します。</p></div>}
            <SettingRow id="settings-background" label="バックグラウンドで動作" help="ウィンドウを閉じても、商品情報の受信・自動整理・リモートへの自動同期を続けます。">
              <label className="switch-label"><input aria-label="バックグラウンドで動作" type="checkbox" checked={settings.backgroundSupported && settings.backgroundMode} disabled={!settings.backgroundSupported} onChange={(e) => { const enabled = e.target.checked; changeSettings({ backgroundMode: enabled, launchAtLogin: enabled ? settings.launchAtLogin : false }); }} /></label>
            </SettingRow>
            <SettingRow id="settings-launch-at-login" label="ログイン時に起動" help="バックグラウンド動作が有効な場合だけ使用できます。">
              <label className="switch-label"><input aria-label="ログイン時に起動" type="checkbox" checked={settings.backgroundSupported && settings.launchAtLogin} disabled={!settings.backgroundSupported || !settings.backgroundMode} onChange={(e) => changeSettings({ launchAtLogin: e.target.checked })} /></label>
            </SettingRow>
            {settings.backgroundSupported && <details className="settings-details"><summary>起動と常駐について詳しく</summary><p>オフにすると、ウィンドウを閉じたときにアプリも終了します。オンにすると、トレイ（Windows・Linux）またはメニューバー（macOS）から再表示・終了できます。</p><p>「ログイン時に起動」を有効にすると、ウィンドウを表示せずに起動します。バックグラウンド動作をオフにすると、ログイン時の起動もオフになります。</p><p>Linux はトレイのメニューから「開く」「終了」を選びます。GNOME では AppIndicator 対応の拡張が必要になる場合があります。トレイを利用できないときはウィンドウを表示し、閉じると終了します。</p></details>}
          </section>


          <section className="settings-section" id="settings-design">
            <header><h2>デザイン</h2><p>アバター詳細のヒーロー画像の高さを選びます。標準デザイン自体は1つです。</p></header>
            <SettingRow id="settings-avatar-hero" label="アバター詳細のヒーロー" help="大・小は画像を中央で切り抜いて表示します。旧デザインは従来の文字だけの見出しです。">
              <fieldset className="scope-choices" aria-label="アバター詳細のヒーロー">
                {AVATAR_HERO_CHOICES.map((choice) => (
                  <label key={choice.value} className="choice-row">
                    <input
                      type="radio"
                      name="avatar-hero"
                      value={choice.value}
                      checked={normalizeAvatarHero(settings.avatarHero) === choice.value}
                      onChange={() => changeSettings({ avatarHero: choice.value })}
                    />
                    <span>
                      <strong>{choice.label}</strong>
                      <span className="meta">{choice.help}</span>
                    </span>
                  </label>
                ))}
              </fieldset>
            </SettingRow>
            <div className="section-body">
              <p className="meta">基準画像は {AVATAR_HERO_BASE.width}×{AVATAR_HERO_BASE.height}（2:1）です。横長（幅/高さ ≥ 1.6）のときは拡大縮小して中央を切り抜きます。正方形・縦長は左のブロック表示になります。横長で幅がおおよそ 2000px 未満のときは粗さの注意を出します。</p>
            </div>
          </section>

          <AssetUpdates />

          <section className="settings-section" id="settings-booth">
            <header><h2>BOOTH の読み込み方</h2><p>拡張機能・内蔵ブラウザ・併用を選びます。</p></header>
            <div className="section-body"><BoothImportSettings onFail={fail} onChange={() => refresh()} pairingId="settings-extension-pairing" /></div>
            {settings.builtinBrowser && <BuiltinBrowserAccessSettings />}
            {settings.builtinBrowser && (
              <SettingRow id="settings-booth-link-target" label="商品ページを開くブラウザ" help="アバター詳細と確認の商品リンクに使います。取得中やエラーのときは既定のブラウザで開きます。">
                <select value={settings.boothLinkTarget === "builtin" ? "builtin" : "external"} onChange={(e) => changeSettings({ boothLinkTarget: e.target.value === "builtin" ? "builtin" : "external" })}>
                  <option value="external">既定のブラウザ</option>
                  <option value="builtin">内蔵ブラウザ</option>
                </select>
              </SettingRow>
            )}
            {settings.builtinBrowser && (
              <SettingRow id="settings-force-dark" label="強制ダークモード" help="ライトテーマのページの背景を暗くし、文字は明るくします。画像・動画・キャンバスは元の色のままです。右パネルと別ウィンドウの両方に、再起動なしで反映されます。">
                <label className="switch-label">
                  <input aria-label="強制ダークモード"
                    type="checkbox"
                    checked={settings.browserForceDark}
                    onChange={(e) => changeSettings({ browserForceDark: e.target.checked })}
                  />
                </label>
              </SettingRow>
            )}
            {settings.builtinBrowser && (
              <SettingRow id="settings-auto-register" label="取得したアセットの自動登録" help="購入完了・ダウンロード完了を検知して登録し、初回に分類先を確認します。ファイルの移動は自動整理の設定に従います。">
                <label className="switch-label"><input aria-label="取得したアセットの自動登録" type="checkbox" checked={settings.browserAutoRegister} onChange={(e) => changeSettings({ browserAutoRegister: e.target.checked })} /></label>
              </SettingRow>
            )}
            {settings.builtinBrowser && (
              <details className="settings-details" id="settings-force-dark-about">
                <summary>強制ダークモードについて</summary>
                <p>開いているページにはすぐに反映されます。設定を変えたあと別のページへ移ると、読み込みが終わるまで元の色が一瞬見えることがあります。そのときはそのページを再読み込みすると、すぐに暗い表示になります。再起動は要りません。</p>
                <p>ログインや入力は操作できます。ログイン状態は変わりません。画像・動画・キャンバスは元の色のままです。色の付いた文字やボタンは明るさも色相も反転します。画像に白い背景が含まれているとその部分は明るく残ります。CSS で指定した背景画像は色が反転することがあります。</p>
                <p>Windows・macOS・Linux で同じ方法を使います。Windows の WebView2 での見た目は実機未確認です。macOS と Linux も未検証です。</p>
              </details>
            )}
            {settings.builtinBrowser && <div className="section-actions" id="settings-clear-browser-data"><button className="button danger" type="button" onClick={() => void (async () => { if (!window.confirm("ブラウザを閉じ、内蔵ブラウザで開いたすべてのサイトのログイン状態と閲覧データを消します。ブックマークは残ります。")) return; try { await api.clearBoothSession(); notify("閲覧データを削除しました"); } catch (err) { fail(errorText(err)); } })()}>内蔵ブラウザの閲覧データを削除</button></div>}
          </section>

          <section className="settings-section" id="settings-data">
            <header><h2>バージョン情報</h2><p>バージョンと更新、ライセンス、同梱の対応表を確認します。</p></header>
            <SettingRow id="settings-version" label="バージョン" help="アプリと拡張機能は別のバージョン番号です。">
              <div className="version-info"><p>アプリ v{version || "確認中"}</p>
                {integration && integration.mode !== "browser" && <>
                  <details className="settings-details"><summary>拡張機能のバージョン</summary><p>推奨する拡張 v{integration.expectedExtensionVersion}</p><p>最後に通信した拡張 {integration.extensionVersion ? `v${integration.extensionVersion}` : "未確認"}</p></details>
                  {integration.extensionVersion && integration.extensionVersion !== integration.expectedExtensionVersion && <p className="warn-text">{extensionVersionMessage(integration.expectedExtensionVersion, integration.extensionVersion)}</p>}
                </>}
              </div>
            </SettingRow>
            <SettingRow id="settings-license" label="ライセンス" help="VRCLibMan は Mozilla Public License 2.0（MPL-2.0）で公開しています。ソースコードは GitHub で入手できます。">
              <div className="inline-control"><button className="button" type="button" onClick={() => void openExternal(REPOSITORY_URL)}>ソースコード</button><button className="button" type="button" onClick={() => void openExternal(`${REPOSITORY_URL}/blob/main/LICENSE`)}>ライセンス全文</button><button className="button" type="button" onClick={() => void openExternal(`${REPOSITORY_URL}/blob/main/THIRD_PARTY_NOTICES.md`)}>第三者のライセンス</button></div>
            </SettingRow>
            <SettingRow id="settings-check-update-on-startup" label="起動時にアップデートを確認" help="配布版の起動時に、GitHub へ1回だけ更新を確認します。オフのときは手動の「更新を確認」だけです。">
              <label className="switch-label">
                <input aria-label="起動時にアップデートを確認"
                  type="checkbox"
                  checked={settings.checkUpdateOnStartup}
                  onChange={(e) => changeSettings({ checkUpdateOnStartup: e.target.checked })}
                />
              </label>
            </SettingRow>
            <SettingRow label="アプリの更新" help="公開済みの配布版を確認して、アプリ内で更新します。ライブラリと設定は引き継ぎます。">
              <AppUpdater beforeInstall={flush} workInProgress={workInProgress} />
            </SettingRow>
            <SyncExportSettings settings={settings} onChange={changeSettings} onBlur={flushSettings} beforeExport={flush} onNotify={notify} onFail={fail} />
            {catalog && <SettingRow id="settings-catalog" label="同梱の対応表" help={`${catalog.credit}（${catalog.author}）・${catalog.count} 件`}>
              <div className="inline-control"><button className="button primary" type="button" onClick={() => void refreshCatalog()}>対応表を更新</button><button className="button" type="button" onClick={() => void openExternal(catalog.sourceUrl)}>出典</button><button className="button" type="button" onClick={() => void openExternal(catalog.termsUrl)}>利用規約</button></div>
            </SettingRow>}
            {catalog && <details className="settings-details"><summary>対応表について詳しく</summary><p>取得日：{catalog.fetchedOn || "未記録"}{catalog.upstreamUpdated ? `・元データの更新日：${catalog.upstreamUpdated}` : ""}</p><p>「対応表を更新」を押すと GitHub から取得します。規約ではコピー、保存、二次利用、商用利用が許されています。正確性の保証はなく、アバターの著作権は各作者にあります。</p></details>}
            <details className="settings-details privacy-notice" id="settings-privacy">
              <summary>プライバシー</summary>
              <p>本アプリには、利用者の情報を開発者へ送信する機能はありません。</p>
              <p>アカウント情報・購入情報・設定は端末内で扱います。分類は端末内の規則だけで行い、商品情報を外部へ送りません。</p>
              <p>内蔵ブラウザでの閲覧やログインには、開いたサイトとの通信を行います。ログイン状態とブックマークは端末内に保存し、選択したファイルのダウンロード時には BOOTH の Cookie を利用します。</p>
              <p>各サイトのログイン状態と閲覧データは「内蔵ブラウザの閲覧データを削除」で削除できます。ライブラリ情報・設定・ブックマークは残ります。</p>
              <p>更新を確認・ダウンロードするときは GitHub と通信します。ライブラリや購入情報は送信しません。</p>
              <p>エクスポートしたファイルには商品名、ショップ名、説明文、サムネイル、ヒーロー画像、ファイル名が含まれます。保存先の管理はご自身で行ってください。</p>
            </details>
          </section>

          <section className="settings-section knowledge-section" id="settings-knowledge">
            <header><h2>対応表を編集</h2><p>アバターの別名と共通素体を編集します。</p></header>
            {knowledge ? <div className="knowledge">
              <div><input className="search" value={filter} placeholder="対応表を検索" onChange={(e) => setFilter(e.target.value)} /><p className="meta">{knowledge?.avatars.length ?? 0} 件中 {avatars.length} 件を表示</p><div className="list">{avatars.map((record) => <button key={record.id} className={record.id === avatar.id ? "selected" : ""} type="button" onClick={() => selectAvatar(record)}>{record.displayName}{record.isBuiltin ? " ・初期" : ""}</button>)}</div><h2>素体</h2><div className="list">{(knowledge?.baseBodies ?? []).map((record) => <button key={record.id} type="button" onClick={() => setBody(record)}>{record.displayName}</button>)}</div></div>
              <div className="stack">
                <div className="panel"><h2>{avatar.id ? "アバターを編集" : "アバターを追加"}</h2><label className="field"><span>表示名</span><input value={avatar.displayName} onChange={(e) => setAvatar({ ...avatar, displayName: e.target.value })} /></label><label className="field"><span>別名（カンマ区切り）</span><input value={avatar.aliases} onChange={(e) => setAvatar({ ...avatar, aliases: e.target.value })} /></label><label className="field"><span>素体</span><select value={avatar.baseBody} onChange={(e) => setAvatar({ ...avatar, baseBody: e.target.value })}><option value="">なし</option>{(knowledge?.baseBodies ?? []).map((record: BaseBody) => <option key={record.id} value={record.id}>{record.displayName}</option>)}</select></label><label className="field"><span>メモ</span><textarea value={avatar.notes} onChange={(e) => setAvatar({ ...avatar, notes: e.target.value })} /></label><div className="row-actions"><button className="button primary" type="button" onClick={() => void saveAvatar()}>保存</button><button className="button" type="button" onClick={() => setAvatar(emptyAvatar)}>新規</button><button className="button danger" type="button" disabled={!avatar.id || avatar.builtin} onClick={() => void removeAvatar()}>削除</button></div></div>
                <div className="panel"><h2>{body.id ? "素体を編集" : "素体を追加"}</h2><label className="field"><span>表示名</span><input value={body.displayName} onChange={(e) => setBody({ ...body, displayName: e.target.value })} /></label><label className="field"><span>根拠のメモ</span><textarea value={body.note} onChange={(e) => setBody({ ...body, note: e.target.value })} /></label><div className="row-actions"><button className="button primary" type="button" onClick={() => void saveBody()}>保存</button><button className="button" type="button" onClick={() => setBody({ id: "", displayName: "", note: "" })}>新規</button><button className="button danger" type="button" disabled={!body.id} onClick={() => void removeBody()}>削除</button></div></div>
              </div>
            </div> : <p className="section-body meta">対応表を読み込んでから編集できます。</p>}
          </section>

          <details className="settings-section debug-section" id="settings-debug">
            <summary>
              <span className="debug-summary-title">デバッグ</span>
              <span className="debug-summary-help">分類ルールの改善に渡すデータです。普段は閉じておきます。</span>
            </summary>
            <ClassificationFeedback />
            <SettingRow id="settings-maintenance" label="メンテナンス" help="不足している初期データの追加や、商品の対応アバターの見直しを行います。通常は更新・編集時に自動で見直します。">
              <div className="inline-control"><button className="button" type="button" onClick={() => void restore()}>初期データを追加</button><button className="button" type="button" onClick={() => void reresolve()}>対応表を再適用</button><button className="button" type="button" onClick={() => void reopenSetup()}>セットアップを再表示</button></div>
            </SettingRow>
          </details>
          </div>
        </div>
      )}
      {saveError && <p className="settings-status warn-text" role="alert">設定を保存できませんでした。{saveError}</p>}
      {status && <p className="settings-status">{status}</p>}

    </section>
  );
}
