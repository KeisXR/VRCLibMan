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

export function ExtensionInstructions() {
  return (
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
}

export function BoothImportSettings({
  onChange,
  onFail,
  onReady,
  navigateBrowser,
}: {
  onChange?: (mode: BoothImportMode) => void;
  onFail: (message: string) => void;
  onReady?: (ready: boolean) => void;
  navigateBrowser?: () => void;
}) {
  const id = useId();
  const [state, setState] = useState<IntegrationStatus | null>(null);
  const [link, setLink] = useState<ExtensionLink | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  async function load() {
    const next = await api.integrationStatus();
    setState(next);
    if (next.mode !== "browser") setLink(await api.extensionLink());
    onReady?.(true);
  }
  useEffect(() => {
    let alive = true;
    void api
      .integrationStatus()
      .then(async (next) => {
        if (!alive) return;
        setState(next);
        onReady?.(true);
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

  async function choose(mode: BoothImportMode) {
    if (!state || mode === state.mode || busy) return;
    if (
      state.mode === "extension" &&
      mode !== "extension" &&
      !window.confirm(BUILTIN_BROWSER_CONFIRM)
    )
      return;
    setBusy(true);
    onReady?.(false);
    try {
      await api.setBoothImportMode(mode);
      setState({ ...state, mode });
      setCopied(false);
      onChange?.(mode);
      if (mode === "extension") await api.closeBoothBrowser();
      if (mode !== "browser") setLink(await api.extensionLink());
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setBusy(false);
      onReady?.(true);
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
              "アプリの別ウィンドウで BOOTH にログインします。拡張は不要で、ログイン状態はアプリ専用の保存場所に残ります。",
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
              checked={state?.mode === mode}
              onChange={() => void choose(mode)}
            />
            <span>
              <strong>{title}</strong>
              <span className="meta">{help}</span>
            </span>
          </label>
        ))}
      </fieldset>
      <p className="meta">
        選択は自動保存されます。あとから変更できます。
      </p>
      {state && state.mode !== "browser" && (
        <div className="import-guidance">
          <h3>拡張機能をつなぐ</h3>
          <ExtensionInstructions />
          {link && (
            <>
              <label className="field">
                <span>ペアリングコード</span>
                <input value={link.token} readOnly />
              </label>
              <p className="meta">
                {link.listening
                  ? "拡張機能からの受信を待っています"
                  : link.error || "受信を開始できませんでした"}
              </p>
              <div className="row-actions">
                <button
                  className="button"
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
                  className="button"
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
      {state && state.mode !== "extension" && (
        <div className="import-guidance">
          <h3>内蔵ブラウザで読み込む</h3>
          <p>
            ブラウザ画面から別ウィンドウを開き、BOOTH にログインして商品情報を読み込みます。
          </p>
          <a
            className="button"
            href="#/browser"
            onClick={() => navigateBrowser?.()}
          >
            ブラウザ画面へ
          </a>
        </div>
      )}
    </div>
  );
}
