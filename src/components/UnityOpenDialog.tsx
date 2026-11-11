import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { api } from "../api";
import { useModalFocus } from "../keyboard";
import { formatBytes } from "../media";
import type { PreparedUnityPackage, UnityLaunchMode, UnityPackageList } from "../types";
import { errorText } from "../ui";
import {
  UNITY_MISSING,
  UNITY_NOT_SET,
  launchedMessage,
  onlyEntry,
  selectedEntries,
  unityEditorReady,
  unityOpenPlan,
} from "../unityOpen";

type DialogState =
  | { step: "choose"; list: UnityPackageList; boothId: string | null; checked: Set<string>; mode: UnityLaunchMode; missing: boolean }
  | { step: "setup"; list: UnityPackageList; boothId: string | null; entries: string[]; missing: boolean }
  | { step: "working"; message: string }
  | { step: "waiting"; prepared: PreparedUnityPackage[]; next: number; mode: UnityLaunchMode };

export interface UnityOpenTarget {
  /** zip か .unitypackage の絶対パス。ライブラリかリモートライブラリの中のもの。 */
  path: string;
  /** 取り出し先 `unity-import/<商品ID>/` に使う。分からなければ null。 */
  boothId?: string | null;
  /** zip の中の 1 件だけを開くとき、その zip 内パス。 */
  entry?: string;
}

export function openUnitySettings() {
  window.location.hash = "#/settings";
  window.setTimeout(() => {
    document.getElementById("settings-unity")?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, 0);
}

/**
 * 「Unityで開く」の流れ。1 件で既定の Unity があればダイアログなしで開き、
 * 2 件以上なら選ぶダイアログ、既定の Unity が無ければ設定への誘導と関連付けで開く案内を出す。
 * 呼び出し側は `dialog` をどこかに置くだけでよい（body へ portal で出す）。
 */
export function useUnityOpen({ notify, fail }: { notify: (message: string) => void; fail: (message: string) => void }) {
  const [dialog, setDialog] = useState<DialogState | null>(null);
  const running = useRef(false);

  const launchFrom = useCallback(
    async (prepared: PreparedUnityPackage[], start: number, mode: UnityLaunchMode) => {
      const total = prepared.length;
      setDialog({
        step: "working",
        message: total > 1 ? `Unity に渡しています（${start + 1}/${total}）` : "Unity に渡しています",
      });
      const result = await api.launchUnityPackages(prepared.slice(start).map((item) => item.path), mode);
      const done = start + result.launched;
      if (done < total) {
        setDialog({ step: "waiting", prepared, next: done, mode });
        return;
      }
      setDialog(null);
      notify(launchedMessage(total, mode));
    },
    [notify],
  );

  const run = useCallback(
    async (list: UnityPackageList, entries: string[], boothId: string | null, mode: UnityLaunchMode) => {
      if (running.current || entries.length === 0) return;
      running.current = true;
      try {
        setDialog({ step: "working", message: list.kind === "zip" ? "zip から取り出しています" : "Unity に渡しています" });
        const prepared = await api.prepareUnityPackages(list.path, entries, boothId);
        await launchFrom(prepared, 0, mode);
      } catch (err) {
        setDialog(null);
        fail(errorText(err));
      } finally {
        running.current = false;
      }
    },
    [fail, launchFrom],
  );

  const resume = useCallback(
    async (prepared: PreparedUnityPackage[], next: number, mode: UnityLaunchMode) => {
      if (running.current) return;
      running.current = true;
      try {
        await launchFrom(prepared, next, mode);
      } catch (err) {
        setDialog(null);
        fail(errorText(err));
      } finally {
        running.current = false;
      }
    },
    [fail, launchFrom],
  );

  const open = useCallback(
    async (target: UnityOpenTarget) => {
      if (running.current) return;
      running.current = true;
      let direct: { list: UnityPackageList; entries: string[]; boothId: string | null } | null = null;
      try {
        const [listed, status] = await Promise.all([
          api.unityPackages(target.path),
          api.unityEditors().catch(() => null),
        ]);
        const list = target.entry != null ? onlyEntry(listed, target.entry) : listed;
        if (!list) throw new Error("選んだ Unity パッケージが zip の中に見つかりません");
        const ready = unityEditorReady(status);
        const missing = Boolean(status?.selectedPath.trim() && status.selectedMissing);
        const boothId = target.boothId ?? null;
        const plan = unityOpenPlan(list, ready);
        if (plan.kind === "none") {
          fail("この zip には Unity パッケージ（.unitypackage）がありません");
        } else if (plan.kind === "direct") {
          direct = { list, entries: plan.entries, boothId };
        } else if (plan.kind === "setup") {
          setDialog({ step: "setup", list, boothId, entries: plan.entries, missing });
        } else {
          setDialog({ step: "choose", list, boothId, checked: new Set(), mode: ready ? "unity" : "system", missing });
        }
      } catch (err) {
        fail(errorText(err));
      } finally {
        running.current = false;
      }
      if (direct) await run(direct.list, direct.entries, direct.boothId, "unity");
    },
    [fail, run],
  );

  const close = useCallback(() => {
    if (!running.current) setDialog(null);
  }, []);

  const element: ReactNode = dialog ? (
    <UnityOpenDialog
      state={dialog}
      onClose={close}
      onToggle={(entry) =>
        setDialog((current) => {
          if (current?.step !== "choose") return current;
          const checked = new Set(current.checked);
          if (checked.has(entry)) checked.delete(entry);
          else checked.add(entry);
          return { ...current, checked };
        })
      }
      onImport={(entries, mode) => {
        if (dialog.step === "choose" || dialog.step === "setup") void run(dialog.list, entries, dialog.boothId, mode);
      }}
      onResume={() => {
        if (dialog.step === "waiting") void resume(dialog.prepared, dialog.next, dialog.mode);
      }}
      onSettings={() => {
        setDialog(null);
        openUnitySettings();
      }}
    />
  ) : null;

  return { open, dialog: element, busy: dialog?.step === "working" };
}

function UnityOpenDialog({
  state,
  onClose,
  onToggle,
  onImport,
  onResume,
  onSettings,
}: {
  state: DialogState;
  onClose: () => void;
  onToggle: (entry: string) => void;
  onImport: (entries: string[], mode: UnityLaunchMode) => void;
  onResume: () => void;
  onSettings: () => void;
}) {
  const panel = useRef<HTMLElement>(null);
  const working = state.step === "working";
  // Tab で背後の画面へ出ない。閉じたら押したボタンへフォーカスを戻す。
  useModalFocus(panel);
  useEffect(() => {
    const target = panel.current?.querySelector<HTMLElement>("input, button:not([disabled])");
    if (target) target.focus();
    else panel.current?.focus();
  }, [state.step]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || working) return;
      event.preventDefault();
      event.stopPropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, working]);
  if (typeof document === "undefined") return null;

  let body: ReactNode;
  if (state.step === "working") {
    body = (
      <>
        <h2 id="unity-open-title">Unity で開く</h2>
        <p className="meta" role="status">{state.message}…</p>
      </>
    );
  } else if (state.step === "waiting") {
    const rest = state.prepared.length - state.next;
    body = (
      <>
        <h2 id="unity-open-title">Unity の起動を待っています</h2>
        <p className="meta">
          {state.next} 件を渡しました。Unity が起動して最初の Import 画面を閉じたら、残りの {rest} 件を渡してください。
        </p>
        <ul className="file-list unity-open-list">
          {state.prepared.slice(state.next).map((item, index) => (
            <li key={`${index}:${item.path}`}>
              <span className="download-name">{item.name}</span>
              <span className="meta">{formatBytes(item.size)}</span>
            </li>
          ))}
        </ul>
        <div className="row-actions">
          <button className="button primary" type="button" onClick={onResume}>
            続きを Unity に渡す
          </button>
          <button className="button" type="button" onClick={onClose}>
            やめる
          </button>
        </div>
      </>
    );
  } else if (state.step === "setup") {
    const item = state.list.packages[0]!;
    body = (
      <>
        <h2 id="unity-open-title">Unity で開く</h2>
        <ul className="file-list unity-open-list">
          <li>
            <span className="download-name">{item.name}</span>
            <span className="meta">{formatBytes(item.size)}</span>
          </li>
        </ul>
        <p className="meta warn-text">{state.missing ? UNITY_MISSING : UNITY_NOT_SET}</p>
        <div className="row-actions">
          <button className="button primary" type="button" onClick={onSettings}>
            設定を開く
          </button>
          <button className="button" type="button" onClick={() => onImport(state.entries, "system")}>
            関連付けで開く
          </button>
          <button className="button" type="button" onClick={onClose}>
            キャンセル
          </button>
        </div>
      </>
    );
  } else {
    const { list, checked, mode } = state;
    const chosen = selectedEntries(list, checked);
    const all = list.packages.map((item) => item.entry);
    body = (
      <>
        <h2 id="unity-open-title">インポートする Unity パッケージ</h2>
        <p className="meta">
          zip の中に {list.packages.length} 件あります。選んだものを順に 1 件ずつ Unity に渡します。Unity の Import 画面でそれぞれ確認してください。
        </p>
        <ul className="file-list unity-open-list" aria-label="Unity パッケージ">
          {list.packages.map((item) => (
            <li key={item.entry}>
              <label className="unity-open-choice">
                <input type="checkbox" checked={checked.has(item.entry)} onChange={() => onToggle(item.entry)} />
                <span className="download-name" title={item.entry}>{item.name}</span>
              </label>
              <span className="meta">{formatBytes(item.size)}</span>
            </li>
          ))}
        </ul>
        {list.truncated && <p className="meta">zip の項目が多いため、先頭の一部だけを表示しています。</p>}
        {mode === "system" && <p className="meta warn-text">{state.missing ? UNITY_MISSING : UNITY_NOT_SET}</p>}
        <div className="row-actions">
          <button className="button primary" type="button" disabled={chosen.length === 0} onClick={() => onImport(chosen, mode)}>
            選んだものをインポート{chosen.length ? `（${chosen.length}件）` : ""}
          </button>
          <button className="button" type="button" onClick={() => onImport(all, mode)}>
            すべてインポート
          </button>
          {mode === "system" && (
            <button className="button" type="button" onClick={onSettings}>
              設定を開く
            </button>
          )}
          <button className="button" type="button" onClick={onClose}>
            キャンセル
          </button>
        </div>
      </>
    );
  }

  return createPortal(
    <div
      className="preview-overlay unity-open-overlay"
      // The dialog is portaled, but React still bubbles its clicks to the row that opened it.
      onClick={(event) => event.stopPropagation()}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        ref={panel}
        className="panel unity-open-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="unity-open-title"
        aria-busy={working}
        tabIndex={-1}
      >
        {body}
      </section>
    </div>,
    document.body,
  );
}

/**
 * 「Unityで開く」ボタン。行の操作（.row-actions）にそのまま置ける。ダイアログは body に出す。
 * 通知はアプリ上部の帯に出す。`onFail` を渡すと、失敗だけをその場（ファイルタブなど）に出せる。
 */
export function UnityOpenButton({
  target,
  notify,
  fail,
  className,
  label = "Unityで開く",
}: {
  target: UnityOpenTarget;
  notify: (message: string) => void;
  fail: (message: string) => void;
  className?: string;
  label?: string;
}) {
  const unity = useUnityOpen({ notify, fail });
  return (
    <>
      <button
        className={className}
        type="button"
        disabled={unity.busy}
        title="zip は展開せず、選んだ Unity パッケージだけを取り出して Unity に渡します"
        onClick={(event) => {
          event.stopPropagation();
          void unity.open(target);
        }}
      >
        {label}
      </button>
      {unity.dialog}
    </>
  );
}

/**
 * ダウンロード完了の通知に付ける操作。通知に操作ボタンを付けられるようになったら
 * （`notify(message, action)`）、ここで作ったものを渡す。
 */
export function unityNoticeAction(
  open: (target: UnityOpenTarget) => Promise<void>,
  target: UnityOpenTarget,
): { label: string; run: () => void } {
  return { label: "Unityで開く", run: () => void open(target) };
}
