import { useCallback, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { api, revealInFolder } from "../api";
import {
  FILE_KIND_LABEL,
  UNITY_OPEN_AVAILABLE,
  fileKind,
  isZipPath,
  joinPath,
  parentOf,
  zipChildren,
  zipParent,
  zipUnityPackages,
} from "../fileBrowser";
import { useShell } from "../App";
import { UnityOpenButton } from "./UnityOpenDialog";
import { boothIdFromPath, isInsideRoot } from "../unityOpen";
import type { FileKind } from "../fileBrowser";
import { formatBytes } from "../media";
import type { DirListing, ZipListing } from "../types";
import { errorText } from "../ui";

type Location = { kind: "dir"; path: string } | { kind: "zip"; path: string; inner: string };

interface UnityHooks {
  canUnity: (path: string) => boolean;
  notify: (message: string) => void;
  fail: (message: string) => void;
}

/** Where the tab was, for this session only. Paths are never written to settings. */
let remembered: Location | null = null;

function KindGlyph({ kind }: { kind: FileKind }) {
  switch (kind) {
    case "dir":
      return (
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path d="M1.5 3.5h4l1.2 1.5h5.8v6.5h-11z" />
        </svg>
      );
    case "zip":
      return (
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path d="M3 1.5h8v11H3zM7 1.5v1.5M7 4v1.5M7 6.5V8M6 9.5h2v1.5H6z" />
        </svg>
      );
    case "unitypackage":
      return (
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path d="M7 1.5 12 4.2v5.6L7 12.5 2 9.8V4.2zM2 4.2 7 7l5-2.8M7 7v5.5" />
        </svg>
      );
    case "link":
      return (
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path d="M6 8 8 6M5 6.5 3.5 8a2 2 0 0 0 2.8 2.8L7.8 9.3M9 7.5 10.5 6a2 2 0 0 0-2.8-2.8L6.2 4.7" />
        </svg>
      );
    default:
      return (
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path d="M3 1.5h5l3 3v8H3zM8 1.5v3h3" />
        </svg>
      );
  }
}

function UpGlyph() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M6 10.5v-9M2 5.5l4-4 4 4" />
    </svg>
  );
}

function EntryLabel({ kind, name, detail }: { kind: FileKind; name: string; detail: string }) {
  return (
    <>
      <span className={`files-icon files-icon-${kind}`}>
        <KindGlyph kind={kind} />
      </span>
      <span className="files-text">
        <span className="files-name">{name}</span>
        <span className="meta files-detail">{detail}</span>
      </span>
    </>
  );
}

function sizeText(kind: FileKind, size: number | null): string {
  if (kind === "dir" || kind === "link" || size == null) return FILE_KIND_LABEL[kind];
  return `${FILE_KIND_LABEL[kind]}・${formatBytes(size)}`;
}

/**
 * Right panel file tab: a small read-only explorer. Opens the library folder by default; the
 * path box can point anywhere, still read only. Zip files show their index without extracting.
 */
export function FilesTab() {
  const [location, setLocation] = useState<Location | null>(remembered);
  const [dir, setDir] = useState<DirListing | null>(null);
  const [zip, setZip] = useState<ZipListing | null>(null);
  const [roots, setRoots] = useState<{ library: string | null; remote: string | null }>({ library: null, remote: null });
  const [draft, setDraft] = useState(remembered?.path ?? "");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const request = useRef(0);

  const load = useCallback(async (target: Location) => {
    const ticket = ++request.current;
    setBusy(true);
    try {
      if (target.kind === "dir") {
        const listing = await api.listDirectory(target.path);
        if (ticket !== request.current) return;
        const next: Location = { kind: "dir", path: listing.path };
        remembered = next;
        setLocation(next);
        setDir(listing);
        setZip(null);
        setRoots({ library: listing.library, remote: listing.remote });
        setDraft(listing.path);
      } else {
        const listing = await api.listZipEntries(target.path);
        if (ticket !== request.current) return;
        const next: Location = { kind: "zip", path: listing.path, inner: target.inner };
        remembered = next;
        setLocation(next);
        setZip(listing);
        setDraft(listing.path);
      }
      setError("");
    } catch (err) {
      if (ticket === request.current) setError(errorText(err));
    } finally {
      if (ticket === request.current) setBusy(false);
    }
  }, []);

  useEffect(() => {
    void load(remembered ?? { kind: "dir", path: "" });
  }, [load]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const typed = draft.trim();
    void load(isZipPath(typed) ? { kind: "zip", path: typed, inner: "" } : { kind: "dir", path: typed });
  };

  const goUp = () => {
    if (!location) return;
    if (location.kind === "zip") {
      const inner = zipParent(location.inner);
      if (inner != null) {
        const next: Location = { ...location, inner };
        remembered = next;
        setLocation(next);
        setError("");
        return;
      }
      const folder = parentOf(location.path);
      if (folder) void load({ kind: "dir", path: folder });
      return;
    }
    if (dir?.parent) void load({ kind: "dir", path: dir.parent });
  };

  const reveal = (path: string) => {
    void revealInFolder(path).catch((err: unknown) => setError(errorText(err)));
  };

  const canGoUp = location?.kind === "zip" || Boolean(dir?.parent);
  const inZip = location?.kind === "zip" && zip != null;
  const insideLibrary = dir?.insideLibrary ?? false;
  const { notify } = useShell();
  const unityFail = useCallback((message: string) => setError(message), []);
  /** Rust refuses files outside the library and the remote library, so the button is not offered there. */
  const canUnity = (path: string) =>
    UNITY_OPEN_AVAILABLE && (isInsideRoot(path, roots.library) || isInsideRoot(path, roots.remote));
  const unity = { canUnity, notify, fail: unityFail };

  return (
    <div className="files-tab" aria-busy={busy}>
      <form className="files-path" onSubmit={submit}>
        <button
          className="right-panel-icon"
          type="button"
          aria-label="上の階層へ"
          title="上の階層へ"
          disabled={!canGoUp || busy}
          onClick={goUp}
        >
          <UpGlyph />
        </button>
        <input
          className="search"
          type="text"
          value={draft}
          spellCheck={false}
          autoComplete="off"
          aria-label="表示するフォルダのパス"
          placeholder="フォルダのパス"
          onChange={(event) => setDraft(event.target.value)}
        />
        <button className="button" type="submit" disabled={busy}>
          移動
        </button>
      </form>
      <div className="row-actions files-places">
        <button className="button" type="button" disabled={busy} onClick={() => void load({ kind: "dir", path: "" })}>
          ライブラリ
        </button>
        {roots.remote && (
          <button className="button" type="button" disabled={busy} onClick={() => void load({ kind: "dir", path: roots.remote! })}>
            リモート
          </button>
        )}
        {location && (
          <button className="button" type="button" disabled={busy} onClick={() => void load(location)}>
            再読み込み
          </button>
        )}
      </div>
      {error && (
        <p className="meta warn-text files-error" role="alert">
          {error}
        </p>
      )}
      {inZip && location.kind === "zip" ? (
        <ZipView zip={zip} inner={location.inner} onOpen={(inner) => {
          const next: Location = { ...location, inner };
          remembered = next;
          setLocation(next);
          setError("");
        }} onReveal={() => reveal(zip.path)} unity={unity} />
      ) : dir ? (
        <>
          <p className="meta files-where">
            {insideLibrary ? "ライブラリの中" : "ライブラリの外"}・読み取りのみ
          </p>
          <DirView listing={dir} busy={busy} onOpen={(target) => void load(target)} onReveal={reveal} unity={unity} />
        </>
      ) : !error && busy ? (
        <p className="meta">読み込み中…</p>
      ) : null}
    </div>
  );
}

function DirView({
  listing,
  busy,
  onOpen,
  onReveal,
  unity,
}: {
  listing: DirListing;
  busy: boolean;
  onOpen: (target: Location) => void;
  onReveal: (path: string) => void;
  unity: UnityHooks;
}) {
  if (listing.entries.length === 0) return <p className="meta">このフォルダは空です。</p>;
  return (
    <>
      <ul className="file-list files-list" aria-label="フォルダの中身">
        {listing.entries.map((entry) => {
          const kind = fileKind(entry.name, entry.kind);
          const path = joinPath(listing.path, entry.name);
          const detail = kind === "link" ? "リンク（たどりません）" : sizeText(kind, entry.size);
          const opens = kind === "dir" || kind === "zip";
          return (
            <li className={`files-row files-row-${kind}`} key={entry.name}>
              {opens ? (
                <button
                  className="files-entry"
                  type="button"
                  disabled={busy}
                  title={kind === "zip" ? "zip の中身を見る（展開しません）" : "開く"}
                  onClick={() => onOpen(kind === "zip" ? { kind: "zip", path, inner: "" } : { kind: "dir", path })}
                >
                  <EntryLabel kind={kind} name={entry.name} detail={detail} />
                </button>
              ) : (
                <div className="files-entry">
                  <EntryLabel kind={kind} name={entry.name} detail={detail} />
                </div>
              )}
              {kind !== "dir" && kind !== "link" && (
                <div className="row-actions">
                  {(kind === "unitypackage" || kind === "zip") && unity.canUnity(path) && (
                    <UnityOpenButton
                      target={{ path, boothId: boothIdFromPath(path) }}
                      notify={unity.notify}
                      fail={unity.fail}
                    />
                  )}
                  <button type="button" onClick={() => onReveal(path)}>
                    保存先を開く
                  </button>
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {listing.truncated && <p className="meta">項目が多いため、先頭の一部だけを表示しています。</p>}
    </>
  );
}

function ZipView({
  zip,
  inner,
  onOpen,
  onReveal,
  unity,
}: {
  zip: ZipListing;
  inner: string;
  onOpen: (inner: string) => void;
  onReveal: () => void;
  unity: UnityHooks;
}) {
  const children = zipChildren(zip.entries, inner);
  const canUnity = unity.canUnity(zip.path) && zipUnityPackages(zip.entries).length > 0;
  const boothId = boothIdFromPath(zip.path);
  return (
    <>
      <div className="files-zip-head">
        <div>
          <p className="meta">zip の中身（展開していません）</p>
          {inner ? <p className="meta files-zip-inner">zip 内: {inner}</p> : null}
        </div>
        <div className="row-actions">
          {canUnity && (
            <UnityOpenButton className="button" target={{ path: zip.path, boothId }} notify={unity.notify} fail={unity.fail} />
          )}
          <button className="button" type="button" onClick={onReveal}>
            zip の保存先を開く
          </button>
        </div>
      </div>
      {children.length === 0 ? (
        <p className="meta">中身がありません。</p>
      ) : (
        <ul className="file-list files-list" aria-label="zip の中身">
          {children.map((child) => {
            const kind = fileKind(child.name, child.dir ? "dir" : "file");
            const detail = sizeText(kind, child.size);
            return (
              <li className={`files-row files-row-${kind}`} key={child.path}>
                {child.dir ? (
                  <button className="files-entry" type="button" title="開く" onClick={() => onOpen(child.path)}>
                    <EntryLabel kind={kind} name={child.name} detail={detail} />
                  </button>
                ) : (
                  <div className="files-entry">
                    <EntryLabel kind={kind} name={child.name} detail={detail} />
                  </div>
                )}
                {kind === "unitypackage" && canUnity && (
                  <div className="row-actions">
                    <UnityOpenButton
                      target={{ path: zip.path, boothId, entry: child.path }}
                      notify={unity.notify}
                      fail={unity.fail}
                    />
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {zip.truncated && <p className="meta">項目が多いため、先頭の一部だけを表示しています。</p>}
    </>
  );
}
