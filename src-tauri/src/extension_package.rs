//! アプリに同梱した拡張機能を、アプリが決めたフォルダ（アプリのローカルデータの `browser-extension`）へ展開する。
//! 利用者はこのフォルダを Chrome / Edge に一度「パッケージ化されていない拡張機能」として読み込ませる。
//! アプリを更新すると、起動時にこのフォルダも同梱の版へ置き換わり、拡張は `/health` で新しい版を知って自分を読み込み直す。
//! 拡張の版はアプリの版と同じなので、ネットワークから別に取得しない（アプリ本体の更新は署名を確かめて GitHub から取る）。
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Manager};

pub const FOLDER: &str = "browser-extension";
const FILE_LIST: &str = ".vrclibman-files";
/// 開発用のファイル。Chrome には要らない。
const SKIP: &[&str] = &["package.json", "package-lock.json"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackageStatus {
    pub path: String,
    pub bundled_version: Option<String>,
    pub installed_version: Option<String>,
    pub up_to_date: bool,
}

fn bundled_dir(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app.path().resource_dir().ok().map(|dir| dir.join("extension"));
    if let Some(dir) = bundled.filter(|dir| dir.join("manifest.json").is_file()) {
        return Some(dir);
    }
    // tauri dev ではリソースを写さないことがあるので、ソースのフォルダを使う。
    if cfg!(debug_assertions) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../extension");
        if source.join("manifest.json").is_file() {
            return Some(source);
        }
    }
    None
}

fn license_file(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app.path().resource_dir().ok().map(|dir| dir.join("LICENSE"));
    if let Some(file) = bundled.filter(|file| file.is_file()) {
        return Some(file);
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../LICENSE");
    (cfg!(debug_assertions) && source.is_file()).then_some(source)
}

pub fn managed_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|dir| dir.join(FOLDER))
        .map_err(|_| "拡張機能のフォルダの場所を決められません".to_string())
}

pub(crate) fn manifest_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let version = value.get("version")?.as_str()?;
    valid_version(version).then(|| version.to_string())
}

fn valid_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    (1..=4).contains(&parts.len()) && parts.iter().all(|part| !part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit()))
}

/// 拡張が自分を読み込み直すかを決めるため、`/health` で知らせる版。展開していなければ無し。
pub fn installed_version(app: &AppHandle) -> Option<String> {
    manifest_version(&managed_dir(app).ok()?)
}

pub fn status(app: &AppHandle) -> Result<PackageStatus, String> {
    let dir = managed_dir(app)?;
    let bundled_version = bundled_dir(app).and_then(|dir| manifest_version(&dir));
    let installed_version = manifest_version(&dir);
    Ok(PackageStatus {
        path: dir.display().to_string(),
        up_to_date: installed_version.is_some() && installed_version == bundled_version,
        bundled_version,
        installed_version,
    })
}

pub fn install(app: &AppHandle) -> Result<PackageStatus, String> {
    let source = bundled_dir(app).ok_or("アプリに同梱した拡張機能が見つかりません。アプリを入れ直してください")?;
    let target = managed_dir(app)?;
    let license = license_file(app);
    install_from(&source, &target, license.as_deref())?;
    status(app)
}

/// 起動時と読み込み方の変更時。拡張を使う設定なら用意し、展開済みなら同梱の版にそろえる。
pub fn refresh(app: &AppHandle, uses_extension: bool) -> Result<bool, String> {
    let current = status(app)?;
    if current.bundled_version.is_none() || current.up_to_date {
        return Ok(false);
    }
    if current.installed_version.is_none() && !uses_extension {
        return Ok(false);
    }
    install(app)?;
    Ok(true)
}

pub fn refresh_from_settings(app: &AppHandle) -> Result<bool, String> {
    let uses_extension = {
        let state = app.state::<crate::AppState>();
        let connection = state.db.lock().map_err(|_| "データベースをロックできません".to_string())?;
        crate::integration::status(&connection)?.mode != "browser"
    };
    refresh(app, uses_extension)
}

fn relative_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1) {
        let entry = entry.map_err(|_| "同梱した拡張機能を読めません")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|_| "同梱した拡張機能を読めません")?
            .to_path_buf();
        let name = relative.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if name.starts_with('.') || (relative.components().count() == 1 && SKIP.contains(&name)) {
            continue;
        }
        files.push(relative);
    }
    files.sort();
    Ok(files)
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|component| matches!(component, Component::Normal(_)))
}

fn write_replacing(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = target.parent().ok_or("拡張機能のフォルダに書けません")?;
    std::fs::create_dir_all(parent).map_err(|_| "拡張機能のフォルダを作れません")?;
    let mut temp = tempfile::Builder::new()
        .prefix(".vrclibman-")
        .tempfile_in(parent)
        .map_err(|_| "拡張機能のフォルダに書けません")?;
    temp.write_all(bytes).map_err(|_| "拡張機能のファイルを書けません")?;
    temp.as_file().sync_all().map_err(|_| "拡張機能のファイルを書けません")?;
    // 同じフォルダ内の一時ファイルから置き換える。読み込み中の Chrome が半端なファイルを読まないため。
    temp.persist(target).map_err(|_| "拡張機能のファイルを置き換えられません。ブラウザを閉じてからもう一度お試しください")?;
    Ok(())
}

/// `source` の拡張を `target` へ写す。`manifest.json` を最後に置き換えるので、途中で失敗しても新しい版を名乗らない。
/// 前回写したファイルのうち、新しい版に無いものだけを消す。利用者が置いたほかのファイルには触れない。
pub(crate) fn install_from(source: &Path, target: &Path, license: Option<&Path>) -> Result<(), String> {
    if manifest_version(source).is_none() {
        return Err("同梱した拡張機能の版を読めません".into());
    }
    std::fs::create_dir_all(target).map_err(|_| "拡張機能のフォルダを作れません")?;
    let target = target.canonicalize().map_err(|_| "拡張機能のフォルダを作れません")?;
    let mut files = relative_files(source)?;
    let manifest = PathBuf::from("manifest.json");
    files.retain(|file| file != &manifest);
    let mut written = BTreeSet::new();
    for file in &files {
        let bytes = std::fs::read(source.join(file)).map_err(|_| "同梱した拡張機能を読めません")?;
        write_replacing(&target.join(file), &bytes)?;
        written.insert(file.clone());
    }
    if let Some(license) = license {
        if let Ok(bytes) = std::fs::read(license) {
            write_replacing(&target.join("LICENSE"), &bytes)?;
            written.insert(PathBuf::from("LICENSE"));
        }
    }
    let manifest_bytes = std::fs::read(source.join(&manifest)).map_err(|_| "同梱した拡張機能を読めません")?;
    write_replacing(&target.join(&manifest), &manifest_bytes)?;
    written.insert(manifest);
    let list_path = target.join(FILE_LIST);
    let previous = std::fs::read_to_string(&list_path).unwrap_or_default();
    for line in previous.lines() {
        let old = PathBuf::from(line.replace('\\', "/"));
        if !safe_relative(&old) || written.contains(&old) {
            continue;
        }
        let path = target.join(&old);
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_file()) {
            let _ = std::fs::remove_file(&path);
        }
    }
    let list: Vec<String> = written
        .iter()
        .map(|file| file.to_string_lossy().replace('\\', "/"))
        .collect();
    write_replacing(&list_path, format!("{}\n", list.join("\n")).as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn bundle(dir: &Path, version: &str, files: &[(&str, &str)]) {
        write(&dir.join("manifest.json"), &format!(r#"{{"manifest_version":3,"version":"{version}"}}"#));
        write(&dir.join("package.json"), "{}");
        for (name, text) in files {
            write(&dir.join(name), text);
        }
    }

    #[test]
    fn the_bundled_extension_is_copied_without_development_files_and_updated_in_place() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("bundle");
        let target = root.path().join("data").join(FOLDER);
        let license = root.path().join("LICENSE");
        write(&license, "MPL");
        bundle(&source, "0.8.3", &[("popup.js", "old"), ("icons/icon-16.png", "png"), ("gone.js", "x")]);
        install_from(&source, &target, Some(&license)).unwrap();
        assert_eq!(manifest_version(&target).as_deref(), Some("0.8.3"));
        assert_eq!(std::fs::read_to_string(target.join("popup.js")).unwrap(), "old");
        assert!(target.join("icons/icon-16.png").is_file());
        assert!(target.join("LICENSE").is_file());
        assert!(!target.join("package.json").exists());

        // 利用者が置いたファイルは消さず、前回写して新しい版に無いファイルだけを消す。
        write(&target.join("notes.txt"), "mine");
        std::fs::remove_file(source.join("gone.js")).unwrap();
        bundle(&source, "0.9.0", &[("popup.js", "new")]);
        install_from(&source, &target, Some(&license)).unwrap();
        assert_eq!(manifest_version(&target).as_deref(), Some("0.9.0"));
        assert_eq!(std::fs::read_to_string(target.join("popup.js")).unwrap(), "new");
        assert!(!target.join("gone.js").exists());
        assert!(target.join("notes.txt").is_file());
        let leftovers: Vec<_> = std::fs::read_dir(&target)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with(".vrclibman-") && name != FILE_LIST
            })
            .collect();
        assert!(leftovers.is_empty(), "no temporary files remain");
    }

    #[test]
    fn a_broken_bundle_or_a_tampered_file_list_cannot_remove_files_outside_the_folder() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("bundle");
        let target = root.path().join(FOLDER);
        write(&source.join("popup.js"), "x");
        assert!(install_from(&source, &target, None).is_err(), "a bundle without a valid version is refused");
        assert!(manifest_version(&target).is_none());

        bundle(&source, "1.0.0", &[]);
        let outside = root.path().join("keep.txt");
        write(&outside, "keep");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join(FILE_LIST), "../keep.txt\n/abs.txt\npopup.js\n").unwrap();
        install_from(&source, &target, None).unwrap();
        assert!(outside.is_file());
        assert!(target.join("popup.js").is_file());
    }

    #[test]
    fn only_numeric_chrome_versions_are_accepted() {
        assert!(valid_version("0.8.3"));
        assert!(valid_version("1.2.3.4"));
        for bad in ["", "1.0.0-beta", "1..2", "1.2.3.4.5", "v1"] {
            assert!(!valid_version(bad), "{bad}");
        }
    }
}
