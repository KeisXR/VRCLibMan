//! Unity エディタの検出。Unity Hub の記録ファイルと既定のインストール先だけを読む。
//! Hub の CLI は起動しない。検出はファイルを読むだけで、Unity を起動しない。

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    Windows,
    Macos,
    Linux,
}

impl Os {
    pub(crate) fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnityEditor {
    /// 例: 2022.3.22f1。パスから読めないときは空文字。
    pub version: String,
    /// Unity 本体の実行ファイル。
    pub path: String,
    /// "hub"（Unity Hub の記録）、"folder"（インストール先のフォルダ）、"manual"（利用者が指定）。
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnityStatus {
    pub editors: Vec<UnityEditor>,
    /// 保存している既定の Unity。未設定なら空文字。
    pub selected_path: String,
    /// 保存したパスに Unity 本体が見つからない。
    pub selected_missing: bool,
    /// Unity Hub の設定フォルダが見つかったか。
    pub hub_found: bool,
    /// 調べたフォルダ。表示用。
    pub searched: Vec<String>,
}

/// 検出に使う場所。テストではダミーのフォルダを渡す。
#[derive(Clone, Debug, Default)]
pub(crate) struct Sources {
    pub hub_dir: Option<PathBuf>,
    pub default_root: Option<PathBuf>,
}

impl Sources {
    pub(crate) fn for_current_os() -> Self {
        let os = Os::current();
        let env = |key: &str| std::env::var_os(key).filter(|value| !value.is_empty()).map(PathBuf::from);
        let home = env("HOME").or_else(|| env("USERPROFILE"));
        match os {
            Os::Windows => Sources {
                hub_dir: env("APPDATA").map(|dir| dir.join("UnityHub")),
                default_root: Some(
                    env("ProgramFiles")
                        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
                        .join("Unity")
                        .join("Hub")
                        .join("Editor"),
                ),
            },
            Os::Macos => Sources {
                hub_dir: home
                    .as_ref()
                    .map(|home| home.join("Library").join("Application Support").join("UnityHub")),
                default_root: Some(PathBuf::from("/Applications/Unity/Hub/Editor")),
            },
            Os::Linux => Sources {
                hub_dir: env("XDG_CONFIG_HOME")
                    .or_else(|| home.as_ref().map(|home| home.join(".config")))
                    .map(|dir| dir.join("UnityHub")),
                default_root: home.map(|home| home.join("Unity").join("Hub").join("Editor")),
            },
        }
    }
}

fn version_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d+\.\d+\.\d+[abfp]\d+(c\d+)?$").expect("valid regex"))
}

pub(crate) fn is_unity_version(text: &str) -> bool {
    version_re().is_match(text.trim())
}

/// 並べ替え用のキー。a < b < f < p。
fn version_key(version: &str) -> Option<(u32, u32, u32, u8, u32, u32)> {
    static PARTS: OnceLock<Regex> = OnceLock::new();
    let caps = PARTS
        .get_or_init(|| Regex::new(r"^(\d+)\.(\d+)\.(\d+)([abfp])(\d+)(?:c(\d+))?$").expect("valid regex"))
        .captures(version.trim())?;
    let num = |i: usize| caps.get(i).and_then(|m| m.as_str().parse::<u32>().ok()).unwrap_or(0);
    let kind = match &caps[4] {
        "a" => 0,
        "b" => 1,
        "f" => 2,
        _ => 3,
    };
    Some((num(1), num(2), num(3), kind, num(5), num(6)))
}

#[derive(Deserialize)]
struct HubV2 {
    #[serde(default)]
    data: Vec<HubEntry>,
}

#[derive(Deserialize)]
struct HubEntry {
    #[serde(default)]
    version: String,
    #[serde(default)]
    location: Location,
}

#[derive(Deserialize, Default)]
#[serde(untagged)]
enum Location {
    Many(Vec<String>),
    One(String),
    #[default]
    None,
}

impl Location {
    fn paths(self) -> Vec<String> {
        match self {
            Location::Many(paths) => paths,
            Location::One(path) => vec![path],
            Location::None => Vec::new(),
        }
    }
}

/// `editors-v2.json`（Hub 3.x）: `{"schema_version":"2","data":[{"version":..,"location":[..]}]}`。
/// 壊れた JSON や想定外の形は空として扱う。
pub(crate) fn parse_editors_v2(json: &str) -> Vec<(String, String)> {
    let Ok(parsed) = serde_json::from_str::<HubV2>(json) else {
        return Vec::new();
    };
    flatten(parsed.data)
}

/// 旧形式の `editors.json`: `{"2019.4.31f1":{"version":..,"location":[..] または ".."}}`。
pub(crate) fn parse_editors_legacy(json: &str) -> Vec<(String, String)> {
    let Ok(map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json) else {
        return Vec::new();
    };
    let entries = map
        .into_iter()
        .filter_map(|(key, value)| {
            let mut entry = serde_json::from_value::<HubEntry>(value).ok()?;
            if entry.version.trim().is_empty() {
                entry.version = key;
            }
            Some(entry)
        })
        .collect();
    flatten(entries)
}

fn flatten(entries: Vec<HubEntry>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in entries {
        let version = entry.version.trim().to_string();
        for path in entry.location.paths() {
            let path = path.trim();
            if !path.is_empty() {
                out.push((version.clone(), path.to_string()));
            }
        }
    }
    out
}

/// `secondaryInstallPath.json` は JSON の文字列 1 つ（例: `"D:\\Unity"`）。空文字は未設定。
pub(crate) fn parse_secondary_install_path(json: &str) -> Option<String> {
    let value = serde_json::from_str::<String>(json.trim()).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// インストール先フォルダ内の、あるバージョンの Unity 本体。
pub(crate) fn executable_in(root: &Path, version: &str, os: Os) -> PathBuf {
    let dir = root.join(version);
    match os {
        Os::Windows => dir.join("Editor").join("Unity.exe"),
        Os::Macos => dir.join("Unity.app").join("Contents").join("MacOS").join("Unity"),
        Os::Linux => dir.join("Editor").join("Unity"),
    }
}

/// Mac の Hub は `Unity.app` を記録することがあるので、中の実行ファイルにする。
pub(crate) fn normalize_location(path: &str, os: Os) -> PathBuf {
    let trimmed = path.trim().trim_end_matches(['/', '\\']);
    let path = PathBuf::from(trimmed);
    if os == Os::Macos
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
    {
        return path.join("Contents").join("MacOS").join("Unity");
    }
    path
}

/// パスの中からバージョンらしいフォルダ名を探す（例: `.../2022.3.22f1/Editor/Unity.exe`）。
pub(crate) fn version_from_path(path: &str) -> String {
    path.split(['/', '\\'])
        .rev()
        .find(|part| is_unity_version(part))
        .unwrap_or_default()
        .to_string()
}

/// 選べる実行ファイル名か。Windows は Unity.exe、Mac と Linux は Unity。
pub(crate) fn is_editor_file_name(path: &Path, os: Os) -> bool {
    let Some(name) = path.to_str().and_then(|text| text.rsplit(['/', '\\']).next()) else {
        return false;
    };
    match os {
        Os::Windows => name.eq_ignore_ascii_case("unity.exe"),
        Os::Macos | Os::Linux => name == "Unity",
    }
}

fn dedupe_key(path: &str, os: Os) -> String {
    let path = path.trim();
    match os {
        Os::Windows => path.replace('/', "\\").to_lowercase(),
        _ => path.to_string(),
    }
}

/// 重複をまとめ、新しいバージョンから並べる。先に来たもの（Hub の記録）を残す。
pub(crate) fn merge_editors(found: Vec<UnityEditor>, os: Os) -> Vec<UnityEditor> {
    let mut seen = HashSet::new();
    let mut editors: Vec<UnityEditor> = found
        .into_iter()
        .filter(|editor| seen.insert(dedupe_key(&editor.path, os)))
        .collect();
    editors.sort_by(|a, b| {
        version_key(&b.version)
            .cmp(&version_key(&a.version))
            .then_with(|| a.path.cmp(&b.path))
    });
    editors
}

/// 実際のフォルダを見て、存在する Unity 本体だけを返す。
pub(crate) fn detect(sources: &Sources, os: Os) -> (Vec<UnityEditor>, bool, Vec<String>) {
    let mut found = Vec::new();
    let mut searched = Vec::new();
    let mut roots = Vec::new();
    let hub_found = sources.hub_dir.as_ref().is_some_and(|dir| dir.is_dir());
    if let Some(hub) = sources.hub_dir.as_ref().filter(|_| hub_found) {
        searched.push(hub.display().to_string());
        let read = |name: &str| std::fs::read_to_string(hub.join(name)).ok();
        let mut entries = read("editors-v2.json").map(|json| parse_editors_v2(&json)).unwrap_or_default();
        entries.extend(read("editors.json").map(|json| parse_editors_legacy(&json)).unwrap_or_default());
        for (version, location) in entries {
            let path = normalize_location(&location, os);
            if path.is_file() {
                let path_text = path.display().to_string();
                let version = if is_unity_version(&version) { version } else { version_from_path(&path_text) };
                found.push(UnityEditor { version, path: path_text, source: "hub".into() });
            }
        }
        if let Some(secondary) = read("secondaryInstallPath.json").and_then(|json| parse_secondary_install_path(&json)) {
            roots.push(PathBuf::from(secondary));
        }
    }
    if let Some(root) = &sources.default_root {
        roots.insert(0, root.clone());
    }
    for root in roots {
        searched.push(root.display().to_string());
        let Ok(children) = std::fs::read_dir(&root) else { continue };
        for child in children.flatten() {
            let name = child.file_name().to_string_lossy().to_string();
            if !is_unity_version(&name) {
                continue;
            }
            let path = executable_in(&root, &name, os);
            if path.is_file() {
                found.push(UnityEditor { version: name, path: path.display().to_string(), source: "folder".into() });
            }
        }
    }
    (merge_editors(found, os), hub_found, searched)
}

/// 利用者が選んだファイルを確かめ、保存するパスを返す。
pub(crate) fn validate_manual(path: &str, os: Os) -> Result<PathBuf, String> {
    let path = normalize_location(path, os);
    let expected = match os {
        Os::Windows => "Unity.exe",
        Os::Macos => "Unity.app",
        Os::Linux => "Unity",
    };
    if !is_editor_file_name(&path, os) {
        return Err(format!("Unity 本体（{expected}）を選んでください"));
    }
    if !path.is_file() {
        return Err(format!("{} が見つかりません", path.display()));
    }
    Ok(path)
}

/// 検出結果と保存済みの選択をまとめる。保存したパスが検出に無くても、存在すれば「手動」として一覧に足す。
pub(crate) fn status(selected: &str, sources: &Sources, os: Os) -> UnityStatus {
    let (mut editors, hub_found, searched) = detect(sources, os);
    let selected_path = selected.trim().to_string();
    let mut selected_missing = false;
    if !selected_path.is_empty() {
        let key = dedupe_key(&selected_path, os);
        if !editors.iter().any(|editor| dedupe_key(&editor.path, os) == key) {
            if Path::new(&selected_path).is_file() {
                editors.push(UnityEditor {
                    version: version_from_path(&selected_path),
                    path: selected_path.clone(),
                    source: "manual".into(),
                });
                editors = merge_editors(editors, os);
            } else {
                selected_missing = true;
            }
        }
    }
    UnityStatus { editors, selected_path, selected_missing, hub_found, searched }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn parses_hub_v2_with_location_arrays() {
        let json = r#"{"schema_version":"2","data":[
            {"version":"2022.3.22f1","location":["C:\\Program Files\\Unity\\Hub\\Editor\\2022.3.22f1\\Editor\\Unity.exe"],"manual":false,"architecture":"x86_64","productName":"Unity"},
            {"version":"6000.0.33f1","location":["D:\\Editors\\6000.0.33f1\\Editor\\Unity.exe","  "],"manual":true}
        ]}"#;
        assert_eq!(
            parse_editors_v2(json),
            vec![
                ("2022.3.22f1".into(), r"C:\Program Files\Unity\Hub\Editor\2022.3.22f1\Editor\Unity.exe".into()),
                ("6000.0.33f1".into(), r"D:\Editors\6000.0.33f1\Editor\Unity.exe".into()),
            ]
        );
        assert!(parse_editors_v2(r#"{"schema_version":"2","data":[]}"#).is_empty());
        assert!(parse_editors_v2("").is_empty());
        assert!(parse_editors_v2("{broken").is_empty());
    }

    #[test]
    fn parses_legacy_editors_with_string_or_array_location() {
        let json = r#"{
            "2019.4.31f1":{"version":"2019.4.31f1","location":["/opt/a/2019.4.31f1/Editor/Unity"],"manual":true},
            "2021.3.8f1":{"location":"/opt/b/Editor/Unity"},
            "bad": 3
        }"#;
        let mut parsed = parse_editors_legacy(json);
        parsed.sort();
        assert_eq!(
            parsed,
            vec![
                ("2019.4.31f1".into(), "/opt/a/2019.4.31f1/Editor/Unity".into()),
                ("2021.3.8f1".into(), "/opt/b/Editor/Unity".into()),
            ]
        );
        assert!(parse_editors_legacy("[]").is_empty());
    }

    #[test]
    fn parses_secondary_install_path() {
        assert_eq!(parse_secondary_install_path(r#""D:\\Unity""#), Some(r"D:\Unity".into()));
        assert_eq!(parse_secondary_install_path(r#""""#), None);
        assert_eq!(parse_secondary_install_path(""), None);
        assert_eq!(parse_secondary_install_path("{}"), None);
    }

    #[test]
    fn builds_executable_paths_per_os() {
        let root = Path::new("root");
        assert_eq!(executable_in(root, "2022.3.22f1", Os::Windows), root.join("2022.3.22f1").join("Editor").join("Unity.exe"));
        assert_eq!(executable_in(root, "2022.3.22f1", Os::Linux), root.join("2022.3.22f1").join("Editor").join("Unity"));
        assert_eq!(
            executable_in(root, "2022.3.22f1", Os::Macos),
            root.join("2022.3.22f1").join("Unity.app").join("Contents").join("MacOS").join("Unity")
        );
        assert_eq!(
            normalize_location("/Applications/Unity/Hub/Editor/2022.3.22f1/Unity.app/", Os::Macos),
            PathBuf::from("/Applications/Unity/Hub/Editor/2022.3.22f1/Unity.app/Contents/MacOS/Unity")
        );
        assert_eq!(normalize_location("/x/Unity.app", Os::Linux), PathBuf::from("/x/Unity.app"));
    }

    #[test]
    fn reads_versions_and_file_names() {
        assert!(is_unity_version("2022.3.22f1"));
        assert!(is_unity_version("6000.0.33f1"));
        assert!(is_unity_version("2021.3.8f1c1"));
        assert!(is_unity_version("2023.1.0b5"));
        assert!(!is_unity_version("Editor"));
        assert!(!is_unity_version("2022.3"));
        assert_eq!(version_from_path(r"C:\Program Files\Unity\Hub\Editor\2022.3.22f1\Editor\Unity.exe"), "2022.3.22f1");
        assert_eq!(version_from_path("/opt/unity/Editor/Unity"), "");
        assert!(is_editor_file_name(Path::new(r"C:\x\UNITY.EXE"), Os::Windows));
        assert!(!is_editor_file_name(Path::new(r"C:\x\Unity Hub.exe"), Os::Windows));
        assert!(is_editor_file_name(Path::new("/x/Unity"), Os::Linux));
        assert!(!is_editor_file_name(Path::new("/x/unityhub"), Os::Linux));
    }

    #[test]
    fn merges_duplicates_and_sorts_newest_first() {
        let editor = |version: &str, path: &str, source: &str| UnityEditor { version: version.into(), path: path.into(), source: source.into() };
        let merged = merge_editors(
            vec![
                editor("2022.3.22f1", r"C:\U\2022.3.22f1\Editor\Unity.exe", "hub"),
                editor("2022.3.22f1", "c:/u/2022.3.22f1/Editor/Unity.exe", "folder"),
                editor("6000.0.33f1", r"D:\E\Unity.exe", "hub"),
                editor("2022.3.6f1", r"C:\U\2022.3.6f1\Editor\Unity.exe", "folder"),
                editor("", r"E:\odd\Unity.exe", "manual"),
            ],
            Os::Windows,
        );
        let versions: Vec<_> = merged.iter().map(|e| (e.version.as_str(), e.source.as_str())).collect();
        assert_eq!(versions, vec![("6000.0.33f1", "hub"), ("2022.3.22f1", "hub"), ("2022.3.6f1", "folder"), ("", "manual")]);
    }

    #[test]
    fn detects_from_dummy_hub_and_install_folders() {
        let dir = tempfile::tempdir().unwrap();
        let hub = dir.path().join("UnityHub");
        let default_root = dir.path().join("Hub").join("Editor");
        let secondary = dir.path().join("Secondary");
        let os = Os::Linux;
        // Hub に記録され、実在する
        let recorded = dir.path().join("Elsewhere").join("2022.3.22f1").join("Editor").join("Unity");
        touch(&recorded);
        // Hub に記録されているが、消えている
        let stale = dir.path().join("Gone").join("2019.4.31f1").join("Editor").join("Unity");
        // 既定と追加のインストール先
        touch(&executable_in(&default_root, "2022.3.6f1", os));
        touch(&executable_in(&secondary, "6000.0.33f1", os));
        std::fs::create_dir_all(default_root.join("not-a-version")).unwrap();
        std::fs::create_dir_all(default_root.join("2021.3.1f1")).unwrap(); // 本体なし
        std::fs::create_dir_all(&hub).unwrap();
        std::fs::write(
            hub.join("editors-v2.json"),
            serde_json::json!({"schema_version":"2","data":[
                {"version":"2022.3.22f1","location":[recorded.display().to_string()]},
                {"version":"2019.4.31f1","location":[stale.display().to_string()]}
            ]})
            .to_string(),
        )
        .unwrap();
        std::fs::write(hub.join("secondaryInstallPath.json"), serde_json::to_string(&secondary.display().to_string()).unwrap()).unwrap();

        let sources = Sources { hub_dir: Some(hub.clone()), default_root: Some(default_root.clone()) };
        let (editors, hub_found, searched) = detect(&sources, os);
        assert!(hub_found);
        assert_eq!(searched.len(), 3);
        let summary: Vec<_> = editors.iter().map(|e| (e.version.as_str(), e.source.as_str())).collect();
        assert_eq!(summary, vec![("6000.0.33f1", "folder"), ("2022.3.22f1", "hub"), ("2022.3.6f1", "folder")]);

        // 検出に無いが実在するファイルは手動として足す。消えたパスは missing。
        let manual = dir.path().join("Custom").join("Editor").join("Unity");
        touch(&manual);
        let with_manual = status(&manual.display().to_string(), &sources, os);
        assert!(!with_manual.selected_missing);
        assert!(with_manual.editors.iter().any(|e| e.source == "manual" && e.path == manual.display().to_string()));
        let missing = status(&stale.display().to_string(), &sources, os);
        assert!(missing.selected_missing);
        assert_eq!(missing.editors.len(), 3);
        let unset = status("", &Sources::default(), os);
        assert!(unset.editors.is_empty() && !unset.hub_found && !unset.selected_missing);
    }

    #[test]
    fn validates_manual_selection() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("2022.3.22f1").join("Editor").join("Unity");
        touch(&exe);
        assert_eq!(validate_manual(&exe.display().to_string(), Os::Linux).unwrap(), exe);
        assert!(validate_manual(&dir.path().join("Unity").display().to_string(), Os::Linux).is_err());
        let other = dir.path().join("unityhub");
        touch(&other);
        assert!(validate_manual(&other.display().to_string(), Os::Linux).is_err());
        let app = dir.path().join("Unity.app");
        touch(&app.join("Contents").join("MacOS").join("Unity"));
        assert_eq!(
            validate_manual(&app.display().to_string(), Os::Macos).unwrap(),
            app.join("Contents").join("MacOS").join("Unity")
        );
    }
}
