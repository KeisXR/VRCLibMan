import { useEffect, useState } from "react";
import { api } from "../api";
import type { CloudTestResult, Settings } from "../types";
import { errorText } from "../ui";
import { SettingRow } from "./SettingRow";

/** 分類方法が「クラウド(APIキー)」のときの設定行。キーは画面に読み戻さない。 */
export function CloudClassifierSettings({
  settings,
  onChange,
  onBlur,
  beforeTest,
  onNotify,
  onFail,
}: {
  settings: Settings;
  onChange: (patch: Partial<Settings>, debounceMs?: number) => void;
  onBlur: () => void;
  beforeTest: () => Promise<unknown>;
  onNotify: (message: string) => void;
  onFail: (message: string) => void;
}) {
  const [keySaved, setKeySaved] = useState<boolean | null>(null);
  const [draft, setDraft] = useState("");
  const [pending, setPending] = useState(false);
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<CloudTestResult | null>(null);

  useEffect(() => {
    let cancelled = false;
    api.cloudKeyStatus().then(
      (saved) => { if (!cancelled) setKeySaved(saved); },
      () => { if (!cancelled) setKeySaved(null); },
    );
    return () => { cancelled = true; };
  }, []);

  async function saveKey() {
    if (!draft.trim()) return;
    setPending(true);
    try {
      setKeySaved(await api.saveCloudApiKey(draft));
      setDraft("");
      setResult(null);
      onNotify("APIキーを OS の資格情報に保存しました");
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setPending(false);
    }
  }

  async function deleteKey() {
    if (!window.confirm("保存した APIキーを OS の資格情報から削除します。")) return;
    setPending(true);
    try {
      setKeySaved(await api.deleteCloudApiKey());
      setResult(null);
      onNotify("APIキーを削除しました");
    } catch (err) {
      onFail(errorText(err));
    } finally {
      setPending(false);
    }
  }

  async function test() {
    setTesting(true);
    setResult(null);
    try {
      await beforeTest();
      setResult(await api.testCloudClassifier());
    } catch (err) {
      setResult({ ok: false, kind: "失敗", message: errorText(err), modelListed: null });
    } finally {
      setTesting(false);
    }
  }

  const base = settings.cloudBaseUrl.trim().replace(/\/+$/, "") || "（未設定）";
  const keyLabel = keySaved == null ? "保存状態：未確認" : keySaved ? "保存済み" : "未保存";

  return (
    <>
      <SettingRow label="接続先" help="OpenAI 互換の API の URL です。例：https://api.openai.com/v1、https://api.x.ai/v1。https だけを使えます。">
        <input value={settings.cloudBaseUrl} placeholder="https://api.openai.com/v1" spellCheck={false} onChange={(e) => onChange({ cloudBaseUrl: e.target.value }, 300)} onBlur={onBlur} />
      </SettingRow>
      <SettingRow label="モデル名" help="接続先で使えるモデル名をそのまま入力します。">
        <input value={settings.cloudModel} placeholder="例：gpt-4.1-mini" spellCheck={false} onChange={(e) => onChange({ cloudModel: e.target.value }, 300)} onBlur={onBlur} />
      </SettingRow>
      <SettingRow label="APIキー" help="OS の資格情報（Windows の資格情報マネージャー、macOS のキーチェーン）に保存します。保存後は画面に表示せず、設定・ログ・書き出しにも含めません。">
        <div className="inline-control cloud-key-control">
          <input type="password" value={draft} autoComplete="off" spellCheck={false} placeholder={keySaved ? "新しいキーで置き換える" : "APIキーを入力"} aria-label="APIキー" onChange={(e) => setDraft(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") void saveKey(); }} />
          <button className="button" type="button" disabled={pending || !draft.trim()} onClick={() => void saveKey()}>保存</button>
        </div>
        <div className="inline-control cloud-key-status">
          <span className="meta" role="status">{keyLabel}</span>
          {keySaved && <button className="button danger" type="button" disabled={pending} onClick={() => void deleteKey()}>削除</button>}
        </div>
      </SettingRow>
      <SettingRow label="接続テスト" help="保存したキーで接続先のモデル一覧を取得します。商品の情報は送りません。">
        <div className="inline-control">
          <button className="button" type="button" disabled={testing} onClick={() => void test()}>{testing ? "確認中…" : "接続テスト"}</button>
          <span className={result && !result.ok ? "meta warn-text" : "meta"} role="status">{result ? (result.ok ? result.message : `${result.kind}：${result.message}`) : "未確認"}</span>
        </div>
      </SettingRow>
      <details className="settings-details cloud-privacy" open>
        <summary>クラウドに送る内容</summary>
        <p>BOOTH のカテゴリとタグだけで決まらない商品について、分類の指示文（アプリ内の固定文）と、商品のタイトル・BOOTH カテゴリ・タグ・説明文（先頭から最大約4,000文字）を {base}/chat/completions に送ります。</p>
        <p>APIキーは送信時の認証にだけ使います。購入履歴、ファイル、フォルダのパス、ほかの設定は送りません。送った内容の扱いと利用料金は、接続先の規約に従います。</p>
        <p>年齢制限（R-18）の商品はクラウドに送りません。この PC の Ollama か Apple のオンデバイスモデルが使えるときはそちらで分類し、使えないときは未分類のまま残します。</p>
        <p>同時に送るのは最大2件です。回数制限（429）のときは待ってから最大3回やり直します。</p>
      </details>
    </>
  );
}
