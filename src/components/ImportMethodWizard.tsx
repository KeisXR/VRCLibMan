import { Fragment, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  IMPORT_COMPARISON,
  IMPORT_METHODS,
  importMethod,
  switchNotes,
} from "../importMethods";
import { useModalFocus } from "../keyboard";
import { CheckGlyph, CrossGlyph } from "./glyphs";
import type { BoothImportMode } from "../types";
import { errorText } from "../ui";

/**
 * 読み込み方を選ぶ画面。1 段目で ✓ / × の表を見比べて列の見出しで選び、2 段目でその切り替えで
 * 起きることを確かめてから保存する（`onConfirm`）。保存するのは最後のボタンだけで、キャンセルと Esc は何も変えない。
 * `current` が null なら、まだ選んでいない（初回セットアップ）。どの列も選ばない状態から始める。
 */
export function ImportMethodWizard({
  current,
  onCancel,
  onConfirm,
}: {
  current: BoothImportMode | null;
  onCancel: () => void;
  onConfirm: (mode: BoothImportMode) => Promise<void>;
}) {
  const name = useId();
  const titleId = useId();
  const box = useRef<HTMLElement>(null);
  const [picked, setPicked] = useState<BoothImportMode | null>(current);
  const [step, setStep] = useState<1 | 2>(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useModalFocus(box);

  useEffect(() => {
    const target =
      step === 1
        ? box.current?.querySelector<HTMLInputElement>("input[type=radio]:checked") ??
          box.current?.querySelector<HTMLInputElement>("input[type=radio]")
        : box.current?.querySelector<HTMLButtonElement>(".button.primary");
    target?.focus();
    box.current?.scrollTo?.({ top: 0 });
  }, [step]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busy) return;
      event.preventDefault();
      onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [busy, onCancel]);

  const method = picked ? importMethod(picked) : null;
  const unchanged = picked !== null && picked === current;

  async function confirm() {
    if (!picked || busy) return;
    setBusy(true);
    setError("");
    try {
      await onConfirm(picked);
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  }

  return createPortal(
    <div className="preview-overlay unity-open-overlay">
      <section
        ref={box}
        className="panel import-wizard"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        {step === 1 || !method ? (
          <>
            <p className="meta import-wizard-step">1 / 2 · 比べて選ぶ</p>
            <h2 id={titleId}>読み込み方を選ぶ</h2>
            <p>できること（✓）とできないこと（×）を比べて、表の上の名前で選んでください。</p>
            <div className="import-compare-scroll">
              <table className="import-compare">
                <thead>
                  <tr>
                    <th scope="col">できること</th>
                    {IMPORT_METHODS.map((item) => (
                      <th key={item.mode} scope="col" className={picked === item.mode ? "is-picked" : undefined}>
                        <label className="import-compare-choice">
                          <input
                            type="radio"
                            name={name}
                            value={item.mode}
                            checked={picked === item.mode}
                            onChange={() => setPicked(item.mode)}
                          />
                          <strong>{item.name}</strong>
                          <span className="meta">{item.sub}</span>
                          {item.mode === "browser" && <span className="badge recommend">おすすめ</span>}
                          {item.mode === current && <span className="meta">今の設定</span>}
                        </label>
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {IMPORT_COMPARISON.map((group) => (
                    <Fragment key={group.group}>
                      <tr className="import-compare-group">
                        <th scope="colgroup" colSpan={IMPORT_METHODS.length + 1}>{group.group}</th>
                      </tr>
                      {group.rows.map((row) => (
                        <tr key={row.label} className={row.emphasis ? "is-emphasis" : undefined}>
                          <th scope="row">{row.label}</th>
                          {IMPORT_METHODS.map((item) => {
                            const cell = row[item.mode];
                            return (
                              <td
                                key={item.mode}
                                className={picked === item.mode ? "is-picked" : undefined}
                                onClick={() => setPicked(item.mode)}
                              >
                                <span className={cell.can ? "import-mark yes" : "import-mark no"} role="img" aria-label={cell.can ? "できる" : "できない"}>
                                  {cell.can ? <CheckGlyph /> : <CrossGlyph />}
                                </span>
                                {cell.note && <span className="meta">{cell.note}</span>}
                              </td>
                            );
                          })}
                        </tr>
                      ))}
                    </Fragment>
                  ))}
                </tbody>
              </table>
            </div>
            {/* 選んだものの説明は、表が長くても見えるよう、下端に残る操作の行に入れる。 */}
            <div className="import-wizard-actions">
              <p className="import-wizard-summary" role="status">
                {method ? (
                  <>
                    <strong>{method.name}</strong>：{method.summary}
                    {unchanged && "（今の読み込み方です）"}
                  </>
                ) : (
                  "表の上の名前を選ぶと、次へ進めます。"
                )}
              </p>
              <div className="row-actions">
                <button className="button primary" type="button" disabled={!picked || unchanged} onClick={() => setStep(2)}>
                  次へ
                </button>
                <button className="button" type="button" onClick={onCancel}>
                  キャンセル
                </button>
              </div>
            </div>
          </>
        ) : (
          <>
            <p className="meta import-wizard-step">2 / 2 · 確かめる</p>
            <h2 id={titleId}>{method.action}前に</h2>
            {switchNotes(current, method.mode).map((group) => (
              <div key={group.title} className={group.tone === "warn" ? "import-wizard-notes is-warn" : "import-wizard-notes"}>
                <h3>{group.title}</h3>
                <ul>{group.items.map((item) => <li key={item}>{item}</li>)}</ul>
              </div>
            ))}
            <p className="meta">あとから「設定 → BOOTH の読み込み方」の「変更」で変えられます。</p>
            {error && <p className="note-strong" role="alert">{error}</p>}
            <div className="row-actions import-wizard-actions">
              <button className="button primary" type="button" disabled={busy} onClick={() => void confirm()}>
                {method.action}
              </button>
              <button className="button" type="button" disabled={busy} onClick={() => setStep(1)}>
                戻る
              </button>
              <button className="button" type="button" disabled={busy} onClick={onCancel}>
                キャンセル
              </button>
            </div>
          </>
        )}
      </section>
    </div>,
    document.body,
  );
}
