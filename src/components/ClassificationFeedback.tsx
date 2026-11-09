import { useState } from "react";
import {
  copyClassificationFeedback,
  saveClassificationFeedback,
  type ClassificationFeedbackScope,
} from "../api";
import { useShell } from "../App";
import { errorText } from "../ui";
import { SettingRow } from "./SettingRow";

const SCOPES: { value: ClassificationFeedbackScope; label: string }[] = [
  { value: "other", label: "その他" },
  { value: "unclassified", label: "未分類" },
  { value: "both", label: "両方" },
];

export function ClassificationFeedback() {
  const { notify, fail } = useShell();
  const [scope, setScope] = useState<ClassificationFeedbackScope>("both");
  const [busy, setBusy] = useState(false);

  async function run(action: "save" | "copy") {
    setBusy(true);
    try {
      if (action === "copy") {
        await copyClassificationFeedback(scope);
        notify("分類改善用データをクリップボードにコピーしました");
        return;
      }
      const result = await saveClassificationFeedback(scope);
      if (result === "saved") notify("分類改善用データを保存しました");
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <SettingRow label="対象" help="「その他」はカテゴリがその他の商品です。未分類はカテゴリが決まっていない商品です。">
        <fieldset className="scope-choices" aria-label="書き出す対象">
          {SCOPES.map((choice) => (
            <label key={choice.value}>
              <input
                type="radio"
                name="classification-feedback-scope"
                value={choice.value}
                checked={scope === choice.value}
                onChange={() => setScope(choice.value)}
              />
              {choice.label}
            </label>
          ))}
        </fieldset>
      </SettingRow>
      <SettingRow
        label="分類改善用データ"
        help="説明の先頭、手動で移した履歴、いま固定している分類を JSON にします。トークンやファイルの場所は含めません。"
      >
        <div className="inline-control">
          <button className="button" type="button" disabled={busy} onClick={() => void run("save")}>
            分類改善用データを書き出す
          </button>
          <button className="button" type="button" disabled={busy} onClick={() => void run("copy")}>
            クリップボードにコピー
          </button>
        </div>
      </SettingRow>
    </>
  );
}
