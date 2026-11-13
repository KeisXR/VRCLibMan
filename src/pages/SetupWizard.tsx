import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode, RefObject } from "react";
import { scrollToAnchor } from "../anchorScroll";
import { api, chooseFolder, isTauri, openExternal } from "../api";
import { HOW_TO_USE_URL } from "../helpLinks";
import { useShell } from "../App";
import { BoothImportSettings } from "../components/BoothImportSettings";
import { isProgressEnded } from "../progress";
import type { ProgressBoard } from "../progress";
import type { ImportReceipt, IntegrationStatus, LibraryStats, Settings } from "../types";
import { errorText } from "../ui";

type Step = "intro" | "extension" | "library" | "classify" | "manual" | "files" | "storage" | "presence";
type Guided = "library" | "classify" | "manual" | "files";

export type StepState = "todo" | "running" | "done";
export interface StepStatus {
  state: StepState;
  text: string;
}

export const STEP_STATE_LABEL: Record<StepState, string> = { todo: "未実施", running: "処理中", done: "完了" };

/**
 * セットアップの手順の通し番号（紹介の画面は数えない）。カードの画面も、作業中の画面の下端に出す案内も、
 * 同じ「初回セットアップ · n / 7」で数える。どこまで進んだか、あと何画面かが分かるようにするため。
 */
export const SETUP_STEPS = ["extension", "library", "classify", "manual", "files", "storage", "presence"] as const;

export function setupStepLabel(step: (typeof SETUP_STEPS)[number]): string {
  return `初回セットアップ · ${SETUP_STEPS.indexOf(step) + 1} / ${SETUP_STEPS.length}`;
}

/** これより小さい窓では、案内を最初から折りたたむ。どの大きさでも、左サイドバーの右・下端に置く。 */
export const COMPACT_GUIDE = { width: 1100, height: 760 };

export function compactGuide(width: number, height: number): boolean {
  return width < COMPACT_GUIDE.width || height < COMPACT_GUIDE.height;
}

function runningJob(progress: ProgressBoard, job: string) {
  const entry = progress.jobs[job];
  return entry && !isProgressEnded(entry) ? entry : null;
}

/** 分類（「未分類を分類」や、全取得のあとの自動の分類）が走っているか。 */
export function classificationRunning(progress: ProgressBoard): boolean {
  return runningJob(progress, "extract") !== null;
}

export interface RectLike {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * 案内の右端を、右パネル（右の列でも、浮かせた小窓でも）の手前で止めるための幅。
 * 右パネルのページはネイティブの WebView で、案内が重なるとページを隠す（BROWSER_PANEL_COVER_SELECTORS）。
 * ログインの手順で右パネルのページを見えなくしないよう、案内の高さに掛かるパネルだけを避ける。
 */
export function guideRightClearance(viewWidth: number, panels: readonly RectLike[], guideTop: number): number {
  let clear = 0;
  for (const rect of panels) {
    if (!(rect.width > 0 && rect.height > 0)) continue;
    if (rect.top + rect.height <= guideTop) continue;
    clear = Math.max(clear, viewWidth - rect.left);
  }
  return Math.max(0, Math.ceil(clear));
}

export interface SetupLanding {
  hash: string;
  /** 移った画面の中でスクロールする節の id。 */
  section?: string;
  message: string;
}

/**
 * セットアップを終えたあとに開く画面。商品が無ければ読み込みへ、未分類が残っていれば分類へ、
 * そうでなければ探す画面（持っているアバターがあればアバター、無ければアセット）へ移る。
 */
export function setupLanding(stats: LibraryStats | null, classifying: boolean): SetupLanding {
  if (!stats || stats.products === 0) {
    return {
      hash: "#/import",
      section: "import-booth",
      message: "セットアップを終えました。商品情報は「1. BOOTH から読み込む」から読み込めます",
    };
  }
  if (stats.unextracted > 0 && !classifying) {
    return {
      hash: "#/import",
      section: "import-classification",
      message: `セットアップを終えました。未分類の商品が ${stats.unextracted} 件あります。「未分類を分類」を押すと、一覧に出ます`,
    };
  }
  const hash = stats.ownedAvatars > 0 ? "#/" : "#/assets";
  if (classifying) return { hash, message: "セットアップを終えました。分類を続けています。終わった商品から一覧に出ます" };
  return stats.ownedAvatars > 0
    ? { hash, message: "セットアップを終えました。アバターを選ぶと、対応する衣装や髪を探せます" }
    : { hash, message: "セットアップを終えました。アセットをジャンルごとに探せます" };
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

/** 案内の置き場所を、右パネルの位置と案内自身の高さに合わせて保つ。浮かせた小窓は動くので、短い間隔で測り直す。 */
function useGuidePlacement(ref: RefObject<HTMLDivElement | null>, active: boolean): number {
  const [right, setRight] = useState(0);
  useEffect(() => {
    if (!active) return;
    const root = document.documentElement;
    const measure = () => {
      const guide = ref.current?.getBoundingClientRect();
      const panels = Array.from(document.querySelectorAll<HTMLElement>(".right-panel"), (panel) => panel.getBoundingClientRect());
      const next = guideRightClearance(window.innerWidth, panels, guide ? guide.top : window.innerHeight - 160);
      setRight((current) => (current === next ? current : next));
      // 中身の下に案内の高さぶんの余白を取り、最後の行まで案内の上へ出せるようにする（styles.css）。
      if (guide) root.style.setProperty("--setup-guide-height", `${Math.ceil(guide.height)}px`);
    };
    root.classList.add("has-setup-guide");
    measure();
    const timer = window.setInterval(measure, 500);
    window.addEventListener("resize", measure);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("resize", measure);
      root.classList.remove("has-setup-guide");
      root.style.removeProperty("--setup-guide-height");
    };
  }, [active, ref]);
  return right;
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
  const { revision, notify, fail } = useShell();
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
  const [storageFolder, setStorageFolder] = useState("");
  const [routeChoice, setRouteChoice] = useState<"unset" | "on" | "off">("unset");
  const [storageBusy, setStorageBusy] = useState(false);
  const guideRef = useRef<HTMLDivElement>(null);
  const guiding = step === "library" || step === "classify" || step === "manual" || step === "files";
  const guideRight = useGuidePlacement(guideRef, guiding);

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

  // ファイルの置き場所の手順に入ったら、保存済みのフォルダと自動整理を読む。
  useEffect(() => {
    if (step !== "storage") return;
    let alive = true;
    void api.settings().then(
      (value) => {
        if (!alive) return;
        setStorageFolder(value.libraryFolder);
        setRouteChoice(value.routeDownloads ? "on" : "unset");
      },
      (err: unknown) => {
        if (alive) setMessage(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [step]);

  async function pickStorage() {
    setMessage("");
    try {
      // 見本（ブラウザ確認用）ではフォルダを選べないので、見本のフォルダを使う。
      const folder = isTauri() ? await chooseFolder() : "C:\\Assets";
      if (folder) setStorageFolder(folder);
    } catch (err) {
      setMessage(errorText(err));
    }
  }

  async function finishStorage() {
    if (routeChoice === "unset" || storageBusy) return;
    setMessage("");
    setStorageBusy(true);
    try {
      const saved = await api.settings();
      const folder = storageFolder.trim();
      const patch: Partial<Settings> = {};
      if (folder && folder !== saved.libraryFolder) patch.libraryFolder = folder;
      const route = routeChoice === "on" && folder !== "";
      if (route !== saved.routeDownloads) patch.routeDownloads = route;
      if (Object.keys(patch).length > 0) await api.updateSettings(patch);
      // 設定画面でフォルダを選んだときと同じく、選んだフォルダを調べて商品と結び付ける。
      if (patch.libraryFolder) {
        void api.scanLibrary(patch.libraryFolder).then(
          (report) => notify(`${report.indexed} 件を調べ、${report.linked} 件を商品と結び付けました`),
          (err: unknown) => fail(errorText(err)),
        );
      }
      setStep("presence");
    } catch (err) {
      setMessage(errorText(err));
    } finally {
      setStorageBusy(false);
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

  function guide(next: Guided) {
    setMessage("");
    setStep(next);
    window.location.hash = next === "manual" ? "#/classification-log" : "#/import";
    if (next !== "manual") {
      scrollToAnchor(next === "library" ? "import-booth" : next === "classify" ? "import-classification" : "import-existing-files");
    }
  }

  const presenceReady = backgroundChoice !== "unset" && loginChoice !== "unset";

  if (step === "library" || step === "classify" || step === "manual" || step === "files") {
    const status = guideStatus(step, stats, progress, integration);
    const guides: Record<Guided, { title: string; body: ReactNode; back: () => void; extra?: ReactNode; next: () => void }> = {
      library: {
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
        next: () => setStep("storage"),
      },
    };
    const current = guides[step];
    return (
      <div
        ref={guideRef}
        className={`setup-guide${compact ? " compact" : ""}${collapsed ? " collapsed" : ""}`}
        style={{ "--guide-left": `${railInset}px`, "--guide-right": `${guideRight}px` } as CSSProperties}
        role="region"
        aria-label="使い方ガイド"
      >
        <div className="setup-guide-card panel">
          <div className="setup-guide-head">
            <div className="setup-guide-heading">
              <p className="meta">{setupStepLabel(step)}</p>
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
        <p className="meta">{step === "intro" ? "初回セットアップ" : setupStepLabel(step)}</p>
        <h1>
          {step === "intro"
            ? "VRCLibMan へようこそ"
            : step === "extension"
              ? "BOOTH の読み込み方"
              : step === "storage"
                ? "ダウンロードしたファイルの置き場所"
                : "終了と起動の設定"}
        </h1>
        {step === "intro" && <>
          <p>BOOTH の商品と手元のファイルをまとめて管理し、持っているアバターや対応する衣装を探せるアプリです。</p>
          <p>商品は BOOTH のカテゴリとタグ、タイトルと説明文、ダウンロードファイルの名前から分類します。追加のソフトは要りません。分類結果は手動で修正できます。</p>
          <p>読み込み方を選んだあと、ライブラリの取得、分類、ファイルの登録を順に進めます（全 {SETUP_STEPS.length} 画面）。途中で「あとで行う」を選んでも、あとから同じ操作ができます。</p>
          <div className="row-actions setup-actions">
            <button className="button primary" type="button" onClick={() => setStep("extension")}>はじめる</button>
            <button className="button secondary" type="button" onClick={() => void openExternal(HOW_TO_USE_URL).catch((err: unknown) => setMessage(errorText(err)))}>使い方ガイドを開く</button>
          </div>
          {message && <p className="note-strong" role="alert">{message}</p>}
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
        {step === "storage" && (
          <>
            <p>BOOTH からダウンロードしたファイルを、商品ごとのフォルダにまとめて置く場所（ライブラリのフォルダ）を選びます。</p>
            {browserEnabled && (
              <p className="note-strong">商品カードの「ダウンロード」でファイルを取得するには、フォルダを選んで「ライブラリへ自動整理する」にしてください。</p>
            )}
            <p>ライブラリのフォルダ</p>
            <div className="inline-control setup-folder">
              <input aria-label="ライブラリのフォルダ" value={storageFolder} placeholder="未設定" readOnly />
              <button className="button secondary" type="button" disabled={storageBusy} onClick={() => void pickStorage()}>
                フォルダを選ぶ
              </button>
            </div>
            <p className="meta">ホームフォルダ全体やドライブの直下ではなく、アセット用のフォルダを選んでください。すでにファイルを置いているフォルダでもかまいません。</p>
            <p>ダウンロードしたファイル</p>
            <div className="row-actions">
              <button
                className={routeChoice === "on" ? "button primary" : "button secondary"}
                type="button"
                aria-pressed={routeChoice === "on"}
                disabled={storageFolder.trim() === "" || storageBusy}
                onClick={() => setRouteChoice("on")}
              >
                ライブラリへ自動整理する
              </button>
              <button
                className={routeChoice === "off" ? "button primary" : "button secondary"}
                type="button"
                aria-pressed={routeChoice === "off"}
                disabled={storageBusy}
                onClick={() => setRouteChoice("off")}
              >
                ダウンロード先に置いたままにする
              </button>
            </div>
            <p className="meta">
              自動整理すると、アプリや拡張で自分でダウンロードしたファイルを、ダウンロードフォルダからこのフォルダの商品 ID 別のフォルダへ移します。ほかのファイルは移動しません。
              {browserEnabled ? "置いたままにすると、商品カードの「ダウンロード」は使えません。" : ""}
            </p>
            <p className="meta">設定の「ライブラリとダウンロード」で、あとから変更できます。「あとで設定する」を選ぶと、何も変えずに次へ進みます。</p>
            {message && <p className="note-strong" role="alert">{message}</p>}
            <div className="row-actions setup-actions">
              <button className="button" type="button" onClick={() => guide("files")}>戻る</button>
              <button className="button" type="button" disabled={storageBusy} onClick={() => { setMessage(""); setStep("presence"); }}>
                あとで設定する
              </button>
              <button className="button primary" type="button" disabled={routeChoice === "unset" || storageBusy} onClick={() => void finishStorage()}>
                次へ
              </button>
            </div>
          </>
        )}
        {step === "presence" && (
          <>
            <p>最後に、ウィンドウを閉じたときの動作と自動起動を選びます。使い方ガイドとセットアップのやり直しは、あとから「設定 → アプリ情報とデータ → 使い方」で開けます。</p>
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
              <button className="button" type="button" onClick={() => setStep("storage")}>戻る</button>
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
