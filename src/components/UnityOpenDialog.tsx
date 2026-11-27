import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { openSettingsSection } from "../anchorScroll";
import { api } from "../api";
import { useModalFocus } from "../keyboard";
import { formatBytes } from "../media";
import type {
  PackageCheck,
  PreparedUnityPackage,
  UnityInstance,
  UnityLaunchMode,
  UnityPackageList,
  UnityProject,
  UnityTarget,
} from "../types";
import { errorText } from "../ui";
import {
  UNITY_MISSING,
  UNITY_NOT_SET,
  destinationFolder,
  isUnityLooseName,
  isUnityNotRunning,
  launchedMessage,
  missingNeedsText,
  needsUnityTarget,
  onlyEntry,
  selectedEntries,
  unityEditorReady,
  unityInstanceDetail,
  unityOpenPlan,
} from "../unityOpen";

type DialogState =
  | { step: "choose"; list: UnityPackageList; boothId: string | null; checked: Set<string>; mode: UnityLaunchMode; missing: boolean }
  | { step: "setup"; list: UnityPackageList; boothId: string | null; entries: string[]; missing: boolean }
  | { step: "loose"; list: UnityPackageList; boothId: string | null; entries: string[]; mode: UnityLaunchMode; missing: boolean }
  | { step: "target"; list: UnityPackageList; boothId: string | null; entries: string[]; instances: UnityInstance[]; chosen: string | null }
  | { step: "working"; message: string }
  | { step: "notRunning"; message: string; projects: UnityProject[] }
  | {
      step: "review";
      list: UnityPackageList;
      boothId: string | null;
      entries: string[];
      target: UnityTarget | null;
      check: PackageCheck;
      /** 渡すもの。同梱版は外して始める。 */
      keep: Set<string>;
    }
  | { step: "waiting"; prepared: PreparedUnityPackage[]; next: number; mode: UnityLaunchMode; target: UnityTarget | null };

function targetOf(instance: UnityInstance): UnityTarget {
  return { window: instance.window, pid: instance.pid };
}

export interface UnityOpenTarget {
  /** zip か .unitypackage（または mp3・png など）の絶対パス。ライブラリかリモートライブラリの中のもの。 */
  path: string;
  /** 取り出し先 `unity-import/<商品ID>/` に使う。分からなければ null。 */
  boothId?: string | null;
  /** zip の中の 1 件だけを開くとき、その zip 内パス。 */
  entry?: string;
}

export function openUnitySettings() {
  openSettingsSection("settings-unity");
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
    async (
      prepared: PreparedUnityPackage[],
      start: number,
      mode: UnityLaunchMode,
      target: UnityTarget | null,
      destination?: string,
    ) => {
      const total = prepared.length;
      setDialog({
        step: "working",
        message: total > 1 ? `Unity に渡しています（${start + 1}/${total}）` : "Unity に渡しています",
      });
      const result = await api.launchUnityPackages(prepared.slice(start).map((item) => item.path), mode, target);
      const done = start + result.launched;
      if (done < total) {
        setDialog({ step: "waiting", prepared, next: done, mode, target });
        return;
      }
      setDialog(null);
      notify(
        destination
          ? `${launchedMessage(total, mode)}。取り込むと ${destinationFolder(destination)} に入ります`
          : launchedMessage(total, mode),
      );
    },
    [notify],
  );

  const run = useCallback(
    async (
      list: UnityPackageList,
      entries: string[],
      boothId: string | null,
      mode: UnityLaunchMode,
      target: UnityTarget | null = null,
      asked = false,
      checked = false,
    ) => {
      if (running.current || entries.length === 0) return;
      running.current = true;
      try {
        if (mode === "unity" && !asked) {
          // Unity が 2 つ以上開いていると、-openfile ではどれに届くかを選べない。先に渡す先を選んでもらう。
          const instances = await api.unityInstances().catch(() => []);
          if (needsUnityTarget(instances)) {
            setDialog({ step: "target", list, boothId, entries, instances, chosen: null });
            return;
          }
        }
        if (mode === "unity" && list.kind !== "loose" && !checked) {
          // 渡す先のプロジェクトが決まるとき、もう入っている・足りないものがある・要らない同梱版がある、と
          // 分かっていれば知らせる（止めはしない）。前に調べた結果と、いまの vpm-manifest.json から決まる。
          const check = await api.unityPackageCheck(list.path, entries, boothId, target).catch(() => null);
          if (check) {
            const bundled = new Set(check.bundled.map((item) => item.entry));
            const keep = new Set(entries.filter((entry) => !bundled.has(entry)));
            setDialog({ step: "review", list, boothId, entries, target, check, keep });
            return;
          }
        }
        setDialog({
          step: "working",
          message:
            list.kind === "zip"
              ? "zip から取り出しています"
              : list.kind === "loose"
                ? "Unity パッケージにしています"
                : "Unity に渡しています",
        });
        const prepared = await api.prepareUnityPackages(list.path, entries, boothId);
        await launchFrom(prepared, 0, mode, target, list.kind === "loose" ? list.destination : undefined);
      } catch (err) {
        const message = errorText(err);
        // Unity が起動していないとき、商品を入れられそうなプロジェクト（対応アバターに結び付けたもの、
        // ワールドの商品ならワールドのプロジェクト）があれば、それを開く操作を出す。
        // 開いたあとに自動では渡さない（起動に時間がかかり、いつ受け取れるかを外から確かめられないため）。
        const projects =
          boothId && isUnityNotRunning(message)
            ? (await api.productUnityProjects(boothId).catch(() => [])).filter((project) => project.state === "ok")
            : [];
        if (projects.length > 0) {
          setDialog({ step: "notRunning", message, projects });
        } else {
          setDialog(null);
          fail(message);
        }
      } finally {
        running.current = false;
      }
    },
    [fail, launchFrom],
  );

  const resume = useCallback(
    async (prepared: PreparedUnityPackage[], next: number, mode: UnityLaunchMode, target: UnityTarget | null) => {
      if (running.current) return;
      running.current = true;
      try {
        await launchFrom(prepared, next, mode, target);
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
          api.unityPackages(target.path, target.boothId ?? null),
          api.unityEditors().catch(() => null),
        ]);
        const list = target.entry != null ? onlyEntry(listed, target.entry) : listed;
        if (!list) throw new Error("選んだ Unity パッケージが zip の中に見つかりません");
        // 選んだ Unity へ直接渡すときは既定の Unity を使わない。既定が無くても Unity が 2 つ以上
        // 開いていれば、関連付けに回さず渡す先を選ばせる。
        const ready =
          unityEditorReady(status) || needsUnityTarget(await api.unityInstances().catch(() => []));
        const missing = Boolean(status?.selectedPath.trim() && status.selectedMissing);
        const boothId = target.boothId ?? null;
        const plan = unityOpenPlan(list, ready);
        if (plan.kind === "none") {
          fail("この zip には Unity パッケージ（.unitypackage）がありません");
        } else if (plan.kind === "direct") {
          direct = { list, entries: plan.entries, boothId };
        } else if (plan.kind === "loose") {
          setDialog({ step: "loose", list, boothId, entries: plan.entries, mode: ready ? "unity" : "system", missing });
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
          if (current?.step === "choose") {
            const checked = new Set(current.checked);
            if (checked.has(entry)) checked.delete(entry);
            else checked.add(entry);
            return { ...current, checked };
          }
          if (current?.step === "review") {
            const keep = new Set(current.keep);
            if (keep.has(entry)) keep.delete(entry);
            else keep.add(entry);
            return { ...current, keep };
          }
          return current;
        })
      }
      onImport={(entries, mode) => {
        if (dialog.step === "choose" || dialog.step === "setup" || dialog.step === "loose")
          void run(dialog.list, entries, dialog.boothId, mode);
      }}
      onPickTarget={(window) =>
        setDialog((current) => (current?.step === "target" ? { ...current, chosen: window } : current))
      }
      onSendToTarget={() => {
        if (dialog.step !== "target") return;
        const instance = dialog.instances.find((item) => item.window === dialog.chosen);
        if (!instance) return;
        void run(dialog.list, dialog.entries, dialog.boothId, "unity", targetOf(instance), true);
      }}
      onResume={() => {
        if (dialog.step === "waiting") void resume(dialog.prepared, dialog.next, dialog.mode, dialog.target);
      }}
      onSettings={() => {
        setDialog(null);
        openUnitySettings();
      }}
      onSendReviewed={() => {
        if (dialog.step !== "review") return;
        const entries = dialog.entries.filter((entry) => dialog.keep.has(entry));
        void run(dialog.list, entries, dialog.boothId, "unity", dialog.target, true, true);
      }}
      onOpenProject={(project) => {
        setDialog(null);
        void api.openUnityProject(project.id).then(
          (result) =>
            notify(
              result.alreadyOpen
                ? `${project.name} はすでに Unity で開いています。開き終えたら、もう一度「Unityで開く」を押してください`
                : `${project.name} を Unity で開いています。開き終えたら、もう一度「Unityで開く」を押してください`,
            ),
          (err: unknown) => fail(errorText(err)),
        );
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
  onPickTarget,
  onSendToTarget,
  onResume,
  onSettings,
  onOpenProject,
  onSendReviewed,
}: {
  state: DialogState;
  onClose: () => void;
  onToggle: (entry: string) => void;
  onImport: (entries: string[], mode: UnityLaunchMode) => void;
  onPickTarget: (window: string) => void;
  onSendToTarget: () => void;
  onResume: () => void;
  onSettings: () => void;
  onOpenProject: (project: UnityProject) => void;
  onSendReviewed: () => void;
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
  } else if (state.step === "review") {
    const { check, entries, keep, list } = state;
    const bundled = new Map(check.bundled.map((item) => [item.entry, item.name]));
    const chosen = entries.filter((entry) => keep.has(entry));
    const needs = missingNeedsText(check.missing);
    body = (
      <>
        <h2 id="unity-open-title">{check.projectName} に渡す前に</h2>
        {check.installed && (
          <p className="meta">
            この商品は、このプロジェクトにもう入っています（前に調べた結果）。もう一度渡すと、Unity の Import 画面で同じファイルを上書きするかを選べます。
          </p>
        )}
        {needs.packages && (
          <p className="note note-strong unity-open-needs">
            このプロジェクトに無いもの: {needs.packages}。無いまま取り込むと、マテリアルがピンク色になったり、コンポーネントが「Missing」になったりします。ALCOM で追加してから取り込んでください。
          </p>
        )}
        {needs.products && (
          <p className="note note-strong unity-open-needs">
            {needs.products} のファイルを使っています。このプロジェクトには入っていないので、先に取り込んでください。
          </p>
        )}
        {bundled.size > 0 && (
          <ul className="file-list unity-open-list" aria-label="渡す Unity パッケージ">
            {entries.map((entry) => {
              const item = list.packages.find((pkg) => pkg.entry === entry);
              const owner = bundled.get(entry);
              return (
                <li key={entry} className={owner ? "unity-open-bundled" : undefined}>
                  <label className="unity-open-choice">
                    <input type="checkbox" checked={keep.has(entry)} onChange={() => onToggle(entry)} />
                    <span className="download-name" title={entry || undefined}>{item?.name ?? entry}</span>
                  </label>
                  {owner && (
                    <span className="meta">
                      このプロジェクトには {owner} が入っています。同じものを重ねて取り込むとシェーダーが壊れることがあるので、取り込む必要はありません。
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        )}
        <p className="meta">渡すのは止めません。Unity の Import 画面で中身を確かめてから取り込めます。</p>
        <div className="row-actions">
          <button className="button primary" type="button" disabled={chosen.length === 0} onClick={onSendReviewed}>
            {check.installed ? "もう一度渡す" : "渡す"}
            {entries.length > 1 && chosen.length > 0 ? `（${chosen.length}件）` : ""}
          </button>
          <button className="button" type="button" onClick={onClose}>
            やめる
          </button>
        </div>
      </>
    );
  } else if (state.step === "notRunning") {
    body = (
      <>
        <h2 id="unity-open-title">Unity が起動していません</h2>
        <p className="meta">
          Unity でプロジェクトを開いてから渡します。この商品を入れられそうなプロジェクト（アバターに結び付けたもの、ワールドの商品ならワールドのプロジェクト）を、ここから開けます。開き終えたら、もう一度「Unityで開く」を押してください。
        </p>
        <ul className="file-list unity-open-list">
          {state.projects.map((project) => (
            <li key={project.id}>
              <span className="download-name" title={project.path}>{project.name}</span>
              <button className="button primary" type="button" onClick={() => onOpenProject(project)}>
                {project.running === "open" ? "開いています" : "開く"}
              </button>
            </li>
          ))}
        </ul>
        <div className="row-actions">
          <button className="button" type="button" onClick={onClose}>
            閉じる
          </button>
        </div>
      </>
    );
  } else if (state.step === "target") {
    const { instances, chosen, entries } = state;
    body = (
      <>
        <h2 id="unity-open-title">どの Unity に渡しますか</h2>
        <p className="meta">
          Unity が {instances.length} つ開いています。{entries.length > 1 ? `${entries.length} 件のパッケージを` : "パッケージを"}
          渡すプロジェクトを選んでください。
        </p>
        <ul className="file-list unity-open-list" role="radiogroup" aria-label="渡す先の Unity">
          {instances.map((instance) => {
            const detail = unityInstanceDetail(instance);
            return (
              <li key={instance.window}>
                <label className="unity-open-choice">
                  <input
                    type="radio"
                    name="unity-open-target"
                    checked={chosen === instance.window}
                    onChange={() => onPickTarget(instance.window)}
                  />
                  <span className="download-name" title={instance.editorPath || undefined}>
                    {instance.project}
                  </span>
                </label>
                {detail && <span className="meta">{detail}</span>}
              </li>
            );
          })}
        </ul>
        <div className="row-actions">
          <button className="button primary" type="button" disabled={chosen == null} onClick={onSendToTarget}>
            この Unity に渡す
          </button>
          <button className="button" type="button" onClick={onClose}>
            キャンセル
          </button>
        </div>
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
  } else if (state.step === "loose") {
    const item = state.list.packages[0]!;
    const destination = state.list.destination ?? "";
    body = (
      <>
        <h2 id="unity-open-title">Unity に入れる</h2>
        <ul className="file-list unity-open-list">
          <li>
            <span className="download-name">{item.name}</span>
            <span className="meta">{formatBytes(item.size)}</span>
          </li>
        </ul>
        <p className="meta unity-open-label">Unity の中で入る場所</p>
        <p className="unity-open-destination" title={destination}>{destination}</p>
        <p className="meta">
          Unity パッケージにして Unity に渡します。Unity の Import 画面で確かめてから取り込まれます。
          同じファイルをもう一度渡しても、同じ場所のものとして扱われ、複製はできません。
        </p>
        {state.mode === "system" && <p className="meta warn-text">{state.missing ? UNITY_MISSING : UNITY_NOT_SET}</p>}
        <div className="row-actions">
          {state.mode === "unity" ? (
            <button className="button primary" type="button" onClick={() => onImport(state.entries, "unity")}>
              Unity に渡す
            </button>
          ) : (
            <>
              <button className="button primary" type="button" onClick={onSettings}>
                設定を開く
              </button>
              <button className="button" type="button" onClick={() => onImport(state.entries, "system")}>
                関連付けで開く
              </button>
            </>
          )}
          <button className="button" type="button" onClick={onClose}>
            キャンセル
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
        title={
          isUnityLooseName(target.path)
            ? "Unity パッケージにして Unity に渡します。入る場所は次の画面で確かめられます"
            : "zip は展開せず、選んだ Unity パッケージだけを取り出して Unity に渡します"
        }
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
