import type { FileEntry, ZipEntry } from "./types";

/**
 * Right panel file tab: pure helpers. The tab only lists. It never writes, moves or deletes,
 * and zip files are read from their index without extracting anything.
 */

/** "Unityで開く" on .unitypackage and zip rows. Only files inside the library or the remote library. */
export const UNITY_OPEN_AVAILABLE = true;

export type FileKind = "dir" | "zip" | "unitypackage" | "file" | "link";

export function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

export function fileKind(name: string, kind: FileEntry["kind"]): FileKind {
  if (kind === "dir" || kind === "link") return kind;
  const ext = extensionOf(name);
  if (ext === "zip") return "zip";
  if (ext === "unitypackage") return "unitypackage";
  return "file";
}

export const FILE_KIND_LABEL: Record<FileKind, string> = {
  dir: "フォルダ",
  zip: "zip",
  unitypackage: "Unity パッケージ",
  file: "ファイル",
  link: "リンク",
};

/** Which separator a path uses. Windows paths may mix both; `\` wins when present. */
export function separatorOf(path: string): "\\" | "/" {
  return path.includes("\\") ? "\\" : "/";
}

export function joinPath(dir: string, name: string): string {
  const sep = separatorOf(dir);
  return dir.endsWith(sep) || dir.endsWith("/") ? `${dir}${name}` : `${dir}${sep}${name}`;
}

/** Folder that holds `path`, or null at a root such as `C:\` or `/`. */
export function parentOf(path: string): string | null {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (cut < 0) return null;
  const head = trimmed.slice(0, cut);
  if (head === "") return trimmed.startsWith("/") ? "/" : null;
  if (/^[A-Za-z]:$/.test(head)) return `${head}\\`;
  if (/^\\\\[^\\]+$/.test(head)) return null;
  return head;
}

export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  return cut < 0 ? trimmed : trimmed.slice(cut + 1);
}

export interface ZipChild {
  name: string;
  /** Path inside the archive, `/`-separated. */
  path: string;
  dir: boolean;
  size: number | null;
}

/**
 * Direct children of `inner` (`""` for the archive root). Folders that only appear as part of a
 * deeper path are listed too, since many zips have no entries for their folders.
 */
export function zipChildren(entries: ZipEntry[], inner: string): ZipChild[] {
  const prefix = inner ? `${inner.replace(/\/+$/, "")}/` : "";
  const seen = new Map<string, ZipChild>();
  for (const entry of entries) {
    if (!entry.path.startsWith(prefix) || entry.path === prefix.replace(/\/$/, "")) continue;
    const rest = entry.path.slice(prefix.length);
    if (!rest) continue;
    const slash = rest.indexOf("/");
    const name = slash < 0 ? rest : rest.slice(0, slash);
    const dir = slash >= 0 || entry.dir;
    const path = `${prefix}${name}`;
    const current = seen.get(name);
    if (current && (current.dir || !dir)) continue;
    seen.set(name, { name, path, dir, size: dir ? null : entry.size });
  }
  return sortByFolderThenName([...seen.values()], (child) => child.dir);
}

export function zipParent(inner: string): string | null {
  if (!inner) return null;
  const cut = inner.lastIndexOf("/");
  return cut < 0 ? "" : inner.slice(0, cut);
}

/** `.unitypackage` files anywhere in an archive. "Unityで開く" on a zip offers these. */
export function zipUnityPackages(entries: ZipEntry[]): ZipEntry[] {
  return entries.filter((entry) => !entry.dir && extensionOf(entry.path) === "unitypackage");
}

export function sortByFolderThenName<T extends { name: string }>(items: T[], isDir: (item: T) => boolean): T[] {
  return [...items].sort((left, right) => {
    const rank = Number(!isDir(left)) - Number(!isDir(right));
    if (rank !== 0) return rank;
    const lower = left.name.toLowerCase().localeCompare(right.name.toLowerCase(), "ja");
    return lower !== 0 ? lower : left.name.localeCompare(right.name, "ja");
  });
}

/** What the path box submits. A path ending in `.zip` opens the archive's index instead. */
export function isZipPath(path: string): boolean {
  return extensionOf(baseName(path.trim().replace(/^"(.*)"$/, "$1"))) === "zip";
}
