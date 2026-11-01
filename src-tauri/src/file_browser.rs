//! Read-only listing for the right panel's file tab.
//!
//! Nothing here writes, moves or deletes. Directory entries are read with `symlink_metadata`,
//! so a symlink or junction in a listing is shown as a link and never followed. A typed path
//! is resolved once with `canonicalize` and the resolved path is what the tab shows. Paths with
//! `..` are refused rather than resolved, and relative paths are refused. Zip files are read
//! from their central directory only; no entry is decompressed or extracted.
//! Names and paths are never written to logs.

use crate::library_files::{wait_for, without_verbatim_prefix};
use serde::Serialize;
use std::fs::File;
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Entries shown for one folder. More than this is reported as `truncated`.
pub(crate) const MAX_DIR_ENTRIES: usize = 2000;
/// Entries read from one zip file.
pub(crate) const MAX_ZIP_ENTRIES: usize = 5000;
/// A shared folder that stops answering ends the listing instead of freezing the tab.
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EntryKind {
    Dir,
    File,
    /// Symlink or junction. Shown, never followed.
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirEntryView {
    pub name: String,
    pub kind: EntryKind,
    /// File size in bytes. Folders and links have none.
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirListing {
    /// The resolved folder, without the Windows `\\?\` prefix.
    pub path: String,
    pub parent: Option<String>,
    /// The configured library folder, resolved, when it exists.
    pub library: Option<String>,
    /// The configured remote library folder, resolved, when it answers.
    pub remote: Option<String>,
    /// `path` is the library folder or inside it.
    pub inside_library: bool,
    pub entries: Vec<DirEntryView>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ZipEntryView {
    /// `/`-separated path inside the archive, without a leading `/`.
    pub path: String,
    pub dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ZipListing {
    pub path: String,
    pub library: Option<String>,
    pub remote: Option<String>,
    pub entries: Vec<ZipEntryView>,
    pub truncated: bool,
}

const NO_LIBRARY: &str =
    "ライブラリフォルダが設定されていません。設定で選ぶか、上の欄にフォルダのパスを入力してください";
const NOT_ABSOLUTE: &str = "フォルダの場所は、ドライブや / から始まるパスで入力してください";
const PARENT_PART: &str = "「..」を含むパスは開けません。上の階層へは「上へ」を使ってください";
const NOT_FOUND: &str = "このパスは見つかりません";
pub(crate) const NO_ANSWER: &str = "フォルダが応答しません。接続を確認してからもう一度";

/// Accepts what a user pastes: surrounding spaces or quotes ("Copy as path" on Windows).
fn clean_input(raw: &str) -> &str {
    let trimmed = raw.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(trimmed);
    unquoted.trim()
}

/// Lexical checks before touching the disk.
pub(crate) fn requested_path(raw: &str, library: &str) -> Result<PathBuf, String> {
    let text = clean_input(raw);
    let text = if text.is_empty() { library.trim() } else { text };
    if text.is_empty() {
        return Err(NO_LIBRARY.into());
    }
    let path = PathBuf::from(text);
    if path.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err(PARENT_PART.into());
    }
    if !path.is_absolute() {
        return Err(NOT_ABSOLUTE.into());
    }
    Ok(path)
}

pub(crate) fn resolve(path: &Path) -> Result<PathBuf, String> {
    let owned = path.to_path_buf();
    match wait_for(LIST_TIMEOUT, move || std::fs::canonicalize(&owned)) {
        None => Err(NO_ANSWER.into()),
        Some(Ok(path)) => Ok(without_verbatim_prefix(&path)),
        Some(Err(_)) => Err(NOT_FOUND.into()),
    }
}

/// A configured root that may be missing or offline. Never fails the listing.
pub(crate) fn optional_root(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return None;
    }
    resolve(path).ok().filter(|path| path.is_dir())
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

fn read_entries(dir: PathBuf) -> std::io::Result<(Vec<DirEntryView>, bool)> {
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in std::fs::read_dir(&dir)? {
        let Ok(entry) = entry else { continue };
        if entries.len() >= MAX_DIR_ENTRIES {
            truncated = true;
            break;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        // `DirEntry::file_type` does not follow links, same as `symlink_metadata`.
        let Ok(file_type) = entry.file_type() else { continue };
        let (kind, size) = if file_type.is_symlink() {
            (EntryKind::Link, None)
        } else if file_type.is_dir() {
            (EntryKind::Dir, None)
        } else {
            let size = entry.metadata().ok().map(|meta| meta.len());
            (EntryKind::File, size)
        };
        entries.push(DirEntryView { name, kind, size });
    }
    sort_entries(&mut entries);
    Ok((entries, truncated))
}

/// Folders first, then by name without case. Same order on every platform.
pub(crate) fn sort_entries(entries: &mut [DirEntryView]) {
    entries.sort_by(|left, right| {
        let rank = |entry: &DirEntryView| u8::from(entry.kind != EntryKind::Dir);
        rank(left)
            .cmp(&rank(right))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });
}

/// Lists one folder. An empty `raw` opens the library folder.
pub(crate) fn list_directory(raw: &str, library: &str, remote: &str) -> Result<DirListing, String> {
    let requested = requested_path(raw, library)?;
    let resolved = resolve(&requested)?;
    if !resolved.is_dir() {
        return Err("フォルダではありません".into());
    }
    let read_from = resolved.clone();
    let (entries, truncated) = match wait_for(LIST_TIMEOUT, move || read_entries(read_from)) {
        None => return Err(NO_ANSWER.into()),
        Some(Err(_)) => return Err("このフォルダの中身を読めません".into()),
        Some(Ok(result)) => result,
    };
    let library_root = optional_root(library);
    let remote_root = optional_root(remote);
    let inside_library = library_root
        .as_ref()
        .is_some_and(|root| resolved.starts_with(root));
    Ok(DirListing {
        path: display(&resolved),
        parent: resolved.parent().map(display),
        library: library_root.as_deref().map(display),
        remote: remote_root.as_deref().map(display),
        inside_library,
        entries,
        truncated,
    })
}

/// zip decodes names without the UTF-8 flag as CP437, but on Japanese Windows they are
/// usually Shift_JIS. Valid UTF-8 (Mac, 7-Zip, flagged names) is kept as is; anything that is
/// neither falls back to the zip crate's own decoding.
pub(crate) fn decode_zip_name(raw: &[u8], fallback: &str) -> String {
    if raw.is_ascii() {
        return fallback.to_string();
    }
    if let Ok(text) = std::str::from_utf8(raw) {
        return text.to_string();
    }
    let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(raw);
    if had_errors {
        fallback.to_string()
    } else {
        text.into_owned()
    }
}

/// `\` to `/`, no leading `/`, no empty or `.` segments. Only for display; nothing is extracted.
pub(crate) fn clean_zip_path(name: &str) -> String {
    name.replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn read_zip(path: PathBuf) -> Result<(Vec<ZipEntryView>, bool), String> {
    let file = File::open(&path).map_err(|_| "zip を開けません".to_string())?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|_| "zip として読めません。壊れているか、別の形式です".to_string())?;
    let total = archive.len();
    let mut entries = Vec::new();
    for index in 0..total.min(MAX_ZIP_ENTRIES) {
        let Ok(entry) = archive.by_index_raw(index) else { continue };
        let name = decode_zip_name(entry.name_raw(), entry.name());
        let dir = entry.is_dir() || name.ends_with('/') || name.ends_with('\\');
        let path = clean_zip_path(&name);
        if path.is_empty() {
            continue;
        }
        entries.push(ZipEntryView {
            path,
            dir,
            size: if dir { 0 } else { entry.size() },
        });
    }
    Ok((entries, total > MAX_ZIP_ENTRIES))
}

/// Lists a zip file's entries without extracting anything.
pub(crate) fn list_zip(raw: &str, library: &str, remote: &str) -> Result<ZipListing, String> {
    let requested = requested_path(raw, "")?;
    let is_zip = requested
        .extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("zip"));
    if !is_zip {
        return Err("zip ファイルではありません".into());
    }
    let resolved = resolve(&requested)?;
    if !resolved.is_file() {
        return Err("zip ファイルではありません".into());
    }
    let read_from = resolved.clone();
    let (entries, truncated) = match wait_for(LIST_TIMEOUT, move || read_zip(read_from)) {
        None => return Err(NO_ANSWER.into()),
        Some(result) => result?,
    };
    Ok(ZipListing {
        path: display(&resolved),
        library: optional_root(library).as_deref().map(display),
        remote: optional_root(remote).as_deref().map(display),
        entries,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_files::resolved_temp_dir;
    use std::io::Write;

    fn temp_root(name: &str) -> PathBuf {
        let root = resolved_temp_dir().join(format!(
            "vrclibman-files-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn snapshot(root: &Path) -> Vec<(String, u64)> {
        let mut seen = Vec::new();
        for entry in walkdir::WalkDir::new(root).follow_links(false) {
            let entry = entry.unwrap();
            let meta = std::fs::symlink_metadata(entry.path()).unwrap();
            seen.push((entry.path().display().to_string(), meta.len()));
        }
        seen.sort();
        seen
    }

    #[test]
    fn empty_input_opens_the_library_and_lists_folders_first() {
        let root = temp_root("library");
        std::fs::create_dir(root.join("b-folder")).unwrap();
        std::fs::create_dir(root.join("A-folder")).unwrap();
        std::fs::write(root.join("a.unitypackage"), b"dummy").unwrap();
        std::fs::write(root.join("C.zip"), b"").unwrap();
        let before = snapshot(&root);

        let listing = list_directory("", &root.display().to_string(), "").unwrap();
        assert_eq!(listing.path, root.display().to_string());
        assert!(listing.inside_library);
        assert_eq!(listing.library.as_deref(), Some(listing.path.as_str()));
        assert_eq!(listing.remote, None);
        let names: Vec<_> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["A-folder", "b-folder", "a.unitypackage", "C.zip"]);
        assert_eq!(listing.entries[2].size, Some(5));
        assert_eq!(listing.entries[0].size, None);
        assert!(!listing.truncated);
        assert_eq!(snapshot(&root), before, "listing must not change anything");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn typed_paths_are_checked_before_the_disk() {
        assert_eq!(requested_path("", "").unwrap_err(), NO_LIBRARY);
        assert_eq!(requested_path("   ", "").unwrap_err(), NO_LIBRARY);
        assert_eq!(requested_path("relative/folder", "").unwrap_err(), NOT_ABSOLUTE);
        let root = temp_root("dots");
        let dotted = format!("{}{}..", root.display(), std::path::MAIN_SEPARATOR);
        assert_eq!(requested_path(&dotted, "").unwrap_err(), PARENT_PART);
        let quoted = format!("  \"{}\"  ", root.display());
        assert_eq!(requested_path(&quoted, "").unwrap(), root);
        assert_eq!(
            list_directory(&root.join("missing").display().to_string(), "", "").unwrap_err(),
            NOT_FOUND
        );
        std::fs::write(root.join("file.txt"), b"x").unwrap();
        assert!(list_directory(&root.join("file.txt").display().to_string(), "", "").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn folders_outside_the_library_are_listed_read_only() {
        let library = temp_root("inside");
        let other = temp_root("outside");
        std::fs::create_dir(other.join("sub")).unwrap();
        let listing = list_directory(
            &other.display().to_string(),
            &library.display().to_string(),
            &other.display().to_string(),
        )
        .unwrap();
        assert!(!listing.inside_library);
        assert_eq!(listing.remote.as_deref(), Some(other.display().to_string().as_str()));
        assert_eq!(listing.parent, other.parent().map(|p| p.display().to_string()));
        let missing_library = list_directory(&other.display().to_string(), "/no/such/library", "").unwrap();
        assert_eq!(missing_library.library, None);
        assert!(!missing_library.inside_library);
        std::fs::remove_dir_all(library).unwrap();
        std::fs::remove_dir_all(other).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_shown_but_not_followed() {
        let root = temp_root("links");
        let target = temp_root("link-target");
        std::fs::write(target.join("secret.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(&target, root.join("link")).unwrap();
        let listing = list_directory(&root.display().to_string(), "", "").unwrap();
        assert_eq!(
            listing.entries,
            vec![DirEntryView { name: "link".into(), kind: EntryKind::Link, size: None }]
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    fn write_zip(path: &Path, names: &[(&[u8], bool, &[u8])]) {
        // A stored zip written by hand so the test controls the UTF-8 flag and raw name bytes.
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, utf8, body) in names {
            let offset = out.len() as u32;
            let flags: u16 = if *utf8 { 1 << 11 } else { 0 };
            let crc = crc32(body);
            let local = |buf: &mut Vec<u8>| {
                buf.extend_from_slice(&flags.to_le_bytes());
                buf.extend_from_slice(&0u16.to_le_bytes()); // stored
                buf.extend_from_slice(&0u32.to_le_bytes()); // time/date
                buf.extend_from_slice(&crc.to_le_bytes());
                buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
                buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
                buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
                buf.extend_from_slice(&0u16.to_le_bytes());
            };
            out.extend_from_slice(&0x04034b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            local(&mut out);
            out.extend_from_slice(name);
            out.extend_from_slice(body);
            central.extend_from_slice(&0x02014b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            local(&mut central);
            central.extend_from_slice(&0u16.to_le_bytes()); // comment
            central.extend_from_slice(&0u16.to_le_bytes()); // disk
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name);
        }
        let start = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x06054b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(names.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        File::create(path).unwrap().write_all(&out).unwrap();
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for byte in data {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    #[test]
    fn zip_entries_are_listed_without_extracting() {
        let root = temp_root("zip");
        let archive = root.join("Dummy.ZIP");
        // "テスト" in Shift_JIS, without the UTF-8 flag.
        let sjis_dir: &[u8] = b"\x83\x65\x83\x58\x83\x67/";
        let sjis_file: &[u8] = b"\x83\x65\x83\x58\x83\x67/sample.unitypackage";
        write_zip(
            &archive,
            &[
                (sjis_dir, false, b""),
                (sjis_file, false, b"dummy"),
                ("フォルダ/readme.txt".as_bytes(), true, b"hi"),
                ("フォルダ/two.unitypackage".as_bytes(), false, b""),
                (b"/./loose\\name.txt", false, b"abc"),
            ],
        );
        let before = snapshot(&root);
        let listing = list_zip(&archive.display().to_string(), &root.display().to_string(), "").unwrap();
        assert_eq!(listing.library.as_deref(), Some(root.to_str().unwrap()));
        assert!(listing.remote.is_none());
        assert_eq!(
            listing.entries,
            vec![
                ZipEntryView { path: "テスト".into(), dir: true, size: 0 },
                ZipEntryView { path: "テスト/sample.unitypackage".into(), dir: false, size: 5 },
                ZipEntryView { path: "フォルダ/readme.txt".into(), dir: false, size: 2 },
                ZipEntryView { path: "フォルダ/two.unitypackage".into(), dir: false, size: 0 },
                ZipEntryView { path: "loose/name.txt".into(), dir: false, size: 3 },
            ]
        );
        assert!(!listing.truncated);
        assert_eq!(snapshot(&root), before, "listing a zip must not extract anything");

        std::fs::write(root.join("broken.zip"), b"not a zip").unwrap();
        assert!(list_zip(&root.join("broken.zip").display().to_string(), "", "").is_err());
        std::fs::write(root.join("plain.txt"), b"x").unwrap();
        assert!(list_zip(&root.join("plain.txt").display().to_string(), "", "").is_err());
        assert!(list_zip(&root.join("missing.zip").display().to_string(), "", "").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn zip_names_fall_back_when_not_shift_jis() {
        assert_eq!(decode_zip_name(b"plain.txt", "plain.txt"), "plain.txt");
        assert_eq!(decode_zip_name("ä".as_bytes(), "fallback"), "ä");
        assert_eq!(decode_zip_name(b"\xff\xff", "cp437"), "cp437");
        assert_eq!(clean_zip_path("a//b/./c/"), "a/b/c");
    }
}
