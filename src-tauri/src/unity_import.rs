//! 「Unityで開く」: zip の中の `.unitypackage` を一覧し、選んだものだけを取り出して Unity に渡す。
//!
//! - zip は展開しない。中央ディレクトリを読み、選ばれた `.unitypackage` だけを
//!   アプリのキャッシュ `unity-import/<商品ID>/` に取り出す。
//! - 取り出し先のファイル名は zip 内の名前の最後の部分だけを使い、使えない文字を置き換える。
//!   `..` や絶対パスを含む名前でも、取り出し先フォルダの外には書かない（zip slip 対策）。
//! - 1 件の大きさに上限がある。宣言より大きいデータは途中で打ち切る。
//! - キャッシュの古いものは起動時に消す。ライブラリのファイルは読むだけで、書き換えない。
//! - Unity は `<Unity> -openfile <pkg>` で起動するだけ。シェルを通さず、引数は配列で渡す。
//!   `-importPackage` やプロジェクトへの書き込みはしない。
//! - パスやファイル名はログに書かない。

use crate::file_browser::{optional_root, read_zip, requested_path, resolve, NO_ANSWER};
use crate::library_files::wait_for;
use serde::Serialize;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

/// 取り出す 1 件の上限。大きなアバター本体でも数 GB に収まる。
pub(crate) const MAX_PACKAGE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// キャッシュのフォルダ名。アプリのキャッシュフォルダの下に作る。
pub(crate) const CACHE_DIR: &str = "unity-import";
/// 起動時に消す古さ。
pub(crate) const CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// 起動中のエディタへ渡す Unity のプロセスは、渡し終えるとすぐ終わる。
/// この時間を過ぎても終わらないときは、そのプロセスがエディタ本体として起動したとみなし、
/// 残りは利用者が「続きを渡す」を押してから渡す。
pub(crate) const HANDOFF_TIMEOUT: Duration = Duration::from_secs(20);
/// 続けて渡すときの間隔。Import 画面が順に出るように少し空ける。
pub(crate) const HANDOFF_GAP: Duration = Duration::from_millis(1200);

const NOT_PACKAGE: &str = "zip か .unitypackage のファイルを選んでください";
const OUTSIDE: &str = "ライブラリ（またはリモートライブラリ）の中のファイルだけを Unity で開けます";
const NO_EDITOR: &str =
    "既定の Unity が設定されていません。設定の「Unity」で選ぶか、関連付けで開いてください";
const EDITOR_MISSING: &str =
    "設定した Unity が見つかりません。設定の「Unity」で選び直すか、関連付けで開いてください";
const EDITOR_NOT_RUNNING: &str =
    "Unity が起動していません。設定した Unity でプロジェクトを開いてから、もう一度押してください";
const OTHER_EDITOR_RUNNING: &str = "起動中の Unity は、設定した Unity と別のバージョンです。設定した Unity でプロジェクトを開くか、設定の「Unity」で起動中のバージョンを選んでください";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnityPackageEntry {
    /// zip 内のパス（`/` 区切り）。単体の `.unitypackage` では空文字。
    pub entry: String,
    /// 表示するファイル名。
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnityPackageList {
    /// 解決したパス。
    pub path: String,
    /// "zip" か "package"。
    pub kind: &'static str,
    pub packages: Vec<UnityPackageEntry>,
    /// zip の項目が多く、一部だけを読んだ。
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedPackage {
    pub name: String,
    /// Unity に渡すファイル。単体はライブラリ内のもの、zip 内はキャッシュに取り出したもの。
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaunchResult {
    /// 渡し終えた件数。先頭から数える。
    pub launched: usize,
    /// Unity の起動を待つため、ここで止めた。残りは `launched` 件目から。
    pub waiting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchMode {
    /// 設定の既定 Unity で `-openfile`。
    Unity,
    /// OS の関連付け（ダブルクリックと同じ）。
    System,
}

impl LaunchMode {
    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "unity" => Ok(LaunchMode::Unity),
            "system" => Ok(LaunchMode::System),
            _ => Err("開き方が正しくありません".into()),
        }
    }
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|value| value.to_string_lossy().eq_ignore_ascii_case(ext))
}

fn is_package_name(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".unitypackage")
}

/// 受け付けるのは、ライブラリかリモートライブラリの中にある zip と `.unitypackage` だけ。
pub(crate) fn allowed_source(raw: &str, library: &str, remote: &str) -> Result<PathBuf, String> {
    if raw.trim().is_empty() {
        return Err(NOT_PACKAGE.into());
    }
    let requested = requested_path(raw, "")?;
    if !has_extension(&requested, "zip") && !has_extension(&requested, "unitypackage") {
        return Err(NOT_PACKAGE.into());
    }
    let resolved = resolve(&requested)?;
    if !resolved.is_file() {
        return Err(NOT_PACKAGE.into());
    }
    let roots = [optional_root(library), optional_root(remote)];
    if !roots
        .iter()
        .flatten()
        .any(|root| resolved.starts_with(root) && resolved != *root)
    {
        return Err(OUTSIDE.into());
    }
    Ok(resolved)
}

fn last_part(entry: &str) -> &str {
    entry.rsplit(['/', '\\']).next().unwrap_or(entry)
}

/// zip または単体の `.unitypackage` から、Unity に渡せるものを一覧する。展開しない。
pub(crate) fn list_packages(path: &Path) -> Result<UnityPackageList, String> {
    let display = path.display().to_string();
    if has_extension(path, "unitypackage") {
        let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Ok(UnityPackageList {
            path: display,
            kind: "package",
            packages: vec![UnityPackageEntry { entry: String::new(), name, size }],
            truncated: false,
        });
    }
    let read_from = path.to_path_buf();
    let (entries, truncated) = match wait_for(crate::file_browser::LIST_TIMEOUT, move || read_zip(read_from)) {
        None => return Err(NO_ANSWER.into()),
        Some(result) => result?,
    };
    let packages = entries
        .into_iter()
        .filter(|entry| !entry.dir && is_package_name(&entry.path))
        .map(|entry| UnityPackageEntry {
            name: last_part(&entry.path).to_string(),
            entry: entry.path,
            size: entry.size,
        })
        .collect();
    Ok(UnityPackageList { path: display, kind: "zip", packages, truncated })
}

/// 取り出し先のファイル名。zip 内のパスの最後の部分だけを使い、Windows で使えない文字を置き換える。
pub(crate) fn safe_file_name(entry: &str) -> Option<String> {
    let base = last_part(entry);
    let mut cleaned: String = base
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    if !is_package_name(&cleaned) || cleaned.len() <= ".unitypackage".len() {
        return None;
    }
    // 長すぎる名前は先頭を残して縮める（拡張子は残す）。
    const MAX_CHARS: usize = 120;
    if cleaned.chars().count() > MAX_CHARS {
        let stem: String = cleaned.chars().take(MAX_CHARS - ".unitypackage".len()).collect();
        cleaned = format!("{}.unitypackage", stem.trim_end_matches(['.', ' ']));
    }
    Some(cleaned)
}

/// `unity-import/<商品ID>/`。商品 ID が数字でなければ `other`。
pub(crate) fn cache_folder(cache_root: &Path, booth_id: Option<&str>) -> PathBuf {
    let id = booth_id
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.len() <= 12 && id.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or("other");
    cache_root.join(CACHE_DIR).join(id)
}

fn unique_name(taken: &mut Vec<String>, name: String) -> String {
    let lower = |value: &str| value.to_lowercase();
    if !taken.iter().any(|seen| lower(seen) == lower(&name)) {
        taken.push(name.clone());
        return name;
    }
    let stem = &name[..name.len() - ".unitypackage".len()];
    for index in 2.. {
        let candidate = format!("{stem} ({index}).unitypackage");
        if !taken.iter().any(|seen| lower(seen) == lower(&candidate)) {
            taken.push(candidate.clone());
            return candidate;
        }
    }
    unreachable!()
}

/// 選ばれた項目だけを `dest` に取り出す。`entries` は `list_packages` が返した `entry`。
pub(crate) fn extract_packages(
    zip_path: &Path,
    entries: &[String],
    dest: &Path,
    max_bytes: u64,
) -> Result<Vec<PreparedPackage>, String> {
    if entries.is_empty() {
        return Err("取り出す Unity パッケージを選んでください".into());
    }
    let file = File::open(zip_path).map_err(|_| "zip を開けません".to_string())?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|_| "zip として読めません。壊れているか、別の形式です".to_string())?;
    // 表示用の名前（Shift_JIS を考慮して読んだもの）から番号を引く。
    let mut index_of = std::collections::HashMap::new();
    for index in 0..archive.len().min(crate::file_browser::MAX_ZIP_ENTRIES) {
        let Ok(entry) = archive.by_index_raw(index) else { continue };
        let name = crate::file_browser::decode_zip_name(entry.name_raw(), entry.name());
        let path = crate::file_browser::clean_zip_path(&name);
        if entry.is_dir() || path.is_empty() {
            continue;
        }
        index_of.entry(path).or_insert(index);
    }
    std::fs::create_dir_all(dest).map_err(|_| "取り出し先のフォルダを作れません".to_string())?;
    let dest = std::fs::canonicalize(dest)
        .map(|path| crate::library_files::without_verbatim_prefix(&path))
        .map_err(|_| "取り出し先のフォルダを作れません".to_string())?;
    let mut taken = Vec::new();
    let mut prepared = Vec::new();
    for wanted in entries {
        let Some(&index) = index_of.get(wanted.as_str()) else {
            return Err("選んだ Unity パッケージが zip の中に見つかりません".into());
        };
        let name = safe_file_name(wanted)
            .ok_or_else(|| "Unity パッケージ（.unitypackage）だけを取り出せます".to_string())?;
        let name = unique_name(&mut taken, name);
        let target = dest.join(&name);
        // 名前は最後の部分だけなので外へは出ないが、念のため確かめる。
        if target.parent() != Some(dest.as_path()) {
            return Err("取り出し先がフォルダの外になるため、取り出しません".into());
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "この zip の圧縮方式には対応していません".to_string())?;
        let declared = entry.size();
        if declared > max_bytes {
            return Err("Unity パッケージが大きすぎるため、取り出しません".into());
        }
        let partial = dest.join(format!("{name}.part"));
        let written = (|| -> Result<u64, String> {
            let mut out = File::create(&partial).map_err(|_| "取り出し先に書き込めません".to_string())?;
            let mut limited = (&mut entry).take(declared.min(max_bytes) + 1);
            let copied = std::io::copy(&mut limited, &mut out)
                .map_err(|_| "zip から取り出せません。壊れているか、対応していない形式です".to_string())?;
            out.flush().map_err(|_| "取り出し先に書き込めません".to_string())?;
            Ok(copied)
        })();
        match written {
            Ok(copied) if copied == declared => {}
            Ok(_) => {
                let _ = std::fs::remove_file(&partial);
                return Err("zip の中の大きさが記録と合わないため、取り出しません".into());
            }
            Err(err) => {
                let _ = std::fs::remove_file(&partial);
                return Err(err);
            }
        }
        let _ = std::fs::remove_file(&target);
        std::fs::rename(&partial, &target).map_err(|_| {
            let _ = std::fs::remove_file(&partial);
            "取り出し先に書き込めません".to_string()
        })?;
        prepared.push(PreparedPackage {
            name: last_part(wanted).to_string(),
            path: target.display().to_string(),
            size: declared,
        });
    }
    Ok(prepared)
}

/// キャッシュの中の古いファイルを消し、空のフォルダも消す。リンクはたどらない。
pub(crate) fn clean_cache(root: &Path, max_age: Duration, now: SystemTime) -> usize {
    let mut removed = 0;
    let Ok(children) = std::fs::read_dir(root) else { return 0 };
    for child in children.flatten() {
        let path = child.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.is_dir() {
            removed += clean_cache(&path, max_age, now);
            let _ = std::fs::remove_dir(&path);
            continue;
        }
        let old = meta
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);
        if old && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// 渡すファイルは、キャッシュの取り出し先か、ライブラリ内の `.unitypackage` だけ。
pub(crate) fn allowed_launch_path(raw: &str, cache_root: &Path, library: &str, remote: &str) -> Result<PathBuf, String> {
    let requested = requested_path(raw, "")?;
    if !has_extension(&requested, "unitypackage") {
        return Err("Unity パッケージ（.unitypackage）だけを Unity に渡せます".into());
    }
    let resolved = resolve(&requested)?;
    if !resolved.is_file() {
        return Err("Unity パッケージが見つかりません".into());
    }
    let cache = std::fs::canonicalize(cache_root.join(CACHE_DIR))
        .map(|path| crate::library_files::without_verbatim_prefix(&path))
        .ok();
    let roots = [cache, optional_root(library), optional_root(remote)];
    if roots.iter().flatten().any(|root| resolved.starts_with(root)) {
        Ok(resolved)
    } else {
        Err(OUTSIDE.into())
    }
}

/// 設定の既定 Unity を確かめる。
pub(crate) fn editor_from_setting(selected: &str) -> Result<PathBuf, String> {
    let selected = selected.trim();
    if selected.is_empty() {
        return Err(NO_EDITOR.into());
    }
    let path = PathBuf::from(selected);
    if !path.is_file() {
        return Err(EDITOR_MISSING.into());
    }
    Ok(path)
}

/// `-openfile` は起動中のエディタにだけ届く。エディタが無いと Unity は Hub を開いて正常終了するので、
/// 終了コードでは渡せなかったことが分からない。渡す前に、設定した Unity が起動しているかを確かめる。
/// 起動中のプロセスを調べられなかったとき（`None`）は、確かめずに渡す。
pub(crate) fn require_running_editor(editor: &Path, running: Option<&[PathBuf]>) -> Result<(), String> {
    let Some(running) = running else {
        return Ok(());
    };
    if running.iter().any(|path| same_editor(editor, path)) {
        Ok(())
    } else if running.is_empty() {
        Err(EDITOR_NOT_RUNNING.into())
    } else {
        Err(OTHER_EDITOR_RUNNING.into())
    }
}

fn same_editor(a: &Path, b: &Path) -> bool {
    if same_file::is_same_file(a, b).unwrap_or(false) {
        return true;
    }
    if cfg!(windows) {
        let key = |path: &Path| path.to_string_lossy().replace('/', "\\").to_lowercase();
        key(a) == key(b)
    } else {
        a == b
    }
}

/// 起動中の Unity エディタ（Windows は Unity.exe、Mac と Linux は Unity）の実行ファイルのパス。
/// 一覧を作れないときは `None`。Unity Hub は名前が違うので入らない。
pub(crate) fn running_editor_paths() -> Option<Vec<PathBuf>> {
    running_editors::list()
}

#[cfg(windows)]
mod running_editors {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const MAX_PATH: usize = 260;

    #[repr(C)]
    struct ProcessEntry32W {
        size: u32,
        usage: u32,
        process_id: u32,
        default_heap_id: usize,
        module_id: u32,
        threads: u32,
        parent_process_id: u32,
        priority_class_base: i32,
        flags: u32,
        exe_file: [u16; MAX_PATH],
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(process: *mut c_void, flags: u32, name: *mut u16, size: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    fn exe_name(entry: &ProcessEntry32W) -> String {
        let end = entry.exe_file.iter().position(|unit| *unit == 0).unwrap_or(MAX_PATH);
        String::from_utf16_lossy(&entry.exe_file[..end])
    }

    fn image_path(process_id: u32) -> Option<PathBuf> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 32_768];
        let mut size = buffer.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(process) };
        (ok != 0).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer[..size as usize])))
    }

    pub(super) fn list() -> Option<Vec<PathBuf>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot.is_null() || snapshot as isize == -1 {
            return None;
        }
        let mut entry = ProcessEntry32W {
            size: std::mem::size_of::<ProcessEntry32W>() as u32,
            usage: 0,
            process_id: 0,
            default_heap_id: 0,
            module_id: 0,
            threads: 0,
            parent_process_id: 0,
            priority_class_base: 0,
            flags: 0,
            exe_file: [0; MAX_PATH],
        };
        let mut found = Vec::new();
        let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        let listed = more;
        while more {
            if exe_name(&entry).eq_ignore_ascii_case("unity.exe") {
                // 権限の違いでパスを読めない Unity も、起動していることは数える。
                found.push(image_path(entry.process_id).unwrap_or_else(|| PathBuf::from("Unity.exe")));
            }
            more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        listed.then_some(found)
    }
}

#[cfg(target_os = "linux")]
mod running_editors {
    use std::path::PathBuf;

    pub(super) fn list() -> Option<Vec<PathBuf>> {
        let entries = std::fs::read_dir("/proc").ok()?;
        let found = entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()))
            .filter_map(|entry| std::fs::read_link(entry.path().join("exe")).ok())
            .filter(|exe| exe.file_name().is_some_and(|name| name == "Unity"))
            .collect();
        Some(found)
    }
}

#[cfg(target_os = "macos")]
mod running_editors {
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    pub(super) fn list() -> Option<Vec<PathBuf>> {
        let output = Command::new("/bin/ps")
            .args(["-axww", "-o", "comm="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        Some(
            text.lines()
                .map(str::trim)
                .filter(|line| line.ends_with("/Contents/MacOS/Unity"))
                .map(PathBuf::from)
                .collect(),
        )
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod running_editors {
    pub(super) fn list() -> Option<Vec<std::path::PathBuf>> {
        None
    }
}

/// `<Unity> -openfile <pkg>` の引数。シェルを通さないので、空白や記号を含むパスもそのまま渡る。
pub(crate) fn openfile_command(editor: &Path, package: &Path) -> Command {
    let mut command = Command::new(editor);
    command
        .arg("-openfile")
        .arg(package)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = editor.parent() {
        command.current_dir(dir);
    }
    command
}

/// 先頭から順に Unity へ渡す。起動中のエディタに渡すプロセスはすぐ終わるので、終わったら次へ進む。
/// 終わらないときは、エディタ本体が起動したとみなしてここで止める。
pub(crate) fn launch_with_editor(
    editor: &Path,
    packages: &[PathBuf],
    handoff_timeout: Duration,
    gap: Duration,
) -> Result<LaunchResult, String> {
    let mut launched = 0;
    for (index, package) in packages.iter().enumerate() {
        let mut child = openfile_command(editor, package)
            .spawn()
            .map_err(|_| "Unity を起動できませんでした。設定の「Unity」を確かめてください".to_string())?;
        let last = index + 1 == packages.len();
        let started = std::time::Instant::now();
        let finished = loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => break true,
                Ok(Some(_)) => {
                    return Err("Unity が異常終了しました。Unity の起動状態と設定を確かめてください".into());
                }
                Ok(None) if started.elapsed() >= handoff_timeout => break false,
                Ok(None) => std::thread::sleep(Duration::from_millis(200)),
                Err(_) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Err("Unity の起動状態を確認できませんでした。Unity の画面を確かめてください".into());
                }
            }
        };
        launched += 1;
        if !finished {
            // エディタ本体として動き続けるプロセス。終わるまで別スレッドで待つ。
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            if !last {
                return Ok(LaunchResult { launched, waiting: true });
            }
        } else if !last {
            std::thread::sleep(gap);
        }
    }
    Ok(LaunchResult { launched, waiting: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library_files::resolved_temp_dir;
    use zip::write::SimpleFileOptions;

    fn temp_root(name: &str) -> PathBuf {
        let root = resolved_temp_dir().join(format!(
            "vrclibman-unity-import-{name}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8], bool)]) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for (name, data, deflate) in files {
            let method = if *deflate {
                zip::CompressionMethod::Deflated
            } else {
                zip::CompressionMethod::Stored
            };
            writer
                .start_file(*name, SimpleFileOptions::default().compression_method(method))
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn lists_only_unitypackages_inside_a_zip() {
        let root = temp_root("list");
        let zip_path = root.join("Sample_Outfit.zip");
        write_zip(
            &zip_path,
            &[
                ("Sample/Sample_Outfit.unitypackage", b"aaaa", false),
                ("Sample/Readme.txt", b"bb", false),
                ("Sample_PhysBone.UNITYPACKAGE", b"ccc", true),
            ],
        );
        let list = list_packages(&zip_path).unwrap();
        assert_eq!(list.kind, "zip");
        assert_eq!(
            list.packages,
            vec![
                UnityPackageEntry { entry: "Sample/Sample_Outfit.unitypackage".into(), name: "Sample_Outfit.unitypackage".into(), size: 4 },
                UnityPackageEntry { entry: "Sample_PhysBone.UNITYPACKAGE".into(), name: "Sample_PhysBone.UNITYPACKAGE".into(), size: 3 },
            ]
        );
        let single = root.join("Sample_Hair.unitypackage");
        std::fs::write(&single, b"12345").unwrap();
        let list = list_packages(&single).unwrap();
        assert_eq!(list.kind, "package");
        assert_eq!(list.packages, vec![UnityPackageEntry { entry: String::new(), name: "Sample_Hair.unitypackage".into(), size: 5 }]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn extracts_only_the_chosen_entries_and_never_outside_the_folder() {
        let root = temp_root("extract");
        let zip_path = root.join("a.zip");
        let big = vec![7u8; 64 * 1024];
        write_zip(
            &zip_path,
            &[
                ("x/One.unitypackage", b"one", false),
                ("../../evil.unitypackage", b"evil", false),
                ("y/One.unitypackage", &big, true),
                ("z/notes.txt", b"t", false),
            ],
        );
        let dest = root.join("cache").join("unity-import").join("1000001");
        let entries: Vec<String> = list_packages(&zip_path).unwrap().packages.into_iter().map(|p| p.entry).collect();
        assert!(entries.iter().any(|entry| entry == "../../evil.unitypackage"));
        let prepared = extract_packages(&zip_path, &entries, &dest, MAX_PACKAGE_BYTES).unwrap();
        let names: Vec<String> = prepared
            .iter()
            .map(|p| Path::new(&p.path).file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["One.unitypackage", "evil.unitypackage", "One (2).unitypackage"]);
        for package in &prepared {
            // 返すパスは Windows の `\\?\` を外した表示用なので、両方を canonicalize して比べる。
            let parent = std::fs::canonicalize(Path::new(&package.path).parent().unwrap()).unwrap();
            assert_eq!(parent, std::fs::canonicalize(&dest).unwrap());
        }
        assert_eq!(std::fs::read(&prepared[2].path).unwrap(), big);
        assert!(!root.join("evil.unitypackage").exists());
        assert!(!root.parent().unwrap().join("evil.unitypackage").exists());
        // 選んだものだけ。txt は取り出せない。zip に無い名前も断る。
        assert!(extract_packages(&zip_path, &["z/notes.txt".into()], &dest, MAX_PACKAGE_BYTES).is_err());
        assert!(extract_packages(&zip_path, &["missing.unitypackage".into()], &dest, MAX_PACKAGE_BYTES).is_err());
        assert!(extract_packages(&zip_path, &[], &dest, MAX_PACKAGE_BYTES).is_err());
        // 上限を超えるものは取り出さず、途中のファイルも残さない。
        let err = extract_packages(&zip_path, &["y/One.unitypackage".into()], &root.join("small"), 1024).unwrap_err();
        assert!(err.contains("大きすぎる"));
        let leftovers: Vec<_> = std::fs::read_dir(root.join("small")).unwrap().collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn safe_names_drop_folders_and_bad_characters() {
        assert_eq!(safe_file_name("a/b/c.unitypackage").as_deref(), Some("c.unitypackage"));
        assert_eq!(safe_file_name("..\\..\\c.unitypackage").as_deref(), Some("c.unitypackage"));
        assert_eq!(safe_file_name("a:b?.unitypackage").as_deref(), Some("a_b_.unitypackage"));
        assert_eq!(safe_file_name("見本 衣装.unitypackage").as_deref(), Some("見本 衣装.unitypackage"));
        assert_eq!(safe_file_name(".unitypackage"), None);
        assert_eq!(safe_file_name("x.txt"), None);
        assert_eq!(safe_file_name(".."), None);
        let long = format!("{}.unitypackage", "あ".repeat(300));
        let cut = safe_file_name(&long).unwrap();
        assert!(cut.ends_with(".unitypackage"));
        assert_eq!(cut.chars().count(), 120);
    }

    #[test]
    fn cache_folder_uses_digits_only() {
        let root = Path::new("/cache");
        assert_eq!(cache_folder(root, Some("1000001")), root.join("unity-import").join("1000001"));
        assert_eq!(cache_folder(root, Some("../x")), root.join("unity-import").join("other"));
        assert_eq!(cache_folder(root, None), root.join("unity-import").join("other"));
        assert_eq!(cache_folder(root, Some("1234567890123")), root.join("unity-import").join("other"));
    }

    #[test]
    fn only_library_files_are_accepted() {
        let root = temp_root("allowed");
        let library = root.join("library");
        let outside = root.join("outside");
        std::fs::create_dir_all(library.join("1000001")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let inside_zip = library.join("1000001").join("a.zip");
        std::fs::write(&inside_zip, b"").unwrap();
        let outside_pkg = outside.join("b.unitypackage");
        std::fs::write(&outside_pkg, b"").unwrap();
        let text = library.join("1000001").join("c.txt");
        std::fs::write(&text, b"").unwrap();
        let lib = library.display().to_string();
        assert!(allowed_source(&inside_zip.display().to_string(), &lib, "").is_ok());
        assert_eq!(allowed_source(&outside_pkg.display().to_string(), &lib, "").unwrap_err(), OUTSIDE);
        assert!(allowed_source(&outside_pkg.display().to_string(), &lib, &outside.display().to_string()).is_ok());
        assert_eq!(allowed_source(&text.display().to_string(), &lib, "").unwrap_err(), NOT_PACKAGE);
        assert!(allowed_source("relative/a.zip", &lib, "").is_err());
        let sneaky = format!("{}/1000001/../../outside/b.unitypackage", lib);
        assert!(allowed_source(&sneaky, &lib, "").is_err());
        assert_eq!(allowed_source(&inside_zip.display().to_string(), "", "").unwrap_err(), OUTSIDE);

        let cache = root.join("cache");
        std::fs::create_dir_all(cache.join(CACHE_DIR).join("other")).unwrap();
        let cached = cache.join(CACHE_DIR).join("other").join("d.unitypackage");
        std::fs::write(&cached, b"").unwrap();
        assert!(allowed_launch_path(&cached.display().to_string(), &cache, &lib, "").is_ok());
        assert_eq!(allowed_launch_path(&outside_pkg.display().to_string(), &cache, &lib, "").unwrap_err(), OUTSIDE);
        assert!(allowed_launch_path(&inside_zip.display().to_string(), &cache, &lib, "").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn clean_cache_removes_only_old_files() {
        let root = temp_root("clean");
        let cache = root.join(CACHE_DIR);
        std::fs::create_dir_all(cache.join("1000001")).unwrap();
        std::fs::create_dir_all(cache.join("1000002")).unwrap();
        std::fs::write(cache.join("1000001").join("old.unitypackage"), b"x").unwrap();
        std::fs::write(cache.join("1000002").join("new.unitypackage"), b"x").unwrap();
        let later = SystemTime::now() + Duration::from_secs(2 * 24 * 60 * 60);
        // 「今」を 2 日後にすると両方古い。1 時間後なら両方残る。
        assert_eq!(clean_cache(&cache, CACHE_MAX_AGE, SystemTime::now() + Duration::from_secs(3600)), 0);
        assert!(cache.join("1000002").join("new.unitypackage").exists());
        assert_eq!(clean_cache(&cache, CACHE_MAX_AGE, later), 2);
        assert!(!cache.join("1000001").exists());
        assert!(cache.exists());
        assert_eq!(clean_cache(&root.join("missing"), CACHE_MAX_AGE, later), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn editor_setting_must_point_to_a_file() {
        assert_eq!(editor_from_setting("  ").unwrap_err(), NO_EDITOR);
        assert_eq!(editor_from_setting("/no/such/Unity").unwrap_err(), EDITOR_MISSING);
        assert_eq!(LaunchMode::parse("unity"), Ok(LaunchMode::Unity));
        assert_eq!(LaunchMode::parse("system"), Ok(LaunchMode::System));
        assert!(LaunchMode::parse("sh -c").is_err());
    }

    #[test]
    fn hands_off_only_when_the_chosen_editor_is_running() {
        let root = temp_root("running");
        let chosen = root.join("2022.3.22f1").join("Unity.exe");
        let other = root.join("6000.0.33f1").join("Unity.exe");
        for path in [&chosen, &other] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        // エディタが無いと Unity は Hub を開いて正常終了するので、渡す前に止める。
        assert_eq!(require_running_editor(&chosen, Some(&[])).unwrap_err(), EDITOR_NOT_RUNNING);
        assert_eq!(
            require_running_editor(&chosen, Some(&[other.clone()])).unwrap_err(),
            OTHER_EDITOR_RUNNING
        );
        assert!(require_running_editor(&chosen, Some(&[other.clone(), chosen.clone()])).is_ok());
        // 起動中の一覧を作れないときは、これまでどおり渡す。
        assert!(require_running_editor(&chosen, None).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_match_without_regard_to_case_or_slashes() {
        assert!(same_editor(
            Path::new(r"C:\Program Files\Unity\Hub\Editor\2022.3.22f1\Editor\Unity.exe"),
            Path::new("c:/program files/unity/hub/editor/2022.3.22f1/editor/UNITY.EXE"),
        ));
    }

    #[cfg(windows)]
    #[test]
    fn a_running_unity_exe_is_found_with_its_full_path() {
        let root = temp_root("process");
        let fake = root.join("Unity.exe");
        let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        std::fs::copy(system.join("System32").join("ping.exe"), &fake).unwrap();
        let mut child = Command::new(&fake)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let found = running_editor_paths().expect("process list");
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.iter().any(|path| same_editor(path, &fake)), "{found:?}");
        assert!(require_running_editor(&fake, Some(&found)).is_ok());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_running_process_list_can_be_read() {
        // CI にも開発機にも Unity が起動しているとは限らないので、一覧を作れることだけを見る。
        let listed = running_editor_paths().expect("process list");
        assert!(listed.iter().all(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            name == "unity.exe" || name == "unity"
        }));
    }

    #[test]
    fn openfile_passes_arguments_without_a_shell() {
        let command = openfile_command(Path::new("/opt/Unity/Editor/Unity"), Path::new("/tmp/a b;$(x).unitypackage"));
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["-openfile", "/tmp/a b;$(x).unitypackage"]);
        assert_eq!(command.get_program(), "/opt/Unity/Editor/Unity");
    }

    #[cfg(unix)]
    #[test]
    fn failed_handoff_stops_before_the_next_package_and_checks_the_last_one() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("launch-failure");
        let log = root.join("log.txt");
        let editor = root.join("Unity");
        std::fs::write(
            &editor,
            format!("#!/bin/sh\nprintf '%s\\n' \"$2\" >> '{}'\nexit 23\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
        let packages = vec![root.join("a.unitypackage"), root.join("b.unitypackage")];

        for count in [2, 1] {
            let _ = std::fs::remove_file(&log);
            let result = launch_with_editor(
                &editor,
                &packages[..count],
                Duration::from_secs(5),
                Duration::ZERO,
            );
            assert!(result.unwrap_err().contains("異常終了"));
            let passed = std::fs::read_to_string(&log).unwrap();
            assert_eq!(passed.lines().collect::<Vec<_>>(), vec![packages[0].to_str().unwrap()]);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn launches_one_after_another_and_stops_when_the_editor_keeps_running() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("launch");
        let log = root.join("log.txt");
        let quick = root.join("quick-editor");
        std::fs::write(&quick, format!("#!/bin/sh\nprintf '%s|%s\\n' \"$1\" \"$2\" >> '{}'\n", log.display())).unwrap();
        std::fs::set_permissions(&quick, std::fs::Permissions::from_mode(0o755)).unwrap();
        let packages = vec![root.join("a b.unitypackage"), root.join("c.unitypackage")];
        let result = launch_with_editor(&quick, &packages, Duration::from_secs(5), Duration::from_millis(10)).unwrap();
        assert_eq!(result, LaunchResult { launched: 2, waiting: false });
        let text = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            vec![
                format!("-openfile|{}", packages[0].display()),
                format!("-openfile|{}", packages[1].display()),
            ]
        );
        let slow = root.join("slow-editor");
        std::fs::write(&slow, "#!/bin/sh\nsleep 2\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let result = launch_with_editor(&slow, &packages, Duration::from_millis(300), Duration::from_millis(10)).unwrap();
        assert_eq!(result, LaunchResult { launched: 1, waiting: true });
        let _ = std::fs::remove_dir_all(root);
    }
}
