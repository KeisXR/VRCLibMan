import { useId } from "react";
import { MODEL_CHOICES, modelInstalled } from "../models";

export function ModelPicker({
  value,
  models,
  checked,
  disabled,
  onChange,
}: {
  value: string;
  models: string[];
  checked: boolean;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const id = useId();
  const custom = !MODEL_CHOICES.some((choice) => choice.name === value);
  return (
    <fieldset className="model-picker" disabled={disabled}>
      <legend>分類用モデル</legend>
      {MODEL_CHOICES.map((choice) => (
        <label className="choice-row" key={choice.name}>
          <input
            type="radio"
            name={id}
            checked={value === choice.name}
            onChange={() => onChange(choice.name)}
          />
          <span>
            <strong>
              {choice.name} ・{choice.label}
            </strong>
            <span className="meta">
              容量 {choice.size} ／ メモリ {choice.memory}（ともに目安）
            </span>
            <span className="meta">
              {checked
                ? modelInstalled(models, choice.name)
                  ? "導入済み"
                  : "未導入"
                : "未確認"}
            </span>
          </span>
        </label>
      ))}
      <label className="choice-row">
        <input
          type="radio"
          name={id}
          checked={custom}
          onChange={() => onChange("")}
        />
        <span>
          <strong>その他のモデル</strong>
          <span className="meta">
            導入済みから選ぶか、モデル名を入力します。
          </span>
        </span>
      </label>
      {custom && (
        <div className="custom-model">
          <label className="field">
            <span>導入済みモデル</span>
            <select
              value={models.includes(value) ? value : ""}
              onChange={(e) => {
                if (e.target.value) onChange(e.target.value);
              }}
            >
              <option value="">導入済みから選ぶ</option>
              {models.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>モデル名</span>
            <input
              value={value}
              placeholder="例: qwen3:4b"
              onChange={(e) => onChange(e.target.value)}
            />
          </label>
        </div>
      )}
      <p className="meta">
        選ぶだけではダウンロードしません。必要なメモリはほかのアプリやモデルの量子化によって変わります。既存の設定は自動では切り替えません。
      </p>
    </fieldset>
  );
}
