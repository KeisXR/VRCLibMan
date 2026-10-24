import { useEffect, useState } from "react";
import { api, listenProgress, openExternal } from "../api";
import { DEFAULT_OLLAMA_MODEL } from "../types";
import type { AppleFmStatus, ExtensionLink, JobProgress, OllamaStatus, SetupStatus } from "../types";
import { formatBytes } from "../media";
import { errorText } from "../ui";

const LIBRARY_URL = "https://accounts.booth.pm/library";

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
  const [link, setLink] = useState<ExtensionLink | null>(null);
  const [copied, setCopied] = useState(false);
  const [message, setMessage] = useState("");

  async function check() {
    setChecking(true);
    setOllamaError("");
    try {
      const [nextStatus, nextOllama, nextApple] = await Promise.all([
        api.setupStatus(),
        api.testOllama(),
        api.appleFmStatus().catch(() => null),
      ]);
      setStatus(nextStatus);
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

  async function useInstalled(name: string) {
    if (!name) return;
    setMessage("");
    try {
      const settings = await api.settings();
      await api.saveSettings({ ...settings, ollamaModel: name });
      setMessage(`今の設定を ${name} にしました。既定は ${defaultModel} のままです。`);
      await check();
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  async function pull() {
    setPulling(true);
    setMessage("");
    setProgress(null);
    try {
      await api.pullOllamaModel();
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
      setMessage("Apple のオンデバイスモデルを使います。Ollama のモデルは取得しません。既定のモデルは gemma4:12b のままです。");
      await check();
      await showExtension();
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  async function showExtension() {
    setStep("extension");
    try {
      setLink(await api.extensionLink());
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  async function copyCode() {
    if (!link) return;
    await navigator.clipboard.writeText(link.token);
    setCopied(true);
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
  const model = status?.model || DEFAULT_OLLAMA_MODEL;
  const defaultModel = status?.defaultModel || DEFAULT_OLLAMA_MODEL;
  const usingApple = status?.backend === "apple";
  const appleAvailable = apple?.availability === "available";
  const ready = usingApple || Boolean(ollama?.modelReady);
  const connected = ollama != null && ollamaError === "";
  const installedSize =
    ollama?.modelSizeBytes != null && ollama.modelSizeBytes > 0 ? formatBytes(ollama.modelSizeBytes) : null;
  const percent = progress && progress.total > 0 ? Math.min(100, (progress.current / progress.total) * 100) : 0;

  return (
    <div className="setup-screen">
      <div className="setup-card panel">
        <p className="meta">初回セットアップ</p>
        <h1>
          {step === "model" ? "分類用のモデル" : step === "extension" ? "Booth から取り込む" : "閉じたあとの動き"}
        </h1>
        {step === "model" && (
          <>
            <p>
              既定のモデルは {defaultModel} です。今の設定は {model} です。
              商品名の整理には、この PC の Ollama と今の設定を使います。
              {installedSize ? `入っているサイズは ${installedSize} です。` : ""}
              ダウンロードはここでの確認のあとだけ行い、あとから設定でも変えられます。
            </p>
            {checking && <p className="meta">Ollama を確認しています</p>}
            {!checking && !connected && (
              <p className="meta">Ollama にはまだつながっていません。{ollamaError}</p>
            )}
            {!checking && connected && ready && (
              <p className="meta">
                {model} は入っています。
                {installedSize ? `サイズは ${installedSize} です。` : ""}
              </p>
            )}
            {!checking && connected && !ready && (
              <p className="meta">{model} はまだありません。取得するか、入っているモデルから選べます。設定は自動では切り替わりません。</p>
            )}
            {!checking && connected && !ready && (ollama?.models.length ?? 0) > 0 && (
              <label className="field">
                <span>入っているモデル</span>
                <select
                  value=""
                  disabled={pulling}
                  onChange={(event) => {
                    const name = event.target.value;
                    if (name) void useInstalled(name);
                  }}
                >
                  <option value="">選んで切り替える</option>
                  {ollama?.models.map((name) => (
                    <option key={name} value={name}>
                      {name}
                    </option>
                  ))}
                </select>
              </label>
            )}
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
            {usingApple && <p className="meta">分類は Apple のオンデバイスモデルです。Ollama の取得は飛ばします。</p>}
            {message && <p className="meta">{message}</p>}
            <div className="row-actions">
              {appleAvailable && !usingApple && (
                <button className="button" type="button" disabled={pulling} onClick={() => void useApple()}>
                  Apple のオンデバイスモデルを使う
                </button>
              )}
              {!checking && connected && !ready && !usingApple && (
                <button className="button primary" type="button" disabled={pulling} onClick={() => void pull()}>
                  ダウンロードする
                </button>
              )}
              <button className="button" type="button" disabled={checking || pulling} onClick={() => void check()}>
                もう一度確認
              </button>
              <button className="button primary" type="button" disabled={pulling} onClick={() => void showExtension()}>
                {ready ? "次へ" : "スキップして次へ"}
              </button>
            </div>
          </>
        )}
        {step === "extension" && (
          <>
            <p>いつものインポートは Chrome 拡張です。次の順で一度だけ結んでください。</p>
            <ol className="setup-steps">
              <li>Chrome の拡張機能ページでデベロッパーモードをオンにし、「パッケージ化されていない拡張機能を読み込む」から extension フォルダを選びます。</li>
              <li>下のペアリングコードをコピーし、拡張のポップアップに貼って保存します。</li>
              <li>「ライブラリを開く」で Booth のライブラリを開き、拡張の「ライブラリをすべて送る」か「新しい購入だけ送る」を押します。</li>
            </ol>
            <p className="meta">
              内蔵ブラウザは設定で有効にする別のモードです。拡張が使えないときの任意の手段で、ログインはメールとパスワードだけを想定しています。
            </p>
            {link && (
              <label className="field">
                <span>ペアリングコード</span>
                <input value={link.token} readOnly />
              </label>
            )}
            {message && <p className="meta">{message}</p>}
            <div className="row-actions">
              <button className="button" type="button" disabled={!link} onClick={() => void copyCode()}>
                {copied ? "コピーしました" : "コードをコピー"}
              </button>
              <button className="button" type="button" onClick={() => void openExternal(LIBRARY_URL)}>
                ライブラリを開く
              </button>
              <button className="button primary" type="button" onClick={() => setStep("presence")}>
                次へ
              </button>
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
