import { memo, useCallback, useDeferredValue, useEffect, useMemo, useState } from "react";
import { api, openLocal } from "../api";
import { useShell } from "../App";
import { formatBytes } from "../media";
import type { ProductOption, UnlinkedFile } from "../types";
import { Pager, errorText } from "../ui";

type OptionHit = ProductOption & { haystack: string };

const PAGE = 50;

const FileEditor = memo(function FileEditor({
  file,
  options,
  onDone,
  onError,
}: {
  file: UnlinkedFile;
  options: readonly OptionHit[];
  onDone: (message: string) => void;
  onError: (message: string) => void;
}) {
  const [query, setQuery] = useState("");
  const deferred = useDeferredValue(query);
  const [productId, setProductId] = useState(file.candidateProductId ?? "");
  const visible = useMemo(() => {
    const needle = deferred.trim().toLowerCase();
    if (!needle) return options.slice(0, 30);
    const matched: OptionHit[] = [];
    for (const option of options) {
      if (!option.haystack.includes(needle)) continue;
      matched.push(option);
      if (matched.length >= 30) break;
    }
    return matched;
  }, [deferred, options]);

  const link = useCallback(
    async (id: string | null) => {
      try {
        await api.linkFile(file.path, id);
        onDone(id ? "ファイルを商品に結び付けました" : "結びつきを外しました");
      } catch (err) {
        onError(errorText(err));
      }
    },
    [file.path, onDone, onError],
  );

  return (
    <article className="edit-card">
      <div>
        <h3 className="card-title">{file.filename}</h3>
        <div className="meta">
          {file.isDir ? "フォルダ" : "ファイル"} ・ {formatBytes(file.size)}
        </div>
        <button className="button" type="button" onClick={() => void openLocal(file.path).catch((err: unknown) => onError(errorText(err)))}>
          開く
        </button>
      </div>
      {file.candidateProductId && (
        <button className="button" type="button" onClick={() => void link(file.candidateProductId)}>
          候補「{file.candidateTitle ?? file.candidateProductId}」に結び付ける
        </button>
      )}
      <div className="form-grid">
        <input className="search" value={query} placeholder="商品を検索" onChange={(event) => setQuery(event.target.value)} />
        <select value={productId} onChange={(event) => setProductId(event.target.value)}>
          <option value="">商品を選ぶ</option>
          {visible.map((option) => (
            <option key={option.boothId} value={option.boothId}>
              {option.title}
            </option>
          ))}
        </select>
      </div>
      <button className="button primary" type="button" disabled={!productId} onClick={() => void link(productId)}>
        選択した商品に結び付ける
      </button>
    </article>
  );
});

/** スキャンで商品に結び付かなかったファイル。やることの一覧ではないので、件数は折りたたみの中だけに出す。 */
export function UnlinkedFiles() {
  const { revision, refresh, notify, fail } = useShell();
  const [files, setFiles] = useState<UnlinkedFile[]>([]);
  const [total, setTotal] = useState(0);
  const [offset, setOffset] = useState(0);
  const [options, setOptions] = useState<OptionHit[]>([]);
  const [open, setOpen] = useState(false);

  const onDone = useCallback(
    (message: string) => {
      notify(message);
      refresh();
    },
    [notify, refresh],
  );

  useEffect(() => {
    let alive = true;
    void api.unlinkedFiles(offset, PAGE).then(
      (page) => {
        if (!alive) return;
        setFiles(page.files);
        setTotal(page.total);
        // 結び付けて減り、今のページが空になったら 1 ページ戻る。
        if (page.files.length === 0 && offset > 0) setOffset(Math.max(0, offset - PAGE));
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [offset, revision, fail]);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    void api.productOptions().then(
      (next) => {
        if (!alive) return;
        setOptions(next.map((option) => ({ ...option, haystack: `${option.title} ${option.shop} ${option.boothId}`.toLowerCase() })));
      },
      (err: unknown) => {
        if (alive) fail(errorText(err));
      },
    );
    return () => {
      alive = false;
    };
  }, [open, revision, fail]);

  if (total === 0) return null;
  return (
    <details className="settings-details" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>商品未設定のファイル {total} 件</summary>
      {open && (
        <div className="stack">
          {files.map((file) => (
            <FileEditor key={file.path} file={file} options={options} onDone={onDone} onError={fail} />
          ))}
          <Pager offset={offset} pageSize={PAGE} total={total} onChange={setOffset} />
        </div>
      )}
    </details>
  );
}
