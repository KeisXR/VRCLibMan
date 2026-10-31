import type { DirListing, FileEntry, PreparedUnityPackage, UnityPackageList, ZipListing } from "../types";

/**
 * Preview-mode stand-in for the file tab. Dummy names only; no real product names.
 * Errors use the same wording as `src-tauri/src/file_browser.rs`.
 */

const MB = 1024 * 1024;

const dir = (name: string): FileEntry => ({ name, kind: "dir", size: null });
const file = (name: string, size: number): FileEntry => ({ name, kind: "file", size });
const link = (name: string): FileEntry => ({ name, kind: "link", size: null });

const FOLDERS: Record<string, FileEntry[]> = {
  "C:\\": [dir("Assets"), dir("Users")],
  "C:\\Users": [],
  "C:\\Assets": [dir("1000001"), dir("1000002"), dir("1000003"), dir("_未整理"), file("readme.txt", 1200)],
  "C:\\Assets\\1000001": [file("Sample_Outfit_v1.2.zip", 48 * MB), file("Sample_Outfit_Manual.pdf", 1.5 * MB)],
  "C:\\Assets\\1000002": [file("Sample_Hair.unitypackage", 12 * MB), file("Sample_Hair_Textures.zip", 8 * MB)],
  "C:\\Assets\\1000003": [],
  "C:\\Assets\\_未整理": [link("ショートカット"), file("memo.txt", 320)],
  "\\\\nas\\vrclibman": [dir("1000001")],
  "\\\\nas\\vrclibman\\1000001": [file("Sample_Outfit_v1.2.zip", 48 * MB)],
};

const ZIPS: Record<string, ZipListing["entries"]> = {
  "C:\\Assets\\1000001\\Sample_Outfit_v1.2.zip": [
    { path: "Sample_Outfit", dir: true, size: 0 },
    { path: "Sample_Outfit/Sample_Outfit_v1.2.unitypackage", dir: false, size: 40 * MB },
    { path: "Sample_Outfit/Sample_Outfit_PhysBone.unitypackage", dir: false, size: 2 * MB },
    { path: "Sample_Outfit/Textures/body.png", dir: false, size: 4 * MB },
    { path: "Sample_Outfit/Textures/body_normal.png", dir: false, size: 4 * MB },
    { path: "利用規約.txt", dir: false, size: 2400 },
  ],
  "C:\\Assets\\1000002\\Sample_Hair_Textures.zip": [
    { path: "hair_a.png", dir: false, size: 2 * MB },
    { path: "hair_b.png", dir: false, size: 2 * MB },
  ],
};
ZIPS["\\\\nas\\vrclibman\\1000001\\Sample_Outfit_v1.2.zip"] = ZIPS["C:\\Assets\\1000001\\Sample_Outfit_v1.2.zip"]!;
// Download rows in the sample library (asset pages).
ZIPS["C:\\Assets\\Lime\\eye_base.zip"] = [
  { path: "eye_base/Sample_Eye_Base.unitypackage", dir: false, size: 3 * MB },
  { path: "eye_base/Sample_Eye_Extra.unitypackage", dir: false, size: 1 * MB },
  { path: "eye_base/readme.txt", dir: false, size: 900 },
];

function clean(raw: string): string {
  const trimmed = raw.trim();
  const unquoted = trimmed.startsWith('"') && trimmed.endsWith('"') && trimmed.length >= 2 ? trimmed.slice(1, -1) : trimmed;
  const text = unquoted.trim().replace(/\//g, "\\");
  if (/^[A-Za-z]:\\?$/.test(text)) return `${text.slice(0, 2).toUpperCase()}\\`;
  return text.replace(/\\+$/, "").replace(/^([a-z]):/, (_, drive: string) => `${drive.toUpperCase()}:`);
}

function requested(raw: string, library: string): string {
  const text = clean(raw) || clean(library);
  if (!text) throw new Error("ライブラリフォルダが設定されていません。設定で選ぶか、上の欄にフォルダのパスを入力してください");
  if (text.split("\\").includes("..")) throw new Error("「..」を含むパスは開けません。上の階層へは「上へ」を使ってください");
  if (!/^[A-Za-z]:\\/.test(text) && !text.startsWith("\\\\")) {
    throw new Error("フォルダの場所は、ドライブや / から始まるパスで入力してください");
  }
  return text;
}

function lookup(table: Record<string, unknown>, path: string): string | null {
  const lower = path.toLowerCase();
  return Object.keys(table).find((key) => key.toLowerCase() === lower) ?? null;
}

function parent(path: string): string | null {
  if (/^[A-Z]:\\$/.test(path)) return null;
  const cut = path.lastIndexOf("\\");
  if (cut < 0) return null;
  const head = path.slice(0, cut);
  if (/^[A-Z]:$/.test(head)) return `${head}\\`;
  if (/^\\\\[^\\]+$/.test(head) || head === "\\") return null;
  return head;
}

export function previewListDirectory(raw: string, library: string, remote: string): DirListing {
  const path = requested(raw, library);
  const key = lookup(FOLDERS, path);
  if (!key) {
    if (lookup(ZIPS, path)) throw new Error("フォルダではありません");
    throw new Error("このパスは見つかりません");
  }
  const libraryKey = library.trim() ? lookup(FOLDERS, clean(library)) : null;
  const remoteKey = remote.trim() ? lookup(FOLDERS, clean(remote)) : null;
  return {
    path: key,
    parent: parent(key),
    library: libraryKey,
    remote: remoteKey,
    insideLibrary: libraryKey != null && (key === libraryKey || key.startsWith(`${libraryKey}\\`)),
    entries: FOLDERS[key]!.map((entry) => ({ ...entry })),
    truncated: false,
  };
}

export function previewListZip(raw: string): ZipListing {
  const path = requested(raw, "");
  if (!/\.zip$/i.test(path)) throw new Error("zip ファイルではありません");
  const key = lookup(ZIPS, path);
  if (!key) throw new Error("このパスは見つかりません");
  return { path: key, entries: ZIPS[key]!.map((entry) => ({ ...entry })), truncated: false };
}

/**
 * Preview stand-in for "Unityで開く". Same rules as `src-tauri/src/unity_import.rs`: only zip and
 * .unitypackage files inside the library or the remote library; zip files are listed, not extracted.
 */
const OUTSIDE = "ライブラリ（またはリモートライブラリ）の中のファイルだけを Unity で開けます";
const NOT_PACKAGE = "zip か .unitypackage のファイルを選んでください";

function under(path: string, root: string): boolean {
  const base = clean(root).toLowerCase();
  return Boolean(base) && path.toLowerCase().startsWith(`${base}\\`);
}

function baseOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function previewUnityPackages(raw: string, library: string, remote: string, sizeOf: (path: string) => number | null): UnityPackageList {
  if (!raw.trim()) throw new Error(NOT_PACKAGE);
  const path = requested(raw, "");
  if (!/\.(zip|unitypackage)$/i.test(path)) throw new Error(NOT_PACKAGE);
  if (!under(path, library) && !under(path, remote)) throw new Error(OUTSIDE);
  if (/\.unitypackage$/i.test(path)) {
    return { path, kind: "package", packages: [{ entry: "", name: baseOf(path), size: sizeOf(path) ?? 8 * MB }], truncated: false };
  }
  const key = lookup(ZIPS, path);
  if (!key) throw new Error("このパスは見つかりません");
  const packages = ZIPS[key]!
    .filter((entry) => !entry.dir && /\.unitypackage$/i.test(entry.path))
    .map((entry) => ({ entry: entry.path, name: baseOf(entry.path), size: entry.size }));
  return { path: key, kind: "zip", packages, truncated: false };
}

export function previewPrepareUnityPackages(
  list: UnityPackageList,
  entries: string[],
  boothId: string | null,
): PreparedUnityPackage[] {
  if (list.kind === "package") return list.packages.map((item) => ({ name: item.name, path: list.path, size: item.size }));
  if (entries.length === 0) throw new Error("取り出す Unity パッケージを選んでください");
  const folder = `C:\\Users\\Preview\\AppData\\Local\\app.vrclibman.desktop\\unity-import\\${boothId && /^\d{1,12}$/.test(boothId) ? boothId : "other"}`;
  return entries.map((entry) => {
    const found = list.packages.find((item) => item.entry === entry);
    if (!found) throw new Error("選んだ Unity パッケージが zip の中に見つかりません");
    return { name: found.name, path: `${folder}\\${found.name}`, size: found.size };
  });
}
