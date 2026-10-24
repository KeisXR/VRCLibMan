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
  "内蔵ブラウザでは、このアプリの中で BOOTH / pixiv にメールアドレスとパスワードでログインします。ログイン状態はアプリ専用の WebView データストアに保存されます。ファイルの「ダウンロード」を押した場合、アプリは BOOTH 用クッキーを読み出し、その 1 件の BOOTH 向けダウンロード要求に使用します。クッキーは VRCLibMan 独自の外部サーバーやダウンロード先の別ドメインには送りません。拡張機能でページを書き出すより、アカウントへのリスク（制限や停止を含む）が高くなることがあります。年齢確認を自動で進めることはありません。ページを辿るのは、ブラウザ画面の「ライブラリ全取得」を押したときだけです。一括のダウンロードはしません。ご自身の判断で使ってください。有効にしますか？";

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
      <p className="meta">
        配布形式は今後変わる可能性があります。現在は Releases の拡張 zip
        を使ってください。
      </p>
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
      <fieldset className="import-methods" disabled={!state || busy}>
        <legend>BOOTH の購入情報の読み込み方</legend>
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
              "アプリ内の別ウィンドウで BOOTH にログインします。拡張は不要です。メールアドレスとパスワードでのログインを想定し、ログイン状態はアプリ専用の保存場所に残ります。",
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
        選択はその場で保存します。あとから設定で変更できます。ユーザーが開いたページや、ボタンで指定した範囲を読み込みます。無人での巡回や、一括ダウンロードは行いません。
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
                  ? `127.0.0.1:${link.port} で受信待機中`
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
            内蔵ブラウザを有効にしました。ブラウザ画面で別ウィンドウを開き、BOOTH
            にログインしてページを読み込みます。
          </p>
          <p className="meta">
            ログアウトすると専用のログイン状態を消せます。1
            件の「ダウンロード」を押したときだけ、その BOOTH
            向けの要求にクッキーを使います。年齢確認は自動で進めません。
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
