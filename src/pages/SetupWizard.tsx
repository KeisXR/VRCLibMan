import { useEffect, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { api, openExternal } from "../api";
import { useShell } from "../App";
import { BoothImportSettings } from "../components/BoothImportSettings";
import { isProgressEnded } from "../progress";
import type { ProgressBoard } from "../progress";
import type { ImportReceipt, IntegrationStatus, LibraryStats } from "../types";
import { errorText } from "../ui";

type Step = "intro" | "extension" | "library" | "classify" | "manual" | "files" | "presence";
type Guided = "library" | "classify" | "manual" | "files";

export type StepState = "todo" | "running" | "done";
export interface StepStatus {
  state: StepState;
  text: string;
}

export const STEP_STATE_LABEL: Record<StepState, string> = { todo: "未実施", running: "処理中", done: "完了" };

/** これより小さい窓では、案内を作業中の画面の下端の細い帯にする（最初は折りたたむ）。 */
export const COMPACT_GUIDE = { width: 1100, height: 760 };

export function compactGuide(width: number, height: number): boolean {
  return width < COMPACT_GUIDE.width || height < COMPACT_GUIDE.height;
}

function runningJob(progress: ProgressBoard, job: string) {
  const entry = progress.jobs[job];
  return entry && !isProgressEnded(entry) ? entry : null;
}

function latestImport(integration: IntegrationStatus | null): string | null {
  const receipts = [integration?.browserImport, integration?.extensionImport].filter(
    (value): value is ImportReceipt => value != null,
  );
  if (receipts.length === 0) return null;
  const latest = receipts.reduce((left, right) => (right.at > left.at ? right : left));
  const date = new Date(latest.at);
  return Number.isNaN(date.getTime()) ? latest.at : date.toLocaleString("ja-JP");
}

/**
 * 手順ごとの状態。商品・未分類・登録ファイルの数と、いま走っている仕事（job-progress）から決める。
 * 「次へ」を押しただけでは完了にしない。任意の手順（分類の手直し）は null。
 */
export function guideStatus(
  step: Guided,
  stats: LibraryStats | null,
  progress: ProgressBoard,
  integration: IntegrationStatus | null,
): StepStatus | null {
  if (step === "manual") return null;
  if (step === "library") {
    const walk = runningJob(progress, "browser-library");
    if (walk) return { state: "running", text: walk.message || "ライブラリを読み込んでいます" };
    if (!stats) return null;
    if (stats.products > 0) {
      const last = latestImport(integration);
      return { state: "done", text: `商品 ${stats.products} 件を読み込み済みです${last ? `（最後の読み込み: ${last}）` : ""}` };
    }
    if (integration && integration.mode !== "browser" && !integration.extensionLastSeenAt) {
      return { state: "todo", text: "まだ商品がありません。拡張機能からの受信もまだありません。" };
    }
    return { state: "todo", text: "まだ商品がありません。" };
  }
  if (step === "classify") {
    const extract = runningJob(progress, "extract");
    if (extract) return { state: "running", text: extract.message || "分類しています" };
    if (!stats) return null;
    if (stats.products === 0) return { state: "todo", text: "商品がまだないため分類できません。前の手順でライブラリを取得してください。" };
    if (stats.unextracted > 0) return { state: "todo", text: `未分類の商品が ${stats.unextracted} 件あります。` };
    return { state: "done", text: "すべての商品にカテゴリがあります。" };
  }
  const scan = runningJob(progress, "scan");
  if (scan) return { state: "running", text: scan.message || "ファイルを調べています" };
  if (!stats) return null;
  if (stats.registeredFiles > 0) return { state: "done", text: `${stats.registeredFiles} 件のファイルが商品に結び付いています。` };
  return { state: "todo", text: "まだファイルを登録していません。" };
}

/** 状態に合わせた進む操作の文言。未実施なら「あとで行う」と書き、終わったつもりで進ませない。 */
export function guideNextLabel(status: StepStatus | null): string {
  if (!status || status.state === "done") return "次へ";
  if (status.state === "running") return "次へ（処理は続きます）";
  return "あとで行う";
}

function useWindowSize() {
  const [size, setSize] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));
  useEffect(() => {
    const update = () => setSize({ width: window.innerWidth, height: window.innerHeight });
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);
  return size;
}

export function SetupWizard({
  onDone,
  onRefresh,
  stats,
  progress,
  railInset = 0,
}: {
  onDone: () => void;
  onRefresh: () => void;
  stats: LibraryStats | null;
  progress: ProgressBoard;
  /** 左サイドバーの幅。小さい窓では案内をその右に置き、サイドバーを塞がない。 */
  railInset?: number;
}) {
  const { revision } = useShell();
  const [step, setStep] = useState<Step>("intro");
  const [backgroundChoice, setBackgroundChoice] = useState<"unset" | "on" | "off">("unset");
  const [loginChoice, setLoginChoice] = useState<"unset" | "on" | "off">("unset");
  const [importReady, setImportReady] = useState(false);
  const [message, setMessage] = useState("");
  const size = useWindowSize();
  const compact = compactGuide(size.width, size.height);
  const [collapsed, setCollapsed] = useState(compact);
  const [browserEnabled, setBrowserEnabled] = useState(false);
  const [integration, setIntegration] = useState<IntegrationStatus | null>(null);

  useEffect(() => {
    let alive = true;
    void api.integrationStatus().then(
      (value) => {
        if (!alive) return;
        setIntegration(value);
        setBrowserEnabled(value.mode !== "extension");
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [revision, step]);

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

  function guide(next: Guided) {
    setMessage("");
    setStep(next);
    window.location.hash = next === "manual" ? "#/classification-log" : "#/import";
    if (next !== "manual") {
      const target = next === "library" ? "import-booth" : next === "classify" ? "import-classification" : "import-existing-files";
      window.setTimeout(() => document.getElementById(target)?.scrollIntoView?.({ block: "start" }), 100);
    }
  }

  const presenceReady = backgroundChoice !== "unset" && loginChoice !== "unset";

  if (step === "library" || step === "classify" || step === "manual" || step === "files") {
    const status = guideStatus(step, stats, progress, integration);
    const guides: Record<Guided, { index: number; title: string; body: ReactNode; back: () => void; extra?: ReactNode; next: () => void }> = {
      library: {
        index: 2,
        title: "ライブラリを取得する",
        body: <>
          {browserEnabled ? (
            <ol className="setup-steps">
              <li>インポート画面の「1. BOOTH から読み込む」で「ブラウザを開く」を押し、BOOTH にログインします。</li>
              <li>同じ場所の「ライブラリ全取得」を押します。</li>
            </ol>
          ) : <>
            <p>拡張機能を選択しています。</p>
            <ol className="setup-steps">
              <li>普段のブラウザで BOOTH のライブラリを開き、ログインします。</li>
              <li>拡張機能の「ライブラリをすべて送る」を押します。</li>
            </ol>
          </>}
          <p className="note-strong">購入・ギフト・無料ダウンロードの商品情報を取得します。ファイルはダウンロードしません。</p>
          <p className="meta">商品情報はあとからでも取得できます。</p>
        </>,
        back: () => setStep("extension"),
        extra: browserEnabled ? <button className="button secondary" type="button" onClick={() => { window.location.hash = "#/import"; }}>インポート画面へ</button> : undefined,
        next: () => guide("classify"),
      },
      classify: {
        index: 3,
        title: "商品を分類する",
        body: <>
          <p>インポート画面の「2. 分類」で「未分類を分類」を押します。BOOTH のカテゴリ・タグで決まらない商品は、タイトルと説明文から分類します。</p>
          <p className="meta">分類できなかった商品は、次の手順で修正します。</p>
        </>,
        back: () => guide("library"),
        extra: <button className="button secondary" type="button" onClick={() => guide("classify")}>分類画面へ</button>,
        next: () => guide("manual"),
      },
      manual: {
        index: 4,
        title: "分類を手動で直す",
        body: <>
          <ol className="setup-steps">
            <li>「ログ」の「分類記録」で、分類が何をどこへ振り分けたかを見られます。違っていた変更は 1 件ずつ元に戻せます。</li>
            <li>商品のカテゴリ・ジャンル・対応アバターを直すときは、商品の「⋯」から「分類を編集」を開きます。</li>
            <li>未分類の商品は、インポートの「商品を管理・除外」で絞り込めます。</li>
          </ol>
          <p className="meta">この手順は任意です。分類は自動で反映されます。直す必要がなければ、そのまま次へ進めます。</p>
        </>,
        back: () => guide("classify"),
        extra: <button className="button secondary" type="button" onClick={() => { window.location.hash = "#/classification-log"; }}>分類記録へ</button>,
        next: () => guide("files"),
      },
      files: {
        index: 5,
        title: "ダウンロード済みファイルを登録する",
        body: <>
          <ol className="setup-steps">
            <li>インポート画面の「3. 手元のファイル」で「取り込み元を選ぶ」を押し、Downloads などを選びます。</li>
            <li>照合結果で、各ファイルに対応する商品を確認します。</li>
            <li>取り込み方法を選び、「選択した ○ 件を取り込む」を押します。</li>
          </ol>
          <p className="note-strong">「元の場所のまま登録」はファイルを移動しません。「ライブラリへコピー」は元のファイルを残してコピーします。</p>
          <p className="meta">照合には BOOTH の商品・ファイル情報が必要です。ファイルの登録はあとからでも行えます。</p>
        </>,
        back: () => guide("manual"),
        extra: <button className="button secondary" type="button" onClick={() => guide("files")}>ファイル登録画面へ</button>,
        next: () => setStep("presence"),
      },
    };
    const current = guides[step];
    return (
      <div
        className={`setup-guide${compact ? " compact" : ""}${collapsed ? " collapsed" : ""}`}
        style={{ "--guide-left": `${railInset}px` } as CSSProperties}
        role="region"
        aria-label="使い方ガイド"
      >
        <div className="setup-guide-card panel">
          <div className="setup-guide-head">
            <div className="setup-guide-heading">
              <p className="meta">使い方ガイド · {current.index} / 5</p>
              {collapsed ? <strong className="setup-guide-title">{current.title}</strong> : <h1>{current.title}</h1>}
            </div>
            <button className="setup-guide-toggle button secondary" type="button" onClick={() => setCollapsed(!collapsed)} aria-expanded={!collapsed}>
              {collapsed ? "手順を表示" : "折りたたむ"}
            </button>
          </div>
          {status && (
            <p className={`setup-status ${status.state}`} role="status">
              <span className={`state-chip ${status.state}`}>{STEP_STATE_LABEL[status.state]}</span>
              <span className="setup-status-text">{status.text}</span>
            </p>
          )}
          {!collapsed && current.body}
          {message && <p className="note-strong" role="alert">{message}</p>}
          <div className="row-actions setup-actions">
            <button className="button" type="button" onClick={current.back}>戻る</button>
            {!collapsed && current.extra}
            <button className="button primary" type="button" onClick={current.next}>{guideNextLabel(status)}</button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="setup-screen">
      <div className="setup-card panel">
        <p className="meta">初回セットアップ</p>
        <h1>
          {step === "intro" ? "VRCLibMan へようこそ" : step === "extension" ? "BOOTH の読み込み方" : "終了と起動の設定"}
        </h1>
        {step === "intro" && <>
          <p>BOOTH の商品と手元のファイルをまとめて管理し、持っているアバターや対応する衣装を探せるアプリです。</p>
          <p>商品は BOOTH のカテゴリとタグ、タイトルと説明文、ダウンロードファイルの名前から分類します。追加のソフトは要りません。分類結果は手動で修正できます。</p>
          <p>ライブラリの取得、分類、ファイルの登録を順に進めます。</p>
          <div className="row-actions setup-actions"><button className="button primary" type="button" onClick={() => setStep("extension")}>はじめる</button></div>
        </>}
        {step === "extension" && (
          <>
            <p>商品情報の取得方法を選びます。「アプリの内蔵ブラウザ」なら、拡張機能のインストールは不要です。</p>
            <BoothImportSettings requireChoice onChange={(mode) => { setBrowserEnabled(mode !== "extension"); onRefresh(); }} onFail={setMessage} onReady={setImportReady} navigateBrowser={() => guide("library")} />
            {message && <p className="note-strong" role="alert">{message}</p>}
            <div className="row-actions setup-actions">
              <button className="button" type="button" onClick={() => setStep("intro")}>戻る</button>
              <button className="button primary" type="button" disabled={!importReady} onClick={() => guide("library")}>次へ</button>
            </div>
          </>
        )}
        {step === "presence" && (
          <>
            <p>最後に、ウィンドウを閉じたときの動作と自動起動を選びます。</p>
            <button className="button secondary" type="button" onClick={() => void openExternal("https://github.com/KeisXR/VRCLibMan/blob/main/how-to-use.md").catch((err: unknown) => setMessage(errorText(err)))}>使い方を詳しく見る</button>
            <p>ウィンドウを閉じたとき</p>
            <div className="row-actions">
              <button
                className={backgroundChoice === "on" ? "button primary" : "button secondary"}
                type="button"
                aria-pressed={backgroundChoice === "on"}
                onClick={() => setBackgroundChoice("on")}
              >
                バックグラウンドで続ける
              </button>
              <button
                className={backgroundChoice === "off" ? "button primary" : "button secondary"}
                type="button"
                aria-pressed={backgroundChoice === "off"}
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
                className={loginChoice === "on" ? "button primary" : "button secondary"}
                type="button"
                aria-pressed={loginChoice === "on"}
                disabled={backgroundChoice !== "on"}
                onClick={() => setLoginChoice("on")}
              >
                自動起動する
              </button>
              <button
                className={loginChoice === "off" ? "button primary" : "button secondary"}
                type="button"
                aria-pressed={loginChoice === "off"}
                onClick={() => setLoginChoice("off")}
              >
                自動起動しない
              </button>
            </div>
            <p className="meta">
              自動起動は、バックグラウンドで続ける場合に選べます。
            </p>
            <p className="meta">設定はあとから変更できます。「あとで設定する」を選ぶと、バックグラウンド動作と自動起動をオフにして完了します。</p>
            {message && <p className="note-strong" role="alert">{message}</p>}
            <div className="row-actions setup-actions">
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
      </div>
    </div>
  );
}
