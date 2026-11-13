import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { api, openExternal } from "../api";
import { useModalFocus } from "../keyboard";
import type {
  BoothImportMode,
  ExtensionLink,
  IntegrationStatus,
} from "../types";
import { errorText } from "../ui";

export const EXTENSION_RELEASES_URL =
  "https://github.com/KeisXR/VRCLibMan/releases";
export const LIBRARY_URL = "https://accounts.booth.pm/library";
/**
 * 内蔵ブラウザを有効にする前の確認。1 段落の OS のダイアログでは読まれないので、項目ごとに分けて出す。
 * 「おすすめ」と注意が同時に出て矛盾して見えないよう、何を保存し、いつ使い、何をしないかを並べる。
 */
export const BUILTIN_BROWSER_CONFIRM: ReadonlyArray<{ title: string; items: readonly string[] }> = [
  {
    title: "保存するもの",
    items: ["内蔵ブラウザで BOOTH / pixiv にログインした状態を、この PC のアプリ専用の場所に保存します。パスワードはアプリでは扱いません。"],
  },
  {
    title: "使う場面",
    items: [
      "「ライブラリ全取得」を押したときだけ、ライブラリのページを 1 ページずつ読みます。",
      "ファイルの「ダウンロード」を押したときだけ、その 1 件の BOOTH への要求にログイン状態（Cookie）を使います。開発者のサーバーや別のサイトには送りません。",
    ],
  },
  {
    title: "しないこと",
    items: ["購入、一括のダウンロード、年齢確認の自動通過はしません。"],
  },
  {
    title: "注意",
    items: ["BOOTH の非公式アプリです。ログインした状態で読み込むので、拡張機能より、アカウントが制限・停止されるリスクが高くなることがあります。"],
  },
];

/** 内蔵ブラウザを有効にする前の確認。アプリの中のダイアログで、項目を分けて出す。 */
function BuiltinBrowserConfirm({ onAnswer }: { onAnswer: (ok: boolean) => void }) {
  const box = useRef<HTMLElement>(null);
  useModalFocus(box);
  useEffect(() => {
    box.current?.querySelector<HTMLButtonElement>(".button.primary")?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onAnswer(false);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onAnswer]);
  return createPortal(
    <div className="preview-overlay unity-open-overlay">
      <section ref={box} className="panel unity-open-dialog builtin-browser-confirm" role="dialog" aria-modal="true" aria-labelledby="builtin-browser-confirm-title" tabIndex={-1}>
        <h2 id="builtin-browser-confirm-title">内蔵ブラウザを使う前に</h2>
        {BUILTIN_BROWSER_CONFIRM.map((group) => (
          <div key={group.title}>
            <h3>{group.title}</h3>
            <ul>{group.items.map((item) => <li key={item}>{item}</li>)}</ul>
          </div>
        ))}
        <p className="meta">あとから「設定 → BOOTH の読み込み方」で変えられます。ログイン状態は「内蔵ブラウザの閲覧データを削除」で消せます。</p>
        <div className="row-actions">
          <button className="button primary" type="button" onClick={() => onAnswer(true)}>内蔵ブラウザを使う</button>
          <button className="button" type="button" onClick={() => onAnswer(false)}>キャンセル</button>
        </div>
      </section>
    </div>,
    document.body,
  );
}

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
  const id = useId();
  const [state, setState] = useState<IntegrationStatus | null>(null);
  const [link, setLink] = useState<ExtensionLink | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const [chosen, setChosen] = useState(!requireChoice);
  // 内蔵ブラウザを有効にする前の確認の答えを待つ。
  const [confirming, setConfirming] = useState<{ resolve: (ok: boolean) => void } | null>(null);
  const asking = useRef(false);
  const askBuiltinBrowser = async () => {
    asking.current = true;
    try {
      return await new Promise<boolean>((resolve) => setConfirming({ resolve }));
    } finally {
      asking.current = false;
    }
  };

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
    if (!state || busy || asking.current) return;
    if (mode === state.mode) {
      // 保存値と同じでも、初回セットアップでは本人が選んだことを記録して進めるようにする。
      if (!chosen) {
        setChosen(true);
        onChange?.(mode);
        onReady?.(true);
      }
      return;
    }
    if (state.mode === "extension" && mode !== "extension" && !(await askBuiltinBrowser())) return;
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
      {confirming && (
        <BuiltinBrowserConfirm
          onAnswer={(ok) => {
            confirming.resolve(ok);
            setConfirming(null);
          }}
        />
      )}
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
              "browser",
              "アプリの内蔵ブラウザ",
              "アプリの右パネルで BOOTH にログインします。拡張は不要で、ライブラリの全取得、購入履歴、ファイルのダウンロードもアプリの中でできます。ログイン状態はこの PC のアプリ専用の場所に保存します。",
            ],
            [
              "extension",
              "Chrome / Edge の拡張機能",
              "普段のブラウザで BOOTH を開き、拡張のボタンでアプリへ送ります。アプリにログイン状態を置かず、クッキーもアプリへ送りません。拡張の導入とペアリングが必要です。",
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
