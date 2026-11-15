import { useEffect, useState } from "react";
import { api, openExternal, revealInFolder } from "../api";
import { importMethod } from "../importMethods";
import type {
  BoothImportMode,
  ExtensionLink,
  ExtensionPackage,
  IntegrationStatus,
} from "../types";
import { errorText } from "../ui";
import { ImportMethodWizard } from "./ImportMethodWizard";

export const EXTENSION_RELEASES_URL =
  "https://github.com/KeisXR/VRCLibMan/releases";
export const LIBRARY_URL = "https://accounts.booth.pm/library";

/** 拡張機能を読み込ませるフォルダの状態と、ブラウザへの読み込み方。 */
export function extensionFolderText(pkg: ExtensionPackage | null): string {
  if (!pkg) return "確認しています";
  if (!pkg.bundledVersion) return "このアプリには拡張機能が入っていません。Releases の zip を使ってください。";
  if (!pkg.installedVersion) return "まだ用意していません。「拡張機能を用意する」を押すと、このフォルダに作ります。";
  if (pkg.upToDate) return `v${pkg.installedVersion} を用意しています。アプリを更新すると、このフォルダも同じ版に置き換わります。`;
  return `今は v${pkg.installedVersion} です。アプリには v${pkg.bundledVersion} があります。「用意し直す」で置き換えます。`;
}

export function ExtensionInstructions({ collapsible = false }: { collapsible?: boolean }) {
  const [pkg, setPkg] = useState<ExtensionPackage | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    let alive = true;
    void api.extensionPackage().then(
      (value) => { if (alive) setPkg(value); },
      (err: unknown) => { if (alive) setError(errorText(err)); },
    );
    return () => {
      alive = false;
    };
  }, []);
  async function prepare() {
    setBusy(true);
    try {
      setPkg(await api.installExtensionPackage());
      setError("");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  }
  const installed = Boolean(pkg?.installedVersion);
  const steps = (
    <div className="extension-instructions">
      <div className="extension-folder">
        <span className="meta">拡張機能のフォルダ</span>
        <code className="extension-path">{pkg?.path ?? "確認しています"}</code>
        <p className="meta" role="status">{extensionFolderText(pkg)}</p>
        <div className="row-actions">
          {installed && pkg && (
            <>
              <button
                className="button secondary"
                type="button"
                onClick={() =>
                  void navigator.clipboard.writeText(pkg.path).then(
                    () => setCopied(true),
                    (err: unknown) => setError(errorText(err)),
                  )
                }
              >
                {copied ? "コピーしました" : "パスをコピー"}
              </button>
              <button className="button" type="button" onClick={() => void revealInFolder(pkg.path).catch((err: unknown) => setError(errorText(err)))}>
                フォルダを開く
              </button>
            </>
          )}
          {pkg?.bundledVersion && (
            <button className={installed ? "button" : "button secondary"} type="button" disabled={busy} onClick={() => void prepare()}>
              {installed ? "用意し直す" : "拡張機能を用意する"}
            </button>
          )}
        </div>
        {error && <p className="note-strong" role="alert">{error}</p>}
      </div>
      <ol className="setup-steps">
        <li>
          Chrome は chrome://extensions、Edge は edge://extensions を開き、デベロッパーモードをオンにします。
        </li>
        <li>以前に zip から読み込んだ VRCLibMan の拡張機能があれば「削除」します。</li>
        <li>
          「パッケージ化されていない拡張機能を読み込む」を押し、上のフォルダを選びます。フォルダを選ぶ画面のアドレス欄に、コピーしたパスを貼り付けると早く選べます。
        </li>
        <li>拡張機能のアイコンを押し、ペアリングコードを貼って「接続する」を押します。</li>
      </ol>
      <p className="meta">
        以後はアプリを更新すると、このフォルダの拡張機能も同じ版に置き換わり、拡張機能が自分で読み込み直します。フォルダを移動・削除しないでください。
      </p>
      <button
        className="button"
        type="button"
        onClick={() => void openExternal(EXTENSION_RELEASES_URL)}
      >
        zip で入れる（自動では更新されません）
      </button>
    </div>
  );
  if (!collapsible) return steps;
  return (
    <details className="settings-details extension-instructions-details">
      <summary>拡張機能の入れ方（4 手順）</summary>
      {steps}
    </details>
  );
}

function receivedAt(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("ja-JP");
}

/**
 * 受信口の状態と、拡張から実際に受信できたかを分けて出す。過去の受信から今の接続は断定しない。
 */
export function extensionReceiveText(link: ExtensionLink, state: IntegrationStatus | null): { text: string; received: boolean } {
  if (!link.listening) return { text: link.error || "受信を開始できませんでした", received: false };
  if (state?.extensionLastSeenAt) {
    const version = state.extensionVersion ? `、拡張の版 ${state.extensionVersion}` : "";
    return { text: `拡張機能から受信しました（最終: ${receivedAt(state.extensionLastSeenAt)}${version}）`, received: true };
  }
  return {
    text: "受信の準備ができています。まだ拡張機能から届いていません。拡張にコードを貼って保存すると「受信しました」に変わります。",
    received: false,
  };
}

/**
 * 今の読み込み方と「変更」。選ぶのは「変更」で開く ImportMethodWizard で、そこで確定したときだけ保存する。
 * `requireChoice` は初回セットアップ用。保存値の既定（拡張機能）を選んだものとして見せず、本人が選ぶまで
 * 「まだ選んでいません」とする。拡張以外が保存されていれば、それは本人が選んだ値なので選択済みで見せる。
 */
export function BoothImportSettings({
  onChange,
  onFail,
  onReady,
  navigateBrowser,
  requireChoice = false,
  pairingId,
}: {
  onChange?: (mode: BoothImportMode) => void;
  onFail: (message: string) => void;
  /** 選択が済み、保存中でないとき true。 */
  onReady?: (ready: boolean) => void;
  navigateBrowser?: () => void;
  requireChoice?: boolean;
  /** 「拡張機能をつなぐ」の枠の id。設定画面だけが渡す（検索の移動先。セットアップと重ならないように）。 */
  pairingId?: string;
}) {
  const [state, setState] = useState<IntegrationStatus | null>(null);
  const [link, setLink] = useState<ExtensionLink | null>(null);
  const [copied, setCopied] = useState(false);
  const [chosen, setChosen] = useState(!requireChoice);
  const [choosing, setChoosing] = useState(false);

  async function load() {
    const next = await api.integrationStatus();
    setState(next);
    const picked = chosen || next.mode !== "extension";
    setChosen(picked);
    if (next.mode !== "browser") setLink(await api.extensionLink());
    onReady?.(picked);
  }
  useEffect(() => {
    let alive = true;
    void api
      .integrationStatus()
      .then(async (next) => {
        if (!alive) return;
        setState(next);
        const picked = !requireChoice || next.mode !== "extension";
        setChosen(picked);
        onReady?.(picked);
        if (next.mode !== "browser") {
          const value = await api.extensionLink();
          if (alive) setLink(value);
        }
      })
      .catch((err: unknown) => {
        if (alive) onFail(errorText(err));
      });
    return () => {
      alive = false;
    };
  }, []);

  // 拡張をつないでいる途中は、受信できたかを少しずつ確かめる（ローカルの読み取りだけ）。
  const waitingForExtension = Boolean(state && chosen && state.mode !== "browser" && !state.extensionLastSeenAt);
  useEffect(() => {
    if (!waitingForExtension) return;
    let alive = true;
    const timer = window.setInterval(() => {
      void api.integrationStatus().then(
        (next) => { if (alive) setState((current) => (current ? { ...next, mode: current.mode } : next)); },
        () => undefined,
      );
    }, 3000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [waitingForExtension]);

  /** 選ぶ画面で確定したとき。保存に失敗したら投げて、選ぶ画面に理由を出す。 */
  async function apply(mode: BoothImportMode) {
    if (!state) return;
    onReady?.(false);
    try {
      // 初回セットアップで保存値と同じものを選んだときは、保存せず、選んだことだけを記録する。
      if (mode !== state.mode) await api.setBoothImportMode(mode);
    } catch (err) {
      onReady?.(chosen);
      throw err;
    }
    setState({ ...state, mode });
    setChosen(true);
    setCopied(false);
    setChoosing(false);
    onChange?.(mode);
    onReady?.(true);
    try {
      if (mode === "extension") await api.closeBoothBrowser();
      if (mode !== "browser") setLink(await api.extensionLink());
    } catch (err) {
      onFail(errorText(err));
    }
  }

  async function regenerate() {
    if (
      !window.confirm(
        "コードを作り直すと、貼ってあるコードは使えなくなり、結び付いた拡張も外れます。",
      )
    )
      return;
    try {
      setLink(await api.regenerateExtensionToken());
      setCopied(false);
      await load();
    } catch (err) {
      onFail(errorText(err));
    }
  }

  const method = state && chosen ? importMethod(state.mode) : null;
  return (
    <div className="booth-import-settings">
      {choosing && state && (
        <ImportMethodWizard
          current={chosen ? state.mode : null}
          onCancel={() => setChoosing(false)}
          onConfirm={apply}
        />
      )}
      <div className="import-current">
        <div className="import-current-copy">
          <span className="meta">現在の読み込み方</span>
          <strong>{!state ? "確認しています" : method ? method.name : "まだ選んでいません"}</strong>
          <span className="meta">
            {method ? method.summary : "内蔵ブラウザと拡張機能のできることを比べて選びます。"}
          </span>
        </div>
        <button
          className={method ? "button secondary" : "button primary"}
          type="button"
          disabled={!state}
          onClick={() => setChoosing(true)}
        >
          {method ? "変更" : "読み込み方を選ぶ"}
        </button>
      </div>
      {!state && (
        <button
          className="button"
          type="button"
          onClick={() =>
            void load().catch((err: unknown) => onFail(errorText(err)))
          }
        >
          読み込み方を確認
        </button>
      )}
      {requireChoice && state && !chosen && (
        <p className="note-strong" role="status">
          「読み込み方を選ぶ」で比べて選ぶと、次へ進めます。おすすめは内蔵ブラウザです。
        </p>
      )}
      {state && chosen && state.mode !== "browser" && (
        <div className="import-guidance" id={pairingId}>
          <h3>拡張機能をつなぐ</h3>
          <ExtensionInstructions collapsible={requireChoice} />
          {link && (
            <>
              <label className="field">
                <span>ペアリングコード</span>
                <input value={link.token} readOnly />
              </label>
              {(() => {
                const receive = extensionReceiveText(link, state);
                return (
                  <p className="connection-state" role="status">
                    <span className={`state-chip ${receive.received ? "done" : link.listening ? "running" : "todo"}`}>
                      {receive.received ? "受信済み" : link.listening ? "受信待ち" : "受信できません"}
                    </span>
                    <span>{receive.text}</span>
                  </p>
                );
              })()}
              <div className="row-actions">
                <button
                  className="button secondary"
                  type="button"
                  onClick={() =>
                    void navigator.clipboard.writeText(link.token).then(
                      () => setCopied(true),
                      (err: unknown) => onFail(errorText(err)),
                    )
                  }
                >
                  {copied ? "コピーしました" : "コードをコピー"}
                </button>
                <button
                  className="button"
                  type="button"
                  onClick={() => void regenerate()}
                >
                  コードを作り直す
                </button>
                <button
                  className="button secondary"
                  type="button"
                  onClick={() => void openExternal(LIBRARY_URL)}
                >
                  ライブラリを開く
                </button>
              </div>
            </>
          )}
        </div>
      )}
      {state && chosen && state.mode !== "extension" && (
        <div className="import-guidance">
          <h3>内蔵ブラウザで読み込む</h3>
          <p>
            インポート画面の「1. BOOTH から読み込む」で BOOTH を開き、ログインして商品情報を取得します。
          </p>
          <a
            className="button secondary"
            href="#/import"
            onClick={() => navigateBrowser?.()}
          >
            インポート画面へ
          </a>
        </div>
      )}
    </div>
  );
}
