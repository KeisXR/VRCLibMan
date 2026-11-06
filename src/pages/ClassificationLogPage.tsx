import { useCallback, useDeferredValue, useEffect, useState } from "react";
import { api } from "../api";
import { useShell } from "../App";
import {
  categoryName,
  classificationLogHref,
  classifierLabel,
  confidenceText,
  formatLogTime,
  linkNames,
  runKindLabel,
  runNoteLabel,
  unresolvedListText,
} from "../classificationLog";
import type { ClassificationEntry, ClassificationRunView, UnresolvedNameCount } from "../types";
import { Pager, errorText } from "../ui";

const RUN_PAGE = 50;
const ENTRY_PAGE = 100;

function EntryRow({ entry, showRun }: { entry: ClassificationEntry; showRun: boolean }) {
  const { refresh, notify, fail } = useShell();
  const [busy, setBusy] = useState(false);
  const categoryChanged = entry.beforeCategory !== entry.afterCategory;
  const undoable = entry.kind !== "undo" && !entry.undone;

  async function undo() {
    setBusy(true);
    try {
      await api.undoClassificationEntry(entry.id);
      notify("分類を元に戻しました");
      refresh();
    } catch (err) {
      fail(errorText(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <li className="log-entry">
      <div className="log-entry-head">
        <strong>{entry.title ?? "商品情報なし"}</strong>
        <a className="meta" href={classificationLogHref(entry.boothId)}>
          {entry.boothId}
        </a>
        {entry.undone && <span className="badge">元に戻しました</span>}
      </div>
      {showRun && (
        <p className="meta">
          {formatLogTime(entry.startedAt)} ・{runKindLabel(entry.kind)}
        </p>
      )}
      <dl className="log-changes">
        <dt>カテゴリ</dt>
        <dd>
          {categoryChanged
            ? `${categoryName(entry.beforeCategory)} → ${categoryName(entry.afterCategory)}`
            : categoryName(entry.afterCategory)}
        </dd>
        <dt>対応アバター</dt>
        <dd>
          {linkNames(entry.beforeLinks)} → {linkNames(entry.afterLinks)}
        </dd>
        {entry.confidence != null && (
          <>
            <dt>信頼度</dt>
            <dd>{confidenceText(entry.confidence)}</dd>
          </>
        )}
        {entry.unresolvedNames.length > 0 && (
          <>
            <dt>未収録の名前</dt>
            <dd>{entry.unresolvedNames.join("、")}</dd>
          </>
        )}
      </dl>
      {undoable && (
        <div className="log-undo">
          <button
            className="button"
            type="button"
            disabled={busy || entry.undoBlocked != null}
            title={entry.undoBlocked ?? undefined}
            onClick={() => void undo()}
          >
            元に戻す
          </button>
          {entry.undoBlocked && <span className="meta">{entry.undoBlocked}</span>}
        </div>
      )}
    </li>
  );
}

function RunEntries({ run }: { run: ClassificationRunView }) {
  const { revision, fail } = useShell();
  const [entries, setEntries] = useState<ClassificationEntry[] | null>(null);
  const [offset, setOffset] = useState(0);

  useEffect(() => {
    let alive = true;
    void api.classificationEntries(run.id, offset, ENTRY_PAGE).then(
      (next) => {
        if (alive) setEntries(next);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [run.id, offset, revision, fail]);

  if (!entries) return <p className="meta">読み込み中</p>;
  return (
    <>
      <ul className="log-entries">
        {entries.map((entry) => (
          <EntryRow key={entry.id} entry={entry} showRun={false} />
        ))}
      </ul>
      <Pager offset={offset} pageSize={ENTRY_PAGE} total={run.changedCount} onChange={setOffset} />
    </>
  );
}

function RunRow({ run }: { run: ClassificationRunView }) {
  const [open, setOpen] = useState(false);
  const note = runNoteLabel(run.kind, run.note);
  const classifier = classifierLabel(run.classifier);
  return (
    <article className="log-run">
      <button className="log-run-head" type="button" aria-expanded={open} onClick={() => setOpen((value) => !value)}>
        <span className="log-run-time">{formatLogTime(run.startedAt)}</span>
        <strong>{runKindLabel(run.kind)}</strong>
        {note && <span className="meta">{note}</span>}
        {classifier && <span className="meta">{classifier}</span>}
        <span className="log-run-count">{run.changedCount} 件</span>
      </button>
      {open && <RunEntries run={run} />}
    </article>
  );
}

function RunList() {
  const { revision, fail } = useShell();
  const [runs, setRuns] = useState<ClassificationRunView[] | null>(null);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);

  useEffect(() => {
    let alive = true;
    void api.classificationRuns(offset, RUN_PAGE).then(
      (page) => {
        if (!alive) return;
        setRuns(page.runs);
        setTotal(page.total);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [offset, revision, fail]);

  if (!runs) return <p className="meta">読み込み中</p>;
  if (runs.length === 0) return <p className="empty">まだ記録はありません。分類が商品を変更すると、ここに残ります。</p>;
  return (
    <div className="stack">
      {runs.map((run) => (
        <RunRow key={run.id} run={run} />
      ))}
      <Pager offset={offset} pageSize={RUN_PAGE} total={total} onChange={setOffset} />
    </div>
  );
}

function SearchResults({ query }: { query: string }) {
  const { revision, fail } = useShell();
  const [entries, setEntries] = useState<ClassificationEntry[] | null>(null);

  useEffect(() => {
    let alive = true;
    void api.searchClassificationEntries(query).then(
      (next) => {
        if (alive) setEntries(next);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [query, revision, fail]);

  if (!entries) return <p className="meta">読み込み中</p>;
  if (entries.length === 0) return <p className="empty">この商品の記録はありません。</p>;
  return (
    <ul className="log-entries">
      {entries.map((entry) => (
        <EntryRow key={entry.id} entry={entry} showRun />
      ))}
    </ul>
  );
}

function UnresolvedNames() {
  const { revision, notify, fail } = useShell();
  const [names, setNames] = useState<UnresolvedNameCount[]>([]);

  useEffect(() => {
    let alive = true;
    void api.unresolvedNames().then(
      (next) => {
        if (alive) setNames(next);
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [revision, fail]);

  const copy = useCallback(() => {
    void navigator.clipboard.writeText(unresolvedListText(names)).then(
      () => notify("未収録の名前をコピーしました"),
      () => fail("コピーできませんでした"),
    );
  }, [names, notify, fail]);

  if (names.length === 0) return null;
  return (
    <details className="settings-details log-unresolved">
      <summary>未収録の名前 {names.length} 件</summary>
      <p className="meta">対応表に無かった名前です。対応表の欠けとして GitHub の Issue に報告するときに使えます。</p>
      <ul className="log-unresolved-list">
        {names.map((item) => (
          <li key={item.name}>
            <span>{item.name}</span>
            <span className="meta">{item.productCount} 件</span>
          </li>
        ))}
      </ul>
      <button className="button" type="button" onClick={copy}>
        一覧をコピー
      </button>
    </details>
  );
}

export function ClassificationLogPage({ boothId }: { boothId?: string }) {
  const [query, setQuery] = useState(boothId ?? "");
  const needle = useDeferredValue(query.trim());

  useEffect(() => {
    setQuery(boothId ?? "");
  }, [boothId]);

  return (
    <section className="log-page">
      <div className="page-head">
        <div>
          <h1>分類記録</h1>
          <p className="meta">分類がいつ、何をどこへ振り分けたかの記録です。見なくても困りません。違っていた変更は 1 件ずつ元に戻せます。</p>
        </div>
      </div>
      <label className="field log-search">
        <span>商品で絞り込む</span>
        <input
          className="search"
          value={query}
          placeholder="商品名・商品ID"
          onChange={(event) => setQuery(event.target.value)}
        />
      </label>
      <UnresolvedNames />
      {needle ? <SearchResults query={needle} /> : <RunList />}
    </section>
  );
}
