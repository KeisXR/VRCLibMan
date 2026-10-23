use crate::model::{DownloadFact, FilePlace, LocalFile, PlacedDownload};
use crate::view::Snapshot;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// A disconnected UNC or SMB path can block `metadata` for a long time.
/// Callers wait at most this long and leave the blocked read on its own thread.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Probe {
    Present,
    Absent,
    Unreachable,
}

pub(crate) fn classify(local_present: bool, remote_present: bool) -> FilePlace {
    if local_present {
        FilePlace::Local
    } else if remote_present {
        FilePlace::Remote
    } else {
        FilePlace::Missing
    }
}

pub(crate) fn valid_booth_id(value: &str) -> bool {
    (4..=12).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn safe_filename(value: &str) -> Option<String> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 180 {
        return None;
    }
    if name == "." || name == ".." {
        return None;
    }
    if name.chars().any(|ch| {
        matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control()
    }) {
        return None;
    }
    Some(name.to_string())
}

pub(crate) fn clean_variation(value: &str) -> Option<String> {
    let name = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 80 {
        return None;
    }
    if name
        .chars()
        .any(|ch| matches!(ch, '/' | '\\' | '\0') || ch.is_control())
    {
        return None;
    }
    Some(name)
}

fn staged_path_error() -> String {
    format!(
        "移動できるのは、ダウンロードフォルダの VRCLibMan{0}商品ID{0}ファイル名 だけです",
        std::path::MAIN_SEPARATOR
    )
}

#[cfg(test)]
std::thread_local! {
    static TEST_DOWNLOADS: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_test_downloads(path: Option<PathBuf>) {
    TEST_DOWNLOADS.with(|slot| *slot.borrow_mut() = path);
}

pub(crate) fn downloads_folder() -> Result<PathBuf, String> {
    #[cfg(test)]
    if let Some(path) = TEST_DOWNLOADS.with(|slot| slot.borrow().clone()) {
        return Ok(path);
    }
    known_downloads_folder()
}

#[cfg(windows)]
fn known_downloads_folder() -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStringExt;

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }
    // FOLDERID_Downloads {374DE290-123F-4565-9164-39C4925E467B}
    const FOLDERID_DOWNLOADS: Guid = Guid {
        data1: 0x374D_E290,
        data2: 0x123F,
        data3: 0x4565,
        data4: [0x91, 0x64, 0x39, 0xC4, 0x92, 0x5E, 0x46, 0x7B],
    };
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            rfid: *const Guid,
            flags: u32,
            token: isize,
            path: *mut *mut u16,
        ) -> i32;
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(pointer: *mut std::ffi::c_void);
    }
    // SAFETY: SHGetKnownFolderPath writes a CoTaskMem-allocated wide string on success.
    // The pointer is freed with CoTaskMemFree before this function returns.
    unsafe {
        let mut raw = std::ptr::null_mut();
        let code = SHGetKnownFolderPath(&FOLDERID_DOWNLOADS, 0, 0, &mut raw);
        if code != 0 || raw.is_null() {
            return Err("ダウンロードフォルダを確認できません".into());
        }
        let mut length = 0usize;
        while *raw.add(length) != 0 {
            length += 1;
        }
        let text = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, length));
        CoTaskMemFree(raw.cast());
        Ok(PathBuf::from(text))
    }
}

#[cfg(not(windows))]
fn known_downloads_folder() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "ダウンロードフォルダを確認できません".to_string())?;
    Ok(PathBuf::from(home).join("Downloads"))
}

fn component_key(text: &str) -> String {
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_string()
    }
}

fn path_keys(path: &Path) -> Result<Vec<String>, ()> {
    let mut keys = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir | Component::ParentDir => return Err(()),
            Component::RootDir => {}
            Component::Prefix(prefix) => {
                let raw = prefix.as_os_str().to_string_lossy();
                let text = raw.trim_start_matches(r"\\?\");
                if !text.is_empty() {
                    keys.push(component_key(text));
                }
            }
            Component::Normal(name) => keys.push(component_key(&name.to_string_lossy())),
        }
    }
    Ok(keys)
}

fn same_path(left: &Path, right: &Path) -> bool {
    path_keys(left).ok() == path_keys(right).ok()
}

/// Windows `canonicalize` returns `\\?\` verbatim paths. `path_keys` already
/// ignores that prefix when it compares paths. The download target needs the
/// plain form, because WebView2 mishandles the verbatim prefix.
///
/// `\\?\C:\...` becomes `C:\...`, and `\\?\UNC\server\share\...` becomes
/// `\\server\share\...`. A path without that prefix is unchanged, so this is a
/// no-op for ordinary Linux canonical paths.
pub(crate) fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn normal_names(path: &Path) -> Result<Vec<std::ffi::OsString>, ()> {
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir | Component::ParentDir => return Err(()),
            Component::Normal(name) => names.push(name.to_os_string()),
            _ => {}
        }
    }
    Ok(names)
}

/// `parent` is already canonical. The child must be a real directory or file,
/// not a symlink or junction, and its canonical path must stay inside `parent`.
fn open_child(parent: &Path, name: &std::ffi::OsStr, directory: bool) -> Result<PathBuf, String> {
    let text = name.to_string_lossy();
    if name.is_empty() || text == "." || text == ".." || text.contains(['/', '\\']) {
        return Err(staged_path_error());
    }
    let joined = parent.join(name);
    let meta = std::fs::symlink_metadata(&joined).map_err(|_| staged_path_error())?;
    if meta.file_type().is_symlink() {
        return Err("シンボリックリンクは移動しません".into());
    }
    if directory && !meta.is_dir() {
        return Err(staged_path_error());
    }
    if !directory && !meta.is_file() {
        return Err("ダウンロードしたファイルが見つかりません".into());
    }
    let canon = std::fs::canonicalize(&joined).map_err(|_| staged_path_error())?;
    let Some(canon_parent) = canon.parent() else {
        return Err(staged_path_error());
    };
    if !same_path(canon_parent, parent) {
        return Err(staged_path_error());
    }
    Ok(canon)
}

/// Accept only `{downloads}/VRCLibMan/{booth_id}/{filename}` after canonicalization.
/// A `VRCLibMan` folder anywhere else, a different product id, a subdirectory,
/// `..`, or a symlink that leaves the folder is rejected.
pub(crate) fn staged_download_ok(
    source: &Path,
    downloads: &Path,
    booth_id: &str,
) -> Result<PathBuf, String> {
    if !source.is_absolute() || !downloads.is_absolute() {
        return Err("ダウンロードのパスが絶対パスではありません".into());
    }
    if !valid_booth_id(booth_id) {
        return Err("商品IDを特定できません".into());
    }
    let filename = safe_filename(
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
    .ok_or_else(|| "ファイル名を読めません".to_string())?;
    let source_keys = path_keys(source).map_err(|_| staged_path_error())?;
    let root_keys = path_keys(downloads).map_err(|_| staged_path_error())?;
    let tail = [
        component_key("VRCLibMan"),
        component_key(booth_id),
        component_key(&filename),
    ];
    if source_keys.len() != root_keys.len() + tail.len()
        || source_keys[..root_keys.len()] != root_keys[..]
        || source_keys[root_keys.len()..] != tail[..]
    {
        return Err(staged_path_error());
    }
    let source_names = normal_names(source).map_err(|_| staged_path_error())?;
    let root_names = normal_names(downloads).map_err(|_| staged_path_error())?;
    let root = std::fs::canonicalize(downloads).map_err(|_| staged_path_error())?;
    let staging = open_child(&root, &source_names[root_names.len()], true)?;
    let product_dir = open_child(&staging, &source_names[root_names.len() + 1], true)?;
    let file = open_child(&product_dir, &source_names[root_names.len() + 2], false)?;
    let source_canon = std::fs::canonicalize(source).map_err(|_| staged_path_error())?;
    if !same_path(&source_canon, &file) {
        return Err(staged_path_error());
    }
    Ok(file)
}

/// Create `{downloads}/VRCLibMan/{booth_id}` as real directories.
/// A symlink or junction at either level is refused before anything is written through it.
pub(crate) fn prepare_staging_dir(downloads: &Path, booth_id: &str) -> Result<PathBuf, String> {
    if !downloads.is_absolute() {
        return Err("ダウンロードのパスが絶対パスではありません".into());
    }
    if !valid_booth_id(booth_id) {
        return Err("商品IDを特定できません".into());
    }
    let root = std::fs::canonicalize(downloads).map_err(|_| staged_path_error())?;
    let staging = ensure_real_dir(&root, "VRCLibMan")?;
    let product = ensure_real_dir(&staging, booth_id)?;
    Ok(without_verbatim_prefix(&product))
}

fn ensure_real_dir(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(staged_path_error());
    }
    let joined = parent.join(name);
    match std::fs::symlink_metadata(&joined) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err("シンボリックリンクは移動しません".into());
            }
            if !meta.is_dir() {
                return Err(staged_path_error());
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&joined).map_err(|err| format!("保存先を作れません: {err}"))?;
            let meta = std::fs::symlink_metadata(&joined).map_err(|_| staged_path_error())?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                let _ = std::fs::remove_dir(&joined);
                return Err("シンボリックリンクは移動しません".into());
            }
        }
        Err(_) => return Err(staged_path_error()),
    }
    let canon = std::fs::canonicalize(&joined).map_err(|_| staged_path_error())?;
    let Some(canon_parent) = canon.parent() else {
        return Err(staged_path_error());
    };
    if !same_path(canon_parent, parent) {
        return Err(staged_path_error());
    }
    Ok(canon)
}

pub(crate) fn filename_is_number(name: &str) -> bool {
    let name = name.trim();
    (4..=12).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn is_packaged_asset(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ["unitypackage", "zip", "rar", "7z", "vrm"]
        .iter()
        .any(|ext| {
            let suffix = format!(".{ext}");
            match lower.strip_suffix(&suffix) {
                Some(stem) => !stem.is_empty() && !stem.ends_with(' ') && !stem.ends_with('.'),
                None => false,
            }
        })
}

/// Prefer the row's real filename. A number-only row takes the download's suggested name.
/// The result is a single path segment, never a directory.
pub(crate) fn choose_download_name(row_filename: &str, suggested: &str) -> String {
    let suggested = suggested.split(['?', '#']).next().unwrap_or("");
    let suggested = suggested.rsplit(['/', '\\']).next().unwrap_or("");
    let row = safe_filename(row_filename).unwrap_or_default();
    let suggested = safe_filename(suggested).unwrap_or_default();
    if is_packaged_asset(&row) && !filename_is_number(&row) {
        return row;
    }
    if is_packaged_asset(&suggested) {
        return suggested;
    }
    if is_packaged_asset(&row) {
        return row;
    }
    "received.bin".to_string()
}

pub(crate) fn looks_like_login_page(bytes: &[u8]) -> bool {
    let bytes = bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF])
        .unwrap_or(bytes);
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text
        .trim_start()
        .trim_start_matches('\u{feff}')
        .to_ascii_lowercase();
    trimmed.starts_with("<!doctype")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<head")
}

pub(crate) fn file_looks_like_login_page(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 1024];
    let Ok(read) = std::io::Read::read(&mut file, &mut buf) else {
        return false;
    };
    looks_like_login_page(&buf[..read])
}

pub(crate) fn probe_path(path: &Path, timeout: Duration) -> Probe {
    let path = path.to_path_buf();
    wait_for(timeout, move || match std::fs::metadata(&path) {
        Ok(_) => Probe::Present,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Probe::Absent,
        Err(_) => Probe::Unreachable,
    })
    .unwrap_or(Probe::Unreachable)
}

pub(crate) fn probe_file(path: &Path, timeout: Duration) -> Probe {
    let path = path.to_path_buf();
    wait_for(timeout, move || match std::fs::metadata(&path) {
        Ok(meta) if meta.is_file() => Probe::Present,
        Ok(_) => Probe::Absent,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Probe::Absent,
        Err(_) => Probe::Unreachable,
    })
    .unwrap_or(Probe::Unreachable)
}

fn wait_for<T>(timeout: Duration, work: impl FnOnce() -> T + Send + 'static) -> Option<T>
where
    T: Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver.recv_timeout(timeout).ok()
}

fn create_dir_timed(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    match wait_for(PROBE_TIMEOUT, move || std::fs::create_dir_all(&path)) {
        None => Err("保存先に届きません。接続を確認してからもう一度".into()),
        Some(Ok(())) => Ok(()),
        Some(Err(err)) => Err(format!("保存先を作れません: {err}")),
    }
}

fn canonicalize_timed(path: &Path) -> Result<PathBuf, String> {
    let path = path.to_path_buf();
    match wait_for(PROBE_TIMEOUT, move || std::fs::canonicalize(&path)) {
        None => Err("保存先に届きません。接続を確認してからもう一度".into()),
        Some(Ok(path)) => Ok(path),
        Some(Err(err)) => Err(format!("保存先を確認できません: {err}")),
    }
}

fn lexical_parts(path: &Path) -> Result<Vec<std::ffi::OsString>, ()> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => return Err(()),
            Component::Normal(part) => parts.push(part.to_os_string()),
            Component::RootDir | Component::Prefix(_) => {
                parts.push(component.as_os_str().to_os_string())
            }
        }
    }
    Ok(parts)
}

fn lexical_inside(root: &Path, dest: &Path) -> Result<(), ()> {
    let root_parts = lexical_parts(root)?;
    let dest_parts = lexical_parts(dest)?;
    if dest_parts.len() <= root_parts.len() {
        return Err(());
    }
    if root_parts
        .iter()
        .zip(dest_parts.iter())
        .all(|(left, right)| left == right)
    {
        Ok(())
    } else {
        Err(())
    }
}

fn unique_path(path: &Path) -> PathBuf {
    if probe_path(path, PROBE_TIMEOUT) != Probe::Present {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|name| name.to_str());
    let parent = path.parent().unwrap_or(Path::new("."));
    for index in 2..50 {
        let name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = parent.join(name);
        if probe_path(&candidate, PROBE_TIMEOUT) != Probe::Present {
            return candidate;
        }
    }
    parent.join(format!("{stem}-copy"))
}

pub(crate) fn library_destination(
    library: &Path,
    booth_id: &str,
    filename: &str,
) -> Result<PathBuf, String> {
    if !library.is_absolute() {
        return Err("ライブラリのフォルダが絶対パスではありません".into());
    }
    if !valid_booth_id(booth_id) {
        return Err("商品IDを特定できません".into());
    }
    let filename = safe_filename(filename).ok_or_else(|| "ファイル名を読めません".to_string())?;
    let dest = library.join(booth_id).join(filename);
    lexical_inside(library, &dest).map_err(|_| "保存先がライブラリの外です".to_string())?;
    Ok(dest)
}

pub(crate) fn place_staged_download(
    source: &Path,
    downloads: &Path,
    library: &Path,
    booth_id: &str,
) -> Result<PathBuf, String> {
    let source = staged_download_ok(source, downloads, booth_id)?;
    let filename = safe_filename(
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
    .ok_or_else(|| "ファイル名を読めません".to_string())?;
    place_verified_file(&source, library, booth_id, &filename)
}

pub(crate) fn place_into_library(
    source: &Path,
    library: Option<&Path>,
    booth_id: &str,
) -> Result<PathBuf, String> {
    let Some(library) = library.filter(|path| !path.as_os_str().is_empty()) else {
        return Err("ライブラリのフォルダを選んでから、振り分けをオンにしてください".into());
    };
    let downloads = downloads_folder()?;
    place_staged_download(source, &downloads, library, booth_id)
}

fn place_verified_file(
    source: &Path,
    library: &Path,
    booth_id: &str,
    filename: &str,
) -> Result<PathBuf, String> {
    let dest = library_destination(library, booth_id, &filename)?;
    if probe_path(&dest, PROBE_TIMEOUT) == Probe::Present {
        if let (Ok(source_canon), Ok(dest_canon)) =
            (canonicalize_timed(source), canonicalize_timed(&dest))
        {
            if source_canon == dest_canon {
                return Ok(dest);
            }
        }
    }
    create_dir_timed(library)?;
    let parent = dest
        .parent()
        .ok_or_else(|| "保存先を作れません".to_string())?;
    create_dir_timed(parent)?;
    let root = canonicalize_timed(library)?;
    let parent_canon = canonicalize_timed(parent)?;
    if !parent_canon.starts_with(&root) {
        return Err("保存先がライブラリの外です".into());
    }
    let dest = unique_path(&dest);
    move_file(source, &dest)?;
    Ok(dest)
}

fn move_file(source: &Path, dest: &Path) -> Result<(), String> {
    if std::fs::rename(source, dest).is_ok() {
        return Ok(());
    }
    std::fs::copy(source, dest).map_err(|err| format!("ファイルを移動できません: {err}"))?;
    std::fs::remove_file(source).map_err(|err| format!("移動元を消せません: {err}"))?;
    Ok(())
}

#[derive(Debug)]
pub(crate) enum RemoteHit {
    Found(PathBuf),
    Missing,
    Unreachable,
}

pub(crate) fn locate_remote(root: &Path, booth_id: &str, filename: &str) -> RemoteHit {
    if !valid_booth_id(booth_id) || safe_filename(filename).is_none() {
        return RemoteHit::Missing;
    }
    match probe_path(root, PROBE_TIMEOUT) {
        Probe::Present => {}
        Probe::Absent | Probe::Unreachable => return RemoteHit::Unreachable,
    }
    for candidate in [root.join(booth_id).join(filename), root.join(filename)] {
        if lexical_inside(root, &candidate).is_err() {
            continue;
        }
        match probe_file(&candidate, PROBE_TIMEOUT) {
            Probe::Present => return RemoteHit::Found(candidate),
            Probe::Absent => {}
            Probe::Unreachable => return RemoteHit::Unreachable,
        }
    }
    RemoteHit::Missing
}

pub(crate) fn pull_file(
    remote_root: &Path,
    library: &Path,
    booth_id: &str,
    filename: &str,
) -> Result<PathBuf, String> {
    let source = match locate_remote(remote_root, booth_id, filename) {
        RemoteHit::Found(path) => path,
        RemoteHit::Missing => return Err("リモートにファイルがありません".into()),
        RemoteHit::Unreachable => {
            return Err("リモートのフォルダに届きません。接続を確認してからもう一度".into());
        }
    };
    let dest = library_destination(library, booth_id, filename)?;
    create_dir_timed(library)?;
    let parent = dest
        .parent()
        .ok_or_else(|| "保存先を作れません".to_string())?;
    create_dir_timed(parent)?;
    let root = canonicalize_timed(library)?;
    let parent_canon = canonicalize_timed(parent)?;
    if !parent_canon.starts_with(&root) {
        return Err("保存先がライブラリの外です".into());
    }
    copy_confirmed(&source, &dest)?;
    Ok(dest)
}

pub(crate) fn push_file(
    source: &Path,
    remote_root: &Path,
    booth_id: &str,
    filename: &str,
) -> Result<PathBuf, String> {
    if !remote_root.is_absolute() {
        return Err("リモートのフォルダが絶対パスではありません".into());
    }
    if probe_path(remote_root, PROBE_TIMEOUT) != Probe::Present {
        return Err("リモートのフォルダに届きません。接続を確認してからもう一度".into());
    }
    if !valid_booth_id(booth_id) {
        return Err("商品IDを特定できません".into());
    }
    let filename = safe_filename(filename).ok_or_else(|| "ファイル名を読めません".to_string())?;
    let dest = remote_root.join(booth_id).join(&filename);
    lexical_inside(remote_root, &dest).map_err(|_| "送信先がリモートの外です".to_string())?;
    let parent = dest
        .parent()
        .ok_or_else(|| "送信先を作れません".to_string())?;
    create_dir_timed(parent)?;
    let root = canonicalize_timed(remote_root)?;
    let parent_canon = canonicalize_timed(parent)?;
    if !parent_canon.starts_with(&root) {
        return Err("送信先がリモートの外です".into());
    }
    copy_confirmed(source, &dest)?;
    Ok(dest)
}

fn copy_confirmed(source: &Path, dest: &Path) -> Result<(), String> {
    match probe_file(source, PROBE_TIMEOUT) {
        Probe::Present => {}
        Probe::Absent => return Err("ファイルが見つかりません".into()),
        Probe::Unreachable => {
            return Err("保存先に届きません。接続を確認してからもう一度".into());
        }
    }
    std::fs::copy(source, dest).map_err(|err| format!("ファイルをコピーできません: {err}"))?;
    Ok(())
}

fn find_local(row: &DownloadFact, files: &[LocalFile]) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = &row.local_path {
        candidates.push(PathBuf::from(path));
    }
    for file in files {
        if file.is_dir || file.filename != row.filename {
            continue;
        }
        if file.linked_product_id.as_deref() != Some(row.booth_id.as_str()) {
            continue;
        }
        let path = PathBuf::from(&file.path);
        if !candidates.iter().any(|existing| existing == &path) {
            candidates.push(path);
        }
    }
    for path in candidates {
        match probe_file(&path, PROBE_TIMEOUT) {
            Probe::Present => return Some(path),
            Probe::Absent => {}
            Probe::Unreachable => return None,
        }
    }
    None
}

pub(crate) fn fill_local_places(snapshot: &mut Snapshot, remote_folder: &str) {
    fill_places_inner(snapshot, remote_folder, false);
}

pub(crate) fn fill_places(snapshot: &mut Snapshot, remote_folder: &str) {
    fill_places_inner(snapshot, remote_folder, true);
}

fn fill_places_inner(snapshot: &mut Snapshot, remote_folder: &str, probe_remote: bool) {
    let remote_folder = remote_folder.trim();
    snapshot.remote_configured = !remote_folder.is_empty();
    snapshot.remote_checked = !snapshot.remote_configured || probe_remote;
    let remote_root = Path::new(remote_folder);
    let mut reachable = probe_remote
        && snapshot.remote_configured
        && probe_path(remote_root, PROBE_TIMEOUT) == Probe::Present;
    let mut placed = Vec::new();
    for row in &snapshot.download_rows {
        let local = find_local(row, &snapshot.files);
        let mut remote_path = None;
        if probe_remote && reachable && local.is_none() {
            match locate_remote(remote_root, &row.booth_id, &row.filename) {
                RemoteHit::Found(path) => remote_path = Some(path),
                RemoteHit::Missing => {}
                RemoteHit::Unreachable => reachable = false,
            }
        }
        let state = classify(local.is_some(), remote_path.is_some());
        placed.push(PlacedDownload {
            booth_id: row.booth_id.clone(),
            variation: row.variation.clone(),
            filename: row.filename.clone(),
            size_bytes: row.size_bytes,
            state,
            local_path: local.map(|path| path.display().to_string()),
            remote_path: remote_path.map(|path| path.display().to_string()),
        });
    }
    snapshot.remote_reachable = reachable;
    snapshot.placed_downloads = placed;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn file_place_prefers_local_then_remote() {
        assert_eq!(classify(true, false), FilePlace::Local);
        assert_eq!(classify(true, true), FilePlace::Local);
        assert_eq!(classify(false, true), FilePlace::Remote);
        assert_eq!(classify(false, false), FilePlace::Missing);
    }

    #[test]
    fn blocked_probe_returns_before_the_call_finishes() {
        let started = Instant::now();
        let result = wait_for(Duration::from_millis(80), || {
            std::thread::sleep(Duration::from_millis(600));
            Probe::Present
        });
        assert!(result.is_none());
        assert!(started.elapsed() < Duration::from_millis(400));
    }

    fn fixture_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("vrclibman-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn write_file(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn staged_download_accepts_only_the_known_folder_product_file() {
        let root = fixture_root("intake");
        let downloads = root.join("Downloads");
        let library = root.join("library");
        std::fs::create_dir_all(&library).unwrap();
        let good = downloads
            .join("VRCLibMan")
            .join("900001")
            .join("body.unitypackage");
        write_file(&good, b"pkg");
        let unrelated = root
            .join("unrelated")
            .join("VRCLibMan")
            .join("999999")
            .join("body.unitypackage");
        write_file(&unrelated, b"other");
        let documents = root
            .join("Documents")
            .join("VRCLibMan")
            .join("900001")
            .join("body.unitypackage");
        write_file(&documents, b"repo");
        let executable = root.join("Local").join("VRCLibMan").join("VRCLibMan.exe");
        write_file(&executable, b"exe");
        let nested = downloads
            .join("VRCLibMan")
            .join("900001")
            .join("extra")
            .join("body.unitypackage");
        write_file(&nested, b"nested");
        let wrong_id = downloads
            .join("VRCLibMan")
            .join("999999")
            .join("body.unitypackage");
        write_file(&wrong_id, b"wrong");
        let escaped = downloads
            .join("VRCLibMan")
            .join("900001")
            .join("..")
            .join("900001")
            .join("body.unitypackage");

        assert!(staged_download_ok(&good, &downloads, "900001").is_ok());
        assert!(staged_download_ok(&unrelated, &downloads, "900001").is_err());
        assert!(staged_download_ok(&unrelated, &downloads, "999999").is_err());
        assert!(staged_download_ok(&documents, &downloads, "900001").is_err());
        assert!(staged_download_ok(&executable, &downloads, "900001").is_err());
        assert!(staged_download_ok(&nested, &downloads, "900001").is_err());
        assert!(staged_download_ok(&wrong_id, &downloads, "900001").is_err());
        assert!(staged_download_ok(&escaped, &downloads, "900001").is_err());
        assert!(
            staged_download_ok(
                Path::new("Downloads/VRCLibMan/900001/body.unitypackage"),
                &downloads,
                "900001"
            )
            .is_err()
        );
        assert!(safe_filename("../secret.unitypackage").is_none());
        assert!(clean_variation("../通常").is_none());

        let placed = place_staged_download(&good, &downloads, &library, "900001").unwrap();
        assert!(!good.exists());
        assert_eq!(placed, library.join("900001").join("body.unitypackage"));
        assert_eq!(std::fs::read(&placed).unwrap(), b"pkg");
        assert!(unrelated.exists());
        assert!(documents.exists());
        assert!(executable.exists());
        assert!(place_into_library(&good, None, "900001").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lowercase_vrclibman_folder_matches_only_on_windows() {
        let root = fixture_root("case");
        let downloads = root.join("Downloads");
        let lower = downloads
            .join("vrclibman")
            .join("900001")
            .join("body.unitypackage");
        write_file(&lower, b"pkg");
        let result = staged_download_ok(&lower, &downloads, "900001");
        if cfg!(windows) {
            assert!(result.is_ok(), "{}", result.unwrap_err());
        } else {
            assert!(result.is_err());
            assert!(lower.exists());
        }
        let exact = downloads
            .join("VRCLibMan")
            .join("900001")
            .join("Body.unitypackage");
        write_file(&exact, b"Exact");
        assert!(staged_download_ok(&exact, &downloads, "900001").is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn staged_download_rejects_windows_symlinks_when_creation_is_allowed() {
        let root = fixture_root("winlink");
        let downloads = root.join("Downloads");
        let outside = root.join("outside.unitypackage");
        write_file(&outside, b"secret");
        let link = downloads
            .join("VRCLibMan")
            .join("900001")
            .join("body.unitypackage");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        if let Err(err) = std::os::windows::fs::symlink_file(&outside, &link) {
            let _ = std::fs::remove_dir_all(&root);
            if err.raw_os_error() == Some(1314) {
                return;
            }
            panic!("{err}");
        }
        assert!(staged_download_ok(&link, &downloads, "900001").is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn staged_download_rejects_symlinks_that_leave_the_folder() {
        let root = fixture_root("link");
        let downloads = root.join("Downloads");
        let outside = root.join("outside.unitypackage");
        write_file(&outside, b"secret");
        let link = downloads
            .join("VRCLibMan")
            .join("900001")
            .join("body.unitypackage");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let error = staged_download_ok(&link, &downloads, "900001").unwrap_err();
        assert!(error.contains("シンボリックリンク") || error.contains("VRCLibMan"));
        assert_eq!(std::fs::read(&outside).unwrap(), b"secret");

        let pocket = root.join("pocket");
        write_file(&pocket.join("900001").join("body.unitypackage"), b"pocket");
        let linked_root = root.join("linked-downloads");
        std::fs::create_dir_all(&linked_root).unwrap();
        std::os::unix::fs::symlink(&pocket, linked_root.join("VRCLibMan")).unwrap();
        let via = linked_root
            .join("VRCLibMan")
            .join("900001")
            .join("body.unitypackage");
        assert!(staged_download_ok(&via, &linked_root, "900001").is_err());
        assert!(pocket.join("900001").join("body.unitypackage").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_outside_the_library_is_refused() {
        let root = std::env::temp_dir().join(format!("vrclibman-escape-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = root.join("library");
        let outside = root.join("outside");
        let downloads = root.join("Downloads");
        let staging = downloads.join("VRCLibMan").join("424242");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::os::unix::fs::symlink(&outside, library.join("424242")).unwrap();
        let source = staging.join("body.unitypackage");
        std::fs::write(&source, b"pkg").unwrap();
        let err = place_staged_download(&source, &downloads, &library, "424242").unwrap_err();
        assert!(err.contains("外"));
        assert!(source.exists());
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remote_lookup_finds_a_booth_folder_and_reports_a_missing_root() {
        let root = std::env::temp_dir().join(format!("vrclibman-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("424242")).unwrap();
        let file = root.join("424242").join("body.unitypackage");
        std::fs::write(&file, b"pkg").unwrap();
        match locate_remote(&root, "424242", "body.unitypackage") {
            RemoteHit::Found(path) => assert_eq!(path, file),
            other => panic!("expected the file, got {other:?}"),
        }
        assert!(matches!(
            locate_remote(&root, "424242", "missing.unitypackage"),
            RemoteHit::Missing
        ));
        let started = Instant::now();
        assert!(matches!(
            locate_remote(
                Path::new("/tmp/vrclibman-no-such-remote-root"),
                "424242",
                "body.unitypackage"
            ),
            RemoteHit::Unreachable
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pull_and_push_copy_only_the_named_file() {
        let root = std::env::temp_dir().join(format!("vrclibman-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let remote = root.join("remote");
        let library = root.join("library");
        std::fs::create_dir_all(remote.join("424242")).unwrap();
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(remote.join("424242").join("body.unitypackage"), b"remote").unwrap();
        let pulled = pull_file(&remote, &library, "424242", "body.unitypackage").unwrap();
        assert_eq!(std::fs::read(&pulled).unwrap(), b"remote");
        assert!(remote.join("424242").join("body.unitypackage").exists());
        std::fs::write(&pulled, b"local").unwrap();
        let pushed = push_file(&pulled, &remote, "424242", "body.unitypackage").unwrap();
        assert_eq!(std::fs::read(pushed).unwrap(), b"local");
        assert_eq!(std::fs::read(&pulled).unwrap(), b"local");
        let dead = Instant::now();
        let err = pull_file(
            Path::new("/tmp/vrclibman-no-such-remote-root"),
            &library,
            "424242",
            "body.unitypackage",
        )
        .unwrap_err();
        assert!(err.contains("届きません"));
        assert!(dead.elapsed() < Duration::from_secs(1));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn staging_dir_is_only_the_product_folder_under_downloads() {
        let root = fixture_root("stage");
        let downloads = root.join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let dir = prepare_staging_dir(&downloads, "424242").unwrap();
        assert_eq!(dir, downloads.join("VRCLibMan").join("424242"));
        assert_eq!(prepare_staging_dir(&downloads, "424242").unwrap(), dir);
        assert!(prepare_staging_dir(&downloads, "..").is_err());
        assert!(prepare_staging_dir(&downloads, "42").is_err());
        assert!(prepare_staging_dir(Path::new("Downloads"), "424242").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_verbatim_prefix_normalizes_without_canonicalize() {
        assert_eq!(
            without_verbatim_prefix(Path::new(
                r"\\?\C:\Users\someone\Downloads\VRCLibMan\424242"
            )),
            PathBuf::from(r"C:\Users\someone\Downloads\VRCLibMan\424242")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\UNC\server\share\VRCLibMan\424242")),
            PathBuf::from(r"\\server\share\VRCLibMan\424242")
        );
        let plain = PathBuf::from(r"C:\Users\someone\Downloads\VRCLibMan\424242");
        assert_eq!(without_verbatim_prefix(&plain), plain);
        let unix = PathBuf::from("/tmp/Downloads/VRCLibMan/424242");
        assert_eq!(without_verbatim_prefix(&unix), unix);
    }

    #[cfg(unix)]
    #[test]
    fn staging_dir_does_not_follow_a_linked_folder() {
        let root = fixture_root("stage-link");
        let downloads = root.join("Downloads");
        let pocket = root.join("pocket");
        std::fs::create_dir_all(&downloads).unwrap();
        std::fs::create_dir_all(&pocket).unwrap();
        std::os::unix::fs::symlink(&pocket, downloads.join("VRCLibMan")).unwrap();
        assert!(prepare_staging_dir(&downloads, "424242").is_err());
        assert!(!pocket.join("424242").exists());
        std::fs::remove_file(downloads.join("VRCLibMan")).unwrap();
        let product = downloads.join("VRCLibMan").join("424242");
        std::fs::create_dir_all(downloads.join("VRCLibMan")).unwrap();
        std::os::unix::fs::symlink(&pocket, &product).unwrap();
        assert!(prepare_staging_dir(&downloads, "424242").is_err());
        assert!(std::fs::read_dir(&pocket).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn download_name_keeps_a_real_row_name_and_a_number_takes_the_suggestion() {
        assert_eq!(
            choose_download_name("body.unitypackage", "other.zip"),
            "body.unitypackage"
        );
        assert_eq!(
            choose_download_name("2336025", "folder/body.unitypackage?token=secret"),
            "body.unitypackage"
        );
        assert_eq!(choose_download_name("2336025", "../secret.unitypackage"), "secret.unitypackage");
        assert_eq!(choose_download_name("2336025", "login.html"), "received.bin");
        assert_eq!(choose_download_name("body .unitypackage", "pack.zip"), "pack.zip");
        assert!(!is_packaged_asset(".zip"));
        assert!(!is_packaged_asset("note.txt"));
        assert!(looks_like_login_page(b"  <!DOCTYPE html><html><head></head>"));
        assert!(looks_like_login_page(b"\xEF\xBB\xBF<html>"));
        assert!(looks_like_login_page(b"<head><title>login</title>"));
        assert!(!looks_like_login_page(b"PK\x03\x04dummy"));
        assert!(!looks_like_login_page(&[0x1f, 0x8b, 0x08, 0x00]));
        assert!(!looks_like_login_page(b"{ \"asset\": true }"));
    }
}
