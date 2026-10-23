use std::ffi::OsStr;
use std::path::{Component, Path};

const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 20_000;
const FILE_EXTENSIONS: &[&str] = &[
    "zip", "rar", "7z", "unitypackage", "vrm", "fbx", "pmx", "pmd", "blend", "psd",
];
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "target",
    "__macosx",
    ".vs",
    "library",
    "temp",
    "obj",
    "logs",
    "packagecache",
];
const GENERIC_DIRS: &[&str] = &[
    "textures",
    "texture",
    "materials",
    "material",
    "plugins",
    "plugin",
    "prefabs",
    "prefab",
    "animations",
    "animation",
    "meshes",
    "mesh",
    "src",
    "bin",
    "assets",
    "meta",
    "tex",
    "downloads",
    "download",
    "desktop",
    "documents",
    "テクスチャ",
    "マテリアル",
    "プレハブ",
    "ダウンロード",
    "新しいフォルダー",
    "new folder",
];

#[derive(Debug, Clone)]
pub struct ScanEntry {
    pub path: String,
    pub filename: String,
    pub is_dir: bool,
    pub size: u64,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScanOutcome {
    pub entries: Vec<ScanEntry>,
    pub truncated: bool,
}

pub fn validate_scan_root(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err("フォルダが見つかりません".into());
    }
    let canon = path
        .canonicalize()
        .map_err(|err| format!("フォルダを確認できません: {err}"))?;
    scan_block_reason(&canon)
}

fn normal_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect()
}

fn scan_block_reason(canon: &Path) -> Result<(), String> {
    let normals = normal_components(canon);
    if normals.is_empty() {
        return Err(
            "ドライブ直下はスキャンできません。アセットの入ったフォルダを選んでください".into(),
        );
    }
    if let Some(first) = normals.first() {
        if first == "windows" || first.starts_with("program files") {
            return Err("システムフォルダはスキャンできません".into());
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(message) = macos_scan_block(
        canon,
        home_dir().as_deref(),
        std::env::temp_dir().canonicalize().ok().as_deref(),
    ) {
        return Err(message.into());
    }
    Ok(())
}

/// macOS roots that must not be scanned. The temporary directory is allowed so tests can use it.
pub(crate) fn macos_scan_block(
    canon: &Path,
    home: Option<&Path>,
    temp: Option<&Path>,
) -> Option<&'static str> {
    if let Some(temp) = temp.filter(|temp| !temp.as_os_str().is_empty()) {
        if canon.starts_with(temp) && canon != temp {
            return None;
        }
    }
    let normals = normal_components(canon);
    let Some(first) = normals.first() else {
        return Some("ドライブ直下はスキャンできません。アセットの入ったフォルダを選んでください");
    };
    if matches!(first.as_str(), "system" | "library" | "applications") || first == "private" {
        return Some("システムフォルダはスキャンできません");
    }
    if normals.len() == 1 && first == "volumes" {
        return Some("ボリュームの直下はスキャンできません。中のフォルダを選んでください");
    }
    if home.is_some_and(|home| canon == home) {
        return Some("ホームフォルダ直下はスキャンできません。中のフォルダを選んでください");
    }
    None
}

#[cfg(target_os = "macos")]
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

pub fn scan_folder(root: &Path) -> Result<ScanOutcome, String> {
    validate_scan_root(root)?;
    let mut entries = Vec::new();
    let mut truncated = false;
    let walker = walkdir::WalkDir::new(root)
        .max_depth(MAX_DEPTH)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            if entry.file_type().is_dir() {
                return !skip_dir(entry.file_name());
            }
            true
        });

    for item in walker {
        let entry = match item {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        if entry.depth() == 0 {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        let is_dir = entry.file_type().is_dir();
        if is_dir {
            if entry.depth() > 4 || generic_dir(entry.file_name()) {
                continue;
            }
        } else if !allowed_file(entry.path()) {
            continue;
        }
        let Some(filename) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if filename.starts_with('.') {
            continue;
        }
        let size = if is_dir {
            0
        } else {
            entry.metadata().map(|meta| meta.len()).unwrap_or(0)
        };
        let path = entry.path().display().to_string();
        let labels = labels_for(entry.path(), is_dir);
        entries.push(ScanEntry {
            path,
            filename,
            is_dir,
            size,
            labels,
        });
    }

    Ok(ScanOutcome { entries, truncated })
}

fn labels_for(path: &Path, is_dir: bool) -> Vec<String> {
    let mut labels = Vec::new();
    if is_dir {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            labels.push(name.to_string());
        }
    } else if let Some(stem) = path.file_stem().and_then(|name| name.to_str()) {
        labels.push(stem.to_string());
        if let Some(parent) = path.parent().and_then(|parent| parent.file_name()) {
            if let Some(parent) = parent.to_str() {
                if !generic_dir(OsStr::new(parent)) && !skip_dir(OsStr::new(parent)) {
                    labels.push(parent.to_string());
                }
            }
        }
    }
    labels
}

fn allowed_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    FILE_EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
}

fn skip_dir(name: &OsStr) -> bool {
    let lowered = name.to_string_lossy().to_lowercase();
    SKIP_DIRS.iter().any(|skip| *skip == lowered)
}

fn generic_dir(name: &OsStr) -> bool {
    let lowered = name.to_string_lossy().to_lowercase();
    GENERIC_DIRS.iter().any(|generic| *generic == lowered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("vrclibman-scan-{}-{seq}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn indexes_packages_and_skips_dependency_dirs() {
        let root = TempDir::new();
        fs::write(root.0.join("Lime_outfit.unitypackage"), b"pkg").unwrap();
        fs::create_dir_all(root.0.join("node_modules")).unwrap();
        fs::write(root.0.join("node_modules").join("hidden.unitypackage"), b"no").unwrap();
        fs::create_dir_all(root.0.join("textures")).unwrap();
        fs::write(root.0.join("textures").join("cloth.psd"), b"img").unwrap();
        fs::create_dir_all(root.0.join("ライム衣装")).unwrap();

        let outcome = scan_folder(&root.0).unwrap();
        let names: Vec<_> = outcome.entries.iter().map(|entry| entry.filename.as_str()).collect();
        assert!(names.contains(&"Lime_outfit.unitypackage"));
        assert!(names.contains(&"cloth.psd"));
        assert!(names.contains(&"ライム衣装"));
        assert!(!names.iter().any(|name| name.contains("hidden")));
        assert!(!names.iter().any(|name| *name == "textures" || *name == "node_modules"));
        assert!(!outcome.truncated);
    }

    #[test]
    fn rejects_drive_root() {
        let root = if cfg!(windows) {
            Path::new(r"C:\")
        } else {
            Path::new("/")
        };
        let error = validate_scan_root(root).unwrap_err();
        assert!(
            error.contains("ドライブ直下"),
            "expected the drive-root message, got {error}"
        );
    }

    #[test]
    fn macos_blocks_system_roots_home_and_volumes_but_not_a_folder_inside() {
        assert_eq!(
            macos_scan_block(Path::new("/System"), None, None),
            Some("システムフォルダはスキャンできません")
        );
        assert_eq!(
            macos_scan_block(Path::new("/System/Library"), None, None),
            Some("システムフォルダはスキャンできません")
        );
        assert_eq!(
            macos_scan_block(Path::new("/Library"), None, None),
            Some("システムフォルダはスキャンできません")
        );
        assert_eq!(
            macos_scan_block(Path::new("/Applications/Safari.app"), None, None),
            Some("システムフォルダはスキャンできません")
        );
        assert_eq!(
            macos_scan_block(Path::new("/private"), None, None),
            Some("システムフォルダはスキャンできません")
        );
        assert_eq!(
            macos_scan_block(Path::new("/private/etc"), None, None),
            Some("システムフォルダはスキャンできません")
        );
        let temp = Path::new("/private/var/folders/ab/cd");
        assert_eq!(
            macos_scan_block(temp.join("vrclibman-scan").as_path(), None, Some(temp)),
            None
        );
        assert_eq!(
            macos_scan_block(Path::new("/Volumes"), None, None),
            Some("ボリュームの直下はスキャンできません。中のフォルダを選んでください")
        );
        assert_eq!(
            macos_scan_block(Path::new("/Volumes/Data/Assets"), None, None),
            None
        );
        let home = Path::new("/Users/kei");
        assert_eq!(
            macos_scan_block(home, Some(home), None),
            Some("ホームフォルダ直下はスキャンできません。中のフォルダを選んでください")
        );
        assert_eq!(macos_scan_block(Path::new("/Users/kei/Assets"), Some(home), None), None);
        assert_eq!(macos_scan_block(Path::new("/Users/other/Assets"), Some(home), None), None);
    }
}
