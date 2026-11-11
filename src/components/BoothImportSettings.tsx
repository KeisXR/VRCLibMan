import { useEffect, useId, useState } from "react";
import { api, openExternal } from "../api";
import type {
  BoothImportMode,
  ExtensionLink,
  IntegrationStatus,
} from "../types";
import { errorText } from "../ui";

export const EXTENSION_RELEASES_URL =
  "https://github.com/KeisXR/VRCLibMan/releases";
export const LIBRARY_URL = "https://accounts.booth.pm/library";
export const BUILTIN_BROWSER_CONFIRM =
  "内蔵ブラウザで BOOTH / pixiv にログインし、ログイン状態をアプリ専用の保存場所に保持します。ファイルの「ダウンロード」を押すと、その 1 件の BOOTH 向け要求に BOOTH のクッキーを使います。クッキーを開発者のサーバーや別ドメインには送りません。拡張機能より、アカウントが制限・停止されるリスクが高くなることがあります。ライブラリのページ巡回は「ライブラリ全取得」を押したときだけ行い、年齢確認の自動通過や一括ダウンロードはしません。有効にしますか？";

export function ExtensionInstructions({ collapsible = false }: { collapsible?: boolean }) {
  const steps = (
    <div className="extension-instructions">
      <ol className="setup-steps">
        <li>
          GitHub Releases で拡張機能用の zip
          をダウンロードし、残しておける場所に展開します。インストーラだけでは拡張機能は入りません。
        </li>
        <li>
          Chrome の chrome://extensions または Edge の edge://extensions
          を開き、デベロッパーモードをオンにします。「パッケージ化されていない拡張機能を読み込む」で、展開した
          manifest.json のあるフォルダを選びます。
        </li>
        <li>
          アプリのペアリングコードを拡張のポップアップに貼り、保存します。
        </li>
        <li>
          「ライブラリを開く」で普段のブラウザの BOOTH
          にログインし、拡張の「ライブラリをすべて送る」か「新しい購入だけ送る」を押します。
        </li>
      </ol>
      <button
        className="button"
        type="button"
        onClick={() => void openExternal(EXTENSION_RELEASES_URL)}
      >
        拡張機能の入手先
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
 * `requireChoice` は初回セットアップ用。保存値の既定（拡張機能）を選んだ状態で見せず、本人が選ぶまで
 * どれも選ばれていない形にする。拡張以外が保存されていれば、それは本人が選んだ値なので選択済みで見せる。
 */
export function BoothImportSettings({
  onChange,
  onFail,
  onReady,
  navigateBrowser,
  requireChoice = false,
}: {
  onChange?: (mode: BoothImportMode) => void;
  onFail: (message: string) => void;
  /** 選択が済み、保存中でないとき true。 */
  onReady?: (ready: boolean) => void;
  navigateBrowser?: () => void;
  requireChoice?: boolean;
}) {
  const id = useId();
  const [state, setState] = useState<IntegrationStatus | null>(null);
  const [link, setLink] = useState<ExtensionLink | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [chosen, setChosen] = useState(!requireChoice);

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

  async function choose(mode: BoothImportMode) {
    if (!state || busy) return;
    if (mode === state.mode) {
      // 保存値と同じでも、初回セットアップでは本人が選んだことを記録して進めるようにする。
      if (!chosen) {
        setChosen(true);
        onChange?.(mode);
        onReady?.(true);
      }
      return;
    }
    if (
      state.mode === "extension" &&
      mode !== "extension" &&
      !window.confirm(BUILTIN_BROWSER_CONFIRM)
    )
      return;
    setBusy(true);
    onReady?.(false);
    let saved = false;
    try {
      await api.setBoothImportMode(mode);
      saved = true;
      setState({ ...state, mode });
      setChosen(true);
      setCopied(false);
      onChange?.(mode);
      if (mode === "extension") await api.closeBoothBrowser();
      if (mode !== "browser") setLink(await api.extensionLink());
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
      onReady?.(chosen || saved);
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

  return (
    <div className="booth-import-settings">
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
      <fieldset className="import-methods" aria-label="BOOTH の購入情報の読み込み方" disabled={!state || busy}>
        {(
          [
            [
              "extension",
              "Chrome / Edge の拡張機能",
              "普段のブラウザで BOOTH を開き、拡張のボタンでアプリへ送ります。ブラウザのログインを使い、クッキーはアプリへ送りません。",
            ],
            [
              "browser",
              "アプリの内蔵ブラウザ",
              "アプリの右パネルで BOOTH にログインします。拡張は不要で、ライブラリの全取得とファイルのダウンロードもアプリの中でできます。ログイン状態はアプリ専用に保存されます。",
            ],
            [
              "both",
              "両方使う",
              "普段のブラウザの拡張と、アプリの内蔵ブラウザを使い分けます。それぞれのログイン状態は別に管理されます。",
            ],
          ] as const
        ).map(([mode, title, help]) => (
          <label key={mode} className="choice-row">
            <input
              type="radio"
              name={id}
              value={mode}
              checked={chosen && state?.mode === mode}
              onChange={() => void choose(mode)}
            />
            <span>
              <strong>
                {title}
                {mode === "browser" && <span className="badge recommend">おすすめ</span>}
              </strong>
              <span className="meta">{help}</span>
            </span>
          </label>
        ))}
      </fieldset>
      {requireChoice && !chosen ? (
        <p className="note-strong" role="status">
          どれかを選ぶと次へ進めます。おすすめは内蔵ブラウザです。ログイン状態をアプリに保存するため、選ぶと確認が出ます。拡張機能はアプリにログイン情報を置かず、普段のブラウザのまま使えます。
        </p>
      ) : (
        <p className="meta">
          選択は自動保存されます。あとから変更できます。
        </p>
      )}
      {state && chosen && state.mode !== "browser" && (
        <div className="import-guidance">
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
                  disabled={busy}
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
                  disabled={busy}
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
