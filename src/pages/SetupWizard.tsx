import { useEffect, useState } from "react";
import { api, listenProgress } from "../api";
import { DEFAULT_OLLAMA_MODEL } from "../types";
import type { AppleFmStatus, JobProgress, OllamaStatus, SetupStatus } from "../types";
import { ModelPicker } from "../components/ModelPicker";
import { BoothImportSettings } from "../components/BoothImportSettings";
import { modelDownloadPrompt, modelInstalled } from "../models";
import { errorText } from "../ui";

export function SetupWizard({ onDone }: { onDone: () => void }) {
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [apple, setApple] = useState<AppleFmStatus | null>(null);
  const [ollamaError, setOllamaError] = useState("");
  const [checking, setChecking] = useState(true);
  const [step, setStep] = useState<"model" | "extension" | "presence">("model");
  const [backgroundChoice, setBackgroundChoice] = useState<"unset" | "on" | "off">("unset");
  const [loginChoice, setLoginChoice] = useState<"unset" | "on" | "off">("unset");
  const [pulling, setPulling] = useState(false);
  const [progress, setProgress] = useState<JobProgress | null>(null);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [importReady, setImportReady] = useState(false);
  const [message, setMessage] = useState("");

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
      setMessage("Apple のオンデバイスモデルを使います。Ollama のモデルは取得しません。");
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

  const presenceReady = backgroundChoice !== "unset" && loginChoice !== "unset";
  const model = selectedModel ?? status?.model ?? DEFAULT_OLLAMA_MODEL;
  const defaultModel = status?.defaultModel || DEFAULT_OLLAMA_MODEL;
  const usingApple = status?.backend === "apple";
  const appleAvailable = apple?.availability === "available";
  const ready = usingApple || modelInstalled(ollama?.models ?? [], model);
  const connected = ollama != null && ollamaError === "";
  const percent = progress && progress.total > 0 ? Math.min(100, (progress.current / progress.total) * 100) : 0;

  return (
    <div className="setup-screen">
      <div className="setup-card panel">
        <p className="meta">初回セットアップ</p>
        <h1>
          {step === "model" ? "分類用のモデル" : step === "extension" ? "BOOTH の購入情報の読み込み方" : "閉じたあとの動き"}
        </h1>
        {step === "model" && (
          <>
            <p>分類に使うモデルを選んでください。軽量な qwen3.5:4b を推奨します。既定は {defaultModel}、保存済みの設定は {status?.model || defaultModel} です。あとから設定でも変更できます。</p>
            <ModelPicker value={model} models={ollama?.models ?? []} checked={connected} disabled={checking || pulling || usingApple} onChange={setSelectedModel} />
            {checking && <p className="meta">Ollama を確認しています</p>}
            {!checking && !connected && <p className="meta">Ollama にはまだつながっていません。{ollamaError}</p>}
            {!checking && connected && !usingApple && <p className="meta">{model || "モデル"} は{ready ? "導入済みです。" : "未導入です。取得するか、別のモデルを選べます。"}</p>}
            {pulling && (
              <div className="progress">
                <p>{progress?.message ?? "取得しています"}</p>
                <div className={progress && progress.total > 0 ? "track" : "track indeterminate"}>
                  <span style={{ width: progress && progress.total > 0 ? `${percent}%` : undefined }} />
                </div>
              </div>
            )}
            {appleAvailable && !usingApple && (
              <p className="meta">この Mac では Apple のオンデバイスモデルも使えます。選ぶと、Ollama のモデルは取得しません。既定は {defaultModel} のままです。</p>
            )}
            {!checking && apple && !appleAvailable && apple.message && (
              <p className="meta">{apple.message}</p>
            )}
            {usingApple && <><p className="meta">分類は Apple のオンデバイスモデルです。Ollama の取得は飛ばします。</p><button className="button" type="button" onClick={() => void api.settings().then(async (settings) => { await api.saveSettings({ ...settings, llmBackend: "ollama" }); await check(); }).catch((err: unknown) => setMessage(errorText(err)))}>Ollama を使う</button></>}
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
                {ready ? "次へ" : "スキップして次へ"}
              </button>
            </div>
          </>
        )}
        {step === "extension" && (
          <>
            <p>どちらの方法で BOOTH のライブラリを読み込むか選んでください。</p>
            <BoothImportSettings onFail={setMessage} onReady={setImportReady} navigateBrowser={() => setStep("presence")} />
            {message && <p className="meta" role="status">{message}</p>}
            <div className="row-actions">
              <button className="button" type="button" onClick={() => setStep("model")}>モデル選択へ戻る</button>
              <button className="button primary" type="button" disabled={!importReady} onClick={() => setStep("presence")}>次へ</button>
            </div>
          </>
        )}
        {step === "presence" && (
          <>
            <p>
              どちらも既定はオフです。オンは選んだときだけ有効になります。あとから設定でも変えられます。
            </p>
            <p>ウィンドウを閉じても、ダウンロードの受け付けを続けますか？</p>
            <div className="row-actions">
              <button
                className={backgroundChoice === "on" ? "button primary" : "button"}
                type="button"
                onClick={() => setBackgroundChoice("on")}
              >
                オン
              </button>
              <button
                className={backgroundChoice === "off" ? "button primary" : "button"}
                type="button"
                onClick={() => {
                  setBackgroundChoice("off");
                  setLoginChoice("off");
                }}
              >
                オフ
              </button>
            </div>
            <p className="meta">
              オフのときはウィンドウを閉じるとアプリも終了します。オンのときはウィンドウだけ閉じて、トレイ（Windows）またはメニューバー（macOS）から開き直せます。
            </p>
            <p>ログインしたときに起動しますか？</p>
            <div className="row-actions">
              <button
                className={loginChoice === "on" ? "button primary" : "button"}
                type="button"
                disabled={backgroundChoice !== "on"}
                onClick={() => setLoginChoice("on")}
              >
                オン
              </button>
              <button
                className={loginChoice === "off" ? "button primary" : "button"}
                type="button"
                onClick={() => setLoginChoice("off")}
              >
                オフ
              </button>
            </div>
            <p className="meta">
              ログイン時の起動は、バックグラウンドをオンにしたときだけ使えます。両方オンのとき、ログインから起動したプロセスはウィンドウを出さず、トレイまたはメニューバーから開けます。
            </p>
            {message && <p className="meta">{message}</p>}
            <div className="row-actions">
              <button className="button" type="button" onClick={() => void chooseLater()}>
                あとで設定で選ぶ
              </button>
              <button className="button primary" type="button" disabled={!presenceReady} onClick={() => void finishPresence()}>
                はじめる
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
