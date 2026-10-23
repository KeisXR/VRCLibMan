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
    if name.chars().any(|ch| matches!(ch, '/' | '\\' | '\0') || ch.is_control()) {
        return None;
    }
    Some(name)
}

pub(crate) fn path_has_vrclibman(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(part) => part.eq_ignore_ascii_case("VRCLibMan"),
        _ => false,
    })
}

/// Lexical checks only. Existence is probed later so a bad path does no I/O.
pub(crate) fn intake_path_ok(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("ダウンロードのパスが絶対パスではありません".into());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("ダウンロードのパスに親ディレクトリは使えません".into());
    }
    if !path_has_vrclibman(path) {
        return Err("VRCLibMan のフォルダ以外のファイルは移動しません".into());
    }
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    if safe_filename(name).is_none() {
        return Err("ファイル名を読めません".into());
    }
    Ok(())
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

pub(crate) fn place_into_library(
    source: &Path,
    library: Option<&Path>,
    booth_id: &str,
) -> Result<PathBuf, String> {
    intake_path_ok(source)?;
    let filename = safe_filename(source.file_name().and_then(|name| name.to_str()).unwrap_or(""))
        .ok_or_else(|| "ファイル名を読めません".to_string())?;
    if probe_file(source, PROBE_TIMEOUT) != Probe::Present {
        return match probe_path(source, PROBE_TIMEOUT) {
            Probe::Unreachable => Err("ダウンロードしたファイルを確認できません".into()),
            _ => Err("ダウンロードしたファイルが見つかりません".into()),
        };
    }
    let Some(library) = library.filter(|path| !path.as_os_str().is_empty()) else {
        return Ok(source.to_path_buf());
    };
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

pub(crate) fn fill_places(snapshot: &mut Snapshot, remote_folder: &str) {
    let remote_folder = remote_folder.trim();
    snapshot.remote_configured = !remote_folder.is_empty();
    let remote_root = Path::new(remote_folder);
    let mut reachable = snapshot.remote_configured
        && probe_path(remote_root, PROBE_TIMEOUT) == Probe::Present;
    let mut placed = Vec::new();
    for row in &snapshot.download_rows {
        let local = find_local(row, &snapshot.files);
        let mut remote_path = None;
        if reachable && local.is_none() {
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

    #[test]
    fn intake_rejects_paths_outside_the_staging_folder() {
        let staged = Path::new("/tmp/Downloads/VRCLibMan/424242/body.unitypackage");
        assert!(intake_path_ok(staged).is_ok());
        assert!(intake_path_ok(Path::new("Downloads/VRCLibMan/424242/body.unitypackage")).is_err());
        assert!(intake_path_ok(Path::new("/tmp/Downloads/other/body.unitypackage")).is_err());
        assert!(
            intake_path_ok(Path::new("/tmp/Downloads/VRCLibMan/../secret.unitypackage")).is_err()
        );
        assert!(safe_filename("../secret.unitypackage").is_none());
        assert!(safe_filename("body.unitypackage").is_some());
        assert!(clean_variation("通常").is_some());
        assert!(clean_variation("../通常").is_none());
    }

    #[test]
    fn move_stays_inside_the_library_and_registers_in_place_without_one() {
        let root = std::env::temp_dir().join(format!(
            "vrclibman-files-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let staging = root.join("Downloads").join("VRCLibMan").join("424242");
        let library = root.join("library");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&library).unwrap();
        let source = staging.join("body.unitypackage");
        std::fs::write(&source, b"pkg").unwrap();
        let placed = place_into_library(&source, Some(&library), "424242").unwrap();
        assert!(!source.exists());
        assert_eq!(placed, library.join("424242").join("body.unitypackage"));
        assert_eq!(std::fs::read(&placed).unwrap(), b"pkg");
        assert!(placed.starts_with(&library));

        let kept = staging.join("note.vrm");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(&kept, b"vrm").unwrap();
        let stayed = place_into_library(&kept, None, "424242").unwrap();
        assert_eq!(stayed, kept);
        assert!(kept.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_outside_the_library_is_refused() {
        let root = std::env::temp_dir().join(format!(
            "vrclibman-escape-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let library = root.join("library");
        let outside = root.join("outside");
        let staging = root.join("Downloads").join("VRCLibMan").join("424242");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::os::unix::fs::symlink(&outside, library.join("424242")).unwrap();
        let source = staging.join("body.unitypackage");
        std::fs::write(&source, b"pkg").unwrap();
        let err = place_into_library(&source, Some(&library), "424242").unwrap_err();
        assert!(err.contains("外"));
        assert!(source.exists());
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remote_lookup_finds_a_booth_folder_and_reports_a_missing_root() {
        let root = std::env::temp_dir().join(format!(
            "vrclibman-remote-{}",
            std::process::id()
        ));
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
}
