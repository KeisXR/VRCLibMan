import { useEffect, useState } from "react";
import { api, listenProgress, openExternal } from "../api";
import { DEFAULT_OLLAMA_MODEL } from "../types";
import type { AppleFmStatus, JobProgress, OllamaStatus, SetupStatus } from "../types";
import { ModelPicker } from "../components/ModelPicker";
import { BoothImportSettings } from "../components/BoothImportSettings";
import { modelDownloadPrompt, modelInstalled } from "../models";
import { ProgressTrack, errorText } from "../ui";
import { progressState } from "../progress";

export function SetupWizard({ onDone, onRefresh }: { onDone: () => void; onRefresh: () => void }) {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [apple, setApple] = useState<AppleFmStatus | null>(null);
  const [ollamaError, setOllamaError] = useState("");
  const [checking, setChecking] = useState(true);
  const [step, setStep] = useState<"intro" | "model" | "extension" | "library" | "classify" | "manual" | "files" | "presence">("intro");
  const [backgroundChoice, setBackgroundChoice] = useState<"unset" | "on" | "off">("unset");
  const [loginChoice, setLoginChoice] = useState<"unset" | "on" | "off">("unset");
  const [pulling, setPulling] = useState(false);
  const [progress, setProgress] = useState<JobProgress | null>(null);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [importReady, setImportReady] = useState(false);
  const [message, setMessage] = useState("");
  const [collapsed, setCollapsed] = useState(false);
  const [browserEnabled, setBrowserEnabled] = useState(false);

  async function check() {
    setChecking(true);
    setOllamaError("");
    try {
      const [nextStatus, nextOllama, nextApple] = await Promise.all([
        api.setupStatus(),
        api.testOllama().catch((err: unknown) => { setOllamaError(errorText(err)); return null; }),
        api.appleFmStatus().catch(() => null),
      ]);
      setStatus(nextStatus);
      setSelectedModel((current) => current ?? nextStatus.model);
      setOllama(nextOllama);
      setApple(nextApple);
    } catch (err) {
      setOllama(null);
      setOllamaError(errorText(err));
      const nextStatus = await api.setupStatus().catch(() => null);
      if (nextStatus) setStatus(nextStatus);
    } finally {
      setChecking(false);
    }
  }

  useEffect(() => {
    void check();
    void api.integrationStatus().then((value) => setBrowserEnabled(value.mode !== "extension")).catch(() => undefined);
  }, []);

  useEffect(() => {
    let stop = () => undefined as void;
    void listenProgress((next) => {
      if (next.job === "pull") setProgress(next);
    }).then((unlisten) => {
      stop = unlisten;
    });
    return () => stop();
  }, []);

  async function saveModel() {
    const name = model.trim();
    if (!name) throw new Error("モデル名を入力してください");
    const settings = await api.settings();
    await api.saveSettings({ ...settings, ollamaModel: name });
  }

  async function pull() {
    if (!window.confirm(modelDownloadPrompt(model.trim()))) return;
    setPulling(true);
    setMessage("");
    setProgress(null);
    try {
      await saveModel();
      await api.pullOllamaModel(model.trim());
      setMessage("モデルを取得しました");
      await check();
    } catch (err) {
      setMessage(errorText(err));
    } finally {
      setPulling(false);
    }
  }

  async function useApple() {
    setMessage("");
    try {
      const settings = await api.settings();
      await api.saveSettings({ ...settings, llmBackend: "apple" });
      setMessage("Apple のオンデバイスモデルを使います。");
      await check();
      setStep("extension");
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  async function showExtension() {
    setMessage("");
    try {
      if (!usingApple) await saveModel();
      setStep("extension");
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  async function savePresence(backgroundMode: boolean, launchAtLogin: boolean) {
    const settings = await api.settings();
    await api.saveSettings({
      ...settings,
      backgroundMode,
      launchAtLogin: backgroundMode && launchAtLogin,
    });
    await api.completeSetup();
    onDone();
  }

  async function finishPresence() {
    if (backgroundChoice === "unset" || loginChoice === "unset") return;
    setMessage("");
    try {
      await savePresence(backgroundChoice === "on", loginChoice === "on");
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  async function chooseLater() {
    setMessage("");
    try {
      await savePresence(false, false);
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  function guide(next: "library" | "classify" | "manual" | "files") {
    setMessage("");
    setStep(next);
    setCollapsed(false);
    window.location.hash = next === "library" ? "#/browser" : next === "manual" ? "#/classification-log" : "#/import";
    if (next === "classify" || next === "files") {
      window.setTimeout(() => document.getElementById(next === "classify" ? "import-classification" : "import-existing-files")?.scrollIntoView?.({ block: "start" }), 100);
    }
  }

  const guided = step === "library" || step === "classify" || step === "manual" || step === "files";
  const guideTitles = { library: "ライブラリを取得する", classify: "商品を分類する", manual: "分類を手動で直す", files: "ダウンロード済みファイルを登録する" };
  const presenceReady = backgroundChoice !== "unset" && loginChoice !== "unset";
  const model = selectedModel ?? status?.model ?? DEFAULT_OLLAMA_MODEL;
  const usingCloud = status?.backend === "cloud";
  const usingApple = status?.backend === "apple" || usingCloud;
  const appleAvailable = apple?.availability === "available";
  const ready = usingApple || modelInstalled(ollama?.models ?? [], model);
  const connected = ollama != null && ollamaError === "";

  return (
    <div className={guided ? "setup-guide" : "setup-screen"}>
      <div className={guided ? "setup-guide-card panel" : "setup-card panel"}>
        <p className="meta">{guided ? `使い方ガイド · ${step === "library" ? "3" : step === "classify" ? "4" : step === "manual" ? "5" : "6"} / 6` : "初回セットアップ"}</p>
        {guided && <button className="setup-guide-toggle button" type="button" onClick={() => setCollapsed(!collapsed)} aria-expanded={!collapsed}>{collapsed ? "案内を開く" : "折りたたむ"}</button>}
        {(!guided || !collapsed) && <>
        <h1>
          {step === "intro" ? "VRCLibMan へようこそ" : step === "model" ? "分類用のモデル" : step === "extension" ? "BOOTH の読み込み方" : guided ? guideTitles[step] : "終了と起動の設定"}
        </h1>
        {step === "intro" && <>
          <p>BOOTH の商品と手元のファイルをまとめて管理し、持っているアバターや対応する衣装を探せるアプリです。</p>
          <p>Ollama は、この PC で AI モデルを動かすためのアプリです。VRCLibMan はそのモデルで商品情報を分類します。分類結果は手動で修正できます。</p>
          <p>モデルの準備、ライブラリの取得、分類、ファイルの登録を順に進めます。</p>
          <div className="row-actions"><button className="button primary" type="button" onClick={() => setStep("model")}>はじめる</button></div>
        </>}
        {step === "model" && (
          <>
            <p>分類に使うモデルを選びます。迷ったら qwen3.5:9b を選んでください。</p>
            <p className="meta">Ollama がない場合は、公式サイトからインストールして起動します。起動後に「もう一度確認」を押してください。</p>
            <button className="button" type="button" onClick={() => void openExternal("https://ollama.com/download").catch((err: unknown) => setMessage(errorText(err)))}>Ollama の入手先</button>
            {!usingApple && <ModelPicker value={model} models={ollama?.models ?? []} checked={connected} disabled={checking || pulling} onChange={setSelectedModel} />}
            {!usingApple && checking && <p className="meta">Ollama 接続確認中</p>}
            {!usingApple && !checking && !connected && <p className="meta">Ollama につながりません。{ollamaError}</p>}
            {!checking && connected && !usingApple && <p className="meta">{model || "モデル"} は{ready ? "導入済みです。" : "未導入です。「ダウンロードする」で取得できます。"}</p>}
            {!checking && !ready && <p className="meta">モデルの準備をあとで行う場合は「スキップ」を選びます。</p>}
            {pulling && (
              <div className="progress">
                <p>{progress?.message ?? "取得中"}</p>
                <ProgressTrack
                  current={progress?.current ?? 0}
                  total={progress?.total ?? 0}
                  state={progress ? progressState(progress) : "running"}
                  label={progress?.message ?? "取得中"}
                />
              </div>
            )}
            {appleAvailable && !usingApple && (
              <p className="meta">この Mac では Apple のオンデバイスモデルも使えます。追加のモデルダウンロードは不要です。</p>
            )}
            {!checking && usingApple && apple && !appleAvailable && apple.message && (
              <p className="meta">{apple.message}</p>
            )}
            {usingApple && <><p className="meta">{usingCloud ? "クラウド(APIキー)で分類します。キーと接続先は設定で変更できます。" : "Apple のオンデバイスモデルで分類します。"}</p><button className="button" type="button" onClick={() => void api.settings().then(async (settings) => { await api.saveSettings({ ...settings, llmBackend: "ollama" }); await check(); }).catch((err: unknown) => setMessage(errorText(err)))}>Ollama を使う</button></>}
            {message && <p className="meta">{message}</p>}
            <div className="row-actions">
              {appleAvailable && !usingApple && (
                <button className="button" type="button" disabled={pulling} onClick={() => void useApple()}>
                  Apple のオンデバイスモデルを使う
                </button>
              )}
              {!checking && connected && !ready && !usingApple && (
                <button className="button primary" type="button" disabled={pulling || !model.trim()} onClick={() => void pull()}>
                  ダウンロードする
                </button>
              )}
              <button className="button" type="button" disabled={checking || pulling} onClick={() => void check()}>
                もう一度確認
              </button>
              <button className="button primary" type="button" disabled={checking || pulling || (!usingApple && !model.trim())} onClick={() => void showExtension()}>
                {ready ? "次へ" : "スキップ"}
              </button>
            </div>
          </>
        )}
        {step === "extension" && (
          <>
            <p>商品情報の取得方法を選びます。「アプリの内蔵ブラウザ」なら、拡張機能のインストールは不要です。</p>
            <BoothImportSettings onChange={(mode) => { setBrowserEnabled(mode !== "extension"); onRefresh(); }} onFail={setMessage} onReady={setImportReady} navigateBrowser={() => guide("library")} />
            {message && <p className="meta" role="status">{message}</p>}
            <div className="row-actions">
              <button className="button" type="button" onClick={() => setStep("model")}>戻る</button>
              <button className="button primary" type="button" disabled={!importReady} onClick={() => guide("library")}>次へ</button>
            </div>
          </>
        )}
        {step === "library" && <>
          {browserEnabled ? <>
            <ol className="setup-steps">
              <li>ブラウザ画面の「ブラウザを開く」を押し、BOOTH にログインします。</li>
              <li>ブラウザ画面に戻り、「ライブラリ全取得」を押します。</li>
            </ol>
          </> : <>
            <p>拡張機能を選択しています。</p>
            <ol className="setup-steps">
              <li>普段のブラウザで BOOTH のライブラリを開き、ログインします。</li>
              <li>拡張機能の「ライブラリをすべて送る」を押します。</li>
            </ol>
          </>}
          <p className="meta">購入・ギフト・無料ダウンロードの商品情報を取得します。ファイルはダウンロードしません。</p>
          <p className="meta">商品情報はあとからでも取得できます。</p>
          <div className="row-actions">
            <button className="button" type="button" onClick={() => setStep("extension")}>戻る</button>
            {browserEnabled && <button className="button" type="button" onClick={() => { window.location.hash = "#/browser"; }}>ブラウザ画面へ</button>}
            <button className="button primary" type="button" onClick={() => guide("classify")}>次へ</button>
          </div>
        </>}
        {step === "classify" && <>
          <p>インポート画面の「2. 分類」で「未分類を分類」を押します。BOOTH のカテゴリ・タグで決まらない商品は、選んだモデルで分類します。</p>
          <p className="meta">商品がない場合は「戻る」からライブラリを取得してください。分類できなかった商品は、次の手順で修正します。</p>
          <div className="row-actions"><button className="button" type="button" onClick={() => guide("library")}>戻る</button><button className="button" type="button" onClick={() => setStep("model")}>モデルを確認</button><button className="button" type="button" onClick={() => guide("classify")}>分類画面へ</button><button className="button primary" type="button" onClick={() => guide("manual")}>次へ</button></div>
        </>}
        {step === "manual" && <>
          <ol className="setup-steps">
            <li>「分類記録」で、分類が何をどこへ振り分けたかを見られます。違っていた変更は 1 件ずつ元に戻せます。</li>
            <li>商品のカテゴリ・ジャンル・対応アバターを直すときは、商品の「⋯」から「分類を編集」を開きます。</li>
            <li>未分類の商品は、インポートの「商品を管理・除外」で絞り込めます。</li>
          </ol>
          <p className="meta">分類は自動で反映されます。直す必要がなければ、そのまま次へ進めます。</p>
          <div className="row-actions"><button className="button" type="button" onClick={() => guide("classify")}>戻る</button><button className="button" type="button" onClick={() => { window.location.hash = "#/classification-log"; }}>分類記録へ</button><button className="button primary" type="button" onClick={() => guide("files")}>次へ</button></div>
        </>}
        {step === "files" && <>
          <ol className="setup-steps">
            <li>インポート画面の「3. 手元のファイル」で「取り込み元を選ぶ」を押し、Downloads などを選びます。</li>
            <li>照合結果で、各ファイルに対応する商品を確認します。</li>
            <li>取り込み方法を選び、「選択した ○ 件を取り込む」を押します。</li>
          </ol>
          <p className="meta">「元の場所のまま登録」はファイルを移動しません。「ライブラリへコピー」は元のファイルを残してコピーします。</p>
          <p className="meta">照合には BOOTH の商品・ファイル情報が必要です。ファイルの登録はあとからでも行えます。</p>
          <div className="row-actions"><button className="button" type="button" onClick={() => guide("manual")}>戻る</button><button className="button" type="button" onClick={() => guide("files")}>ファイル登録画面へ</button><button className="button primary" type="button" onClick={() => setStep("presence")}>次へ</button></div>
        </>}
        {step === "presence" && (
          <>
            <p>最後に、ウィンドウを閉じたときの動作と自動起動を選びます。</p>
            <button className="button" type="button" onClick={() => void openExternal("https://github.com/KeisXR/VRCLibMan/blob/main/how-to-use.md").catch((err: unknown) => setMessage(errorText(err)))}>使い方を詳しく見る</button>
            <p>ウィンドウを閉じたとき</p>
            <div className="row-actions">
              <button
                className={backgroundChoice === "on" ? "button primary" : "button"}
                type="button"
                onClick={() => setBackgroundChoice("on")}
              >
                バックグラウンドで続ける
              </button>
              <button
                className={backgroundChoice === "off" ? "button primary" : "button"}
                type="button"
                onClick={() => {
                  setBackgroundChoice("off");
                  setLoginChoice("off");
                }}
              >
                アプリを終了する
              </button>
            </div>
            <p className="meta">
              バックグラウンドで続けると、商品情報の受信や自動整理を続けます。トレイまたはメニューバーから開き直せます。
            </p>
            <p>PC にログインしたとき</p>
            <div className="row-actions">
              <button
                className={loginChoice === "on" ? "button primary" : "button"}
                type="button"
                disabled={backgroundChoice !== "on"}
                onClick={() => setLoginChoice("on")}
              >
                自動起動する
              </button>
              <button
                className={loginChoice === "off" ? "button primary" : "button"}
                type="button"
                onClick={() => setLoginChoice("off")}
              >
                自動起動しない
              </button>
            </div>
            <p className="meta">
              自動起動は、バックグラウンドで続ける場合に選べます。
            </p>
            <p className="meta">設定はあとから変更できます。「あとで設定する」を選ぶと、バックグラウンド動作と自動起動をオフにして完了します。</p>
            {message && <p className="meta">{message}</p>}
            <div className="row-actions">
              <button className="button" type="button" onClick={() => guide("files")}>戻る</button>
              <button className="button" type="button" onClick={() => void chooseLater()}>
                あとで設定する
              </button>
              <button className="button primary" type="button" disabled={!presenceReady} onClick={() => void finishPresence()}>
                完了
              </button>
            </div>
          </>
        )}
        </>}
      </div>
    </div>
  );
}
