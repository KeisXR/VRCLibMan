//! Unity プロジェクトの一覧（docs/unity-projects.md の段階 1）。
//!
//! - 一覧は ALCOM / VCC の `settings.json` の `userProjects` と、利用者が手で足したフォルダ。
//!   ALCOM の一覧は DB に写さず、毎回読む。
//! - プロジェクトのフォルダは読むだけ。`ProjectSettings/ProjectVersion.txt` と
//!   `Packages/vpm-manifest.json` のほかは開かない。
//! - 開くのは、プロジェクトの版とちょうど同じ版の Unity だけ。違う版で開くと Unity が
//!   プロジェクトを変換し、元に戻せないため。
//! - プロジェクトの名前とパスはログに書かない。

use crate::unity::{Os, UnityEditor, dedupe_key, is_unity_version};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 1 件の読み取りの上限。切れた共有や外したドライブで画面を止めないため。
pub(crate) const READ_TIMEOUT: Duration = Duration::from_secs(2);
/// 一覧全体の上限。過ぎたら残りは読まずに「確認できません」にする。
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(5);

const VERSION_FILE: &str = "ProjectSettings/ProjectVersion.txt";
const VPM_MANIFEST: &str = "Packages/vpm-manifest.json";

/// 一覧を読む場所。テストではダミーのファイルを渡す。
#[derive(Clone, Debug, Default)]
pub(crate) struct Sources {
    /// ALCOM / VCC の `settings.json`。
    pub alcom_settings: Option<PathBuf>,
}

impl Sources {
    /// Windows は確かめた場所。Mac と Linux は vrc-get が使う場所（データ用のフォルダ）で、未確認。
    pub(crate) fn for_current_os() -> Self {
        let env = |key: &str| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let home = env("HOME").or_else(|| env("USERPROFILE"));
        let data = match Os::current() {
            Os::Windows => env("LOCALAPPDATA"),
            Os::Macos => home.map(|home| home.join("Library").join("Application Support")),
            Os::Linux => {
                env("XDG_DATA_HOME").or_else(|| home.map(|home| home.join(".local").join("share")))
            }
        };
        Sources {
            alcom_settings: data
                .map(|dir| dir.join("VRChatCreatorCompanion").join("settings.json")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VpmPackage {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectAvatar {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnityProject {
    /// パスから決めた 16 桁の 16 進。URL と DB のキーに使う。
    pub id: String,
    /// フォルダ名。Unity の窓のタイトルと同じ。
    pub name: String,
    pub path: String,
    /// "ok"、"missing"（プロジェクトの形のフォルダが無い）、"timeout"（読めなかった）。
    pub state: &'static str,
    /// 例: 2022.3.22f1。読めなければ空。
    pub version: String,
    /// "avatar"、"world"、"unknown"。
    pub kind: &'static str,
    pub packages: Vec<VpmPackage>,
    /// "alcom"（ALCOM / VCC の一覧）、"manual"（手で足した）、"linked"（結び付けたときに覚えた）。
    pub source: &'static str,
    pub hidden: bool,
    /// "open"、"maybe"（同じ名前と版のプロジェクトがほかにもある）、"closed"、"unknown"（調べられない OS）。
    pub running: &'static str,
    /// プロジェクトの版と同じ版の Unity が見つかった。
    pub editor_available: bool,
    pub avatars: Vec<ProjectAvatar>,
    /// `Assets` の更新日時（UNIX 秒）。読めなければ `None`。
    pub modified: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnityProjectList {
    pub projects: Vec<UnityProject>,
    /// ALCOM / VCC の設定ファイルを読めた。
    pub alcom_found: bool,
    /// 読んでいる設定ファイル。表示用。
    pub alcom_path: String,
}

/// DB に残しているプロジェクトの行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StoredProject {
    pub id: String,
    pub path: String,
    pub manual: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OpenResult {
    /// すでに開いていたので起動しなかった。
    pub already_open: bool,
}

/// 前後の空白と引用符、末尾の区切りを外す。Windows では `/` を `\` にそろえる。
pub(crate) fn clean_path(path: &str, os: Os) -> String {
    let trimmed = path.trim().trim_matches('"').trim();
    let text = match os {
        Os::Windows => trimmed.replace('/', "\\"),
        _ => trimmed.to_string(),
    };
    let mut text = text.as_str();
    while text.len() > 1 && (text.ends_with('\\') || text.ends_with('/')) {
        // `C:\` と `/` はそのまま。
        if os == Os::Windows && text.len() == 3 && text.as_bytes()[1] == b':' {
            break;
        }
        text = &text[..text.len() - 1];
    }
    text.to_string()
}

/// 正規化したパスから決める id。FNV-1a 64 bit。
pub(crate) fn project_id(path: &str, os: Os) -> String {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x00000100000001b3;
    let key = dedupe_key(&clean_path(path, os), os);
    let mut hash = OFFSET;
    for byte in b"vrclibman-unity-project\0".iter().chain(key.as_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}

pub(crate) fn valid_project_id(id: &str) -> bool {
    id.len() == 16
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// `settings.json` の `userProjects`。文字列でない要素と空の文字列は捨てる。壊れた JSON は空。
pub(crate) fn parse_user_projects(json: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(strip_bom(json)) else {
        return Vec::new();
    };
    value
        .get("userProjects")
        .and_then(|projects| projects.as_array())
        .map(|projects| {
            projects
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `ProjectVersion.txt` の `m_EditorVersion`。Unity の版の形でなければ `None`。
pub(crate) fn parse_project_version(text: &str) -> Option<String> {
    strip_bom(text).lines().find_map(|line| {
        let version = line.trim().strip_prefix("m_EditorVersion:")?.trim();
        is_unity_version(version).then(|| version.to_string())
    })
}

/// `vpm-manifest.json` の入っているパッケージ（`locked`、無ければ `dependencies`）と、それから決まる種類。
pub(crate) fn parse_vpm_manifest(json: &str) -> (&'static str, Vec<VpmPackage>) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(strip_bom(json)) else {
        return ("unknown", Vec::new());
    };
    let section = ["locked", "dependencies"]
        .iter()
        .find_map(|key| value.get(*key).and_then(|section| section.as_object()))
        .filter(|section| !section.is_empty());
    let mut packages: Vec<VpmPackage> = section
        .map(|section| {
            section
                .iter()
                .filter(|(name, _)| !name.trim().is_empty())
                .map(|(name, entry)| VpmPackage {
                    name: name.clone(),
                    version: entry
                        .get("version")
                        .and_then(|version| version.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    let has = |name: &str| packages.iter().any(|package| package.name == name);
    let kind = if has("com.vrchat.avatars") {
        "avatar"
    } else if has("com.vrchat.worlds") {
        "world"
    } else {
        "unknown"
    };
    (kind, packages)
}

/// `Assets` と `ProjectVersion.txt` のあるフォルダだけをプロジェクトとする。
pub(crate) fn is_project_dir(path: &Path) -> bool {
    path.join("Assets").is_dir() && path.join(VERSION_FILE).is_file()
}

/// プロジェクト 1 件から読んだもの。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectFacts {
    pub version: String,
    pub kind: &'static str,
    pub packages: Vec<VpmPackage>,
    pub modified: Option<i64>,
}

/// フォルダを読む。プロジェクトの形でなければ `None`。
pub(crate) fn read_project(path: &Path) -> Option<ProjectFacts> {
    if !is_project_dir(path) {
        return None;
    }
    let version = std::fs::read_to_string(path.join(VERSION_FILE))
        .ok()
        .and_then(|text| parse_project_version(&text))
        .unwrap_or_default();
    let (kind, packages) = std::fs::read_to_string(path.join(VPM_MANIFEST))
        .map(|json| parse_vpm_manifest(&json))
        .unwrap_or(("unknown", Vec::new()));
    let modified = std::fs::metadata(path.join("Assets"))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_secs() as i64);
    Some(ProjectFacts {
        version,
        kind,
        packages,
        modified,
    })
}

/// 読み取りの結果。`Err(())` は時間切れ。
type Read = Result<Option<ProjectFacts>, ()>;

/// プロジェクトを順に読む。1 件 `per_item`、全体 `total` まで。過ぎたものは時間切れ。
pub(crate) fn read_projects(paths: &[String], per_item: Duration, total: Duration) -> Vec<Read> {
    let started = Instant::now();
    paths
        .iter()
        .map(|path| {
            let left = total.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err(());
            }
            let owned = PathBuf::from(path);
            crate::library_files::wait_for(per_item.min(left), move || read_project(&owned))
                .ok_or(())
        })
        .collect()
}

/// 一覧を組み立てる入力。
pub(crate) struct ListInput<'a> {
    pub alcom: Option<&'a [String]>,
    pub stored: &'a [StoredProject],
    pub links: &'a HashMap<String, Vec<ProjectAvatar>>,
    pub editors: &'a [UnityEditor],
    /// 起動中の Unity の（窓の見出し `Project - Scene - Platform`、版）。調べられない OS では `None`。
    pub running: Option<&'a [(String, String)]>,
    pub os: Os,
}

/// 並べる順の候補。ALCOM の並び、次に DB の行。同じ id は最初の 1 つだけ。
pub(crate) fn candidates(input: &ListInput<'_>) -> Vec<(String, String, &'static str)> {
    let manual: HashSet<&str> = input
        .stored
        .iter()
        .filter(|row| row.manual)
        .map(|row| row.id.as_str())
        .collect();
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for path in input.alcom.unwrap_or(&[]) {
        let path = clean_path(path, input.os);
        let id = project_id(&path, input.os);
        if seen.insert(id.clone()) {
            found.push((id, path, "alcom"));
        }
    }
    for row in input.stored {
        // アバターを消したあとに残った、印も結び付けも無い行は出さない。
        let needed = row.manual || row.hidden || input.links.contains_key(&row.id);
        if needed && seen.insert(row.id.clone()) {
            let source = if manual.contains(row.id.as_str()) {
                "manual"
            } else {
                "linked"
            };
            found.push((row.id.clone(), row.path.clone(), source));
        }
    }
    found
}

fn folder_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// 窓の見出し（`Project - Scene - Platform`）がこのプロジェクト名で始まるか。
/// プロジェクト名にも「 - 」が入り得るので、見出しを区切らずに先頭で比べる。
fn heading_names(heading: &str, name: &str) -> bool {
    heading == name
        || heading
            .strip_prefix(name)
            .is_some_and(|rest| rest.starts_with(" - "))
}

/// 開いているかの判定。窓の見出しと版で照らす。1 つの窓に当てはまるプロジェクトが
/// 2 つ以上あるとき（同じ名前と版、`Avatar` と `Avatar - Main` など）は決めず `maybe` にする。
/// `projects` は（名前、版）で、結果も同じ並び。
fn running_states(
    projects: &[(String, String)],
    running: Option<&[(String, String)]>,
) -> Vec<&'static str> {
    let Some(running) = running else {
        return vec!["unknown"; projects.len()];
    };
    let mut states = vec!["closed"; projects.len()];
    for (heading, version) in running {
        let matched: Vec<usize> = projects
            .iter()
            .enumerate()
            .filter(|(_, (name, project_version))| {
                !project_version.is_empty()
                    && project_version == version
                    && heading_names(heading, name)
            })
            .map(|(index, _)| index)
            .collect();
        let state = if matched.len() == 1 { "open" } else { "maybe" };
        for index in matched {
            // ほかの窓で「開いている」と決まったものは、そのままにする。
            if states[index] != "open" {
                states[index] = state;
            }
        }
    }
    states
}

/// 版がちょうど同じ Unity。
pub(crate) fn matching_editor<'a>(
    editors: &'a [UnityEditor],
    version: &str,
) -> Option<&'a UnityEditor> {
    if version.is_empty() {
        return None;
    }
    editors.iter().find(|editor| editor.version == version)
}

/// 候補と読み取りの結果から一覧を作る。`reads` は `candidates` と同じ並び。
pub(crate) fn build_list(
    input: &ListInput<'_>,
    candidates: &[(String, String, &'static str)],
    reads: &[Read],
) -> Vec<UnityProject> {
    let hidden: HashSet<&str> = input
        .stored
        .iter()
        .filter(|row| row.hidden)
        .map(|row| row.id.as_str())
        .collect();
    let names: Vec<(String, String)> = candidates
        .iter()
        .zip(reads)
        .map(|((_, path, _), read)| {
            let version = match read {
                Ok(Some(facts)) => facts.version.clone(),
                _ => String::new(),
            };
            (folder_name(path), version)
        })
        .collect();
    let states = running_states(&names, input.running);
    let mut projects: Vec<UnityProject> = candidates
        .iter()
        .zip(reads)
        .zip(states)
        .map(|(((id, path, source), read), running)| {
            let name = folder_name(path);
            let (state, facts) = match read {
                Ok(Some(facts)) => ("ok", Some(facts)),
                Ok(None) => ("missing", None),
                Err(()) => ("timeout", None),
            };
            let version = facts.map(|facts| facts.version.clone()).unwrap_or_default();
            UnityProject {
                id: id.clone(),
                running,
                editor_available: matching_editor(input.editors, &version).is_some(),
                name,
                path: path.clone(),
                state,
                kind: facts.map(|facts| facts.kind).unwrap_or("unknown"),
                packages: facts
                    .map(|facts| facts.packages.clone())
                    .unwrap_or_default(),
                source,
                hidden: hidden.contains(id.as_str()),
                avatars: input.links.get(id).cloned().unwrap_or_default(),
                modified: facts.and_then(|facts| facts.modified),
                version,
            }
        })
        .collect();
    // 新しく触ったものから。読めなかったものは後ろ。
    projects.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.cmp(&b.name))
    });
    projects
}

/// 商品を渡せそうなプロジェクト（「Unityで開く」で Unity が起動していないときの候補）。
/// 商品の対応アバターに結び付けたもの、ワールドの商品ならワールドのプロジェクト（結び付けは要らない）。
/// 隠したものは出さない。並びは一覧のまま。
pub(crate) fn projects_for_product(
    projects: Vec<UnityProject>,
    avatar_ids: &HashSet<String>,
    world_product: bool,
) -> Vec<UnityProject> {
    projects
        .into_iter()
        .filter(|project| {
            !project.hidden
                && (project
                    .avatars
                    .iter()
                    .any(|avatar| avatar_ids.contains(&avatar.id))
                    || (world_product && project.kind == "world"))
        })
        .collect()
}

/// `<Unity> -projectPath <フォルダ>` の引数。シェルを通さない。
pub(crate) fn project_command(editor: &Path, project: &Path) -> Command {
    let mut command = Command::new(editor);
    command
        .arg("-projectPath")
        .arg(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(dir) = editor.parent() {
        command.current_dir(dir);
    }
    command
}

/// 開く前の確認。開いていれば起動しない。版の合う Unity が無ければ開かない。
pub(crate) fn editor_for_open<'a>(
    project: &UnityProject,
    editors: &'a [UnityEditor],
) -> Result<Option<&'a UnityEditor>, String> {
    match project.state {
        "missing" => return Err("プロジェクトのフォルダが見つかりません".into()),
        "timeout" => {
            return Err(
                "プロジェクトのフォルダを読めませんでした。ドライブや共有の接続を確かめてください"
                    .into(),
            );
        }
        _ => {}
    }
    if project.running == "open" {
        return Ok(None);
    }
    if project.version.is_empty() {
        return Err(
            "プロジェクトの Unity の版が読めません。ALCOM か Unity Hub で開いてください".into(),
        );
    }
    matching_editor(editors, &project.version)
        .map(Some)
        .ok_or_else(|| {
            format!(
                "このプロジェクトは Unity {} で作られています。同じ版の Unity が見つからないので、ALCOM か Unity Hub で開いてください",
                project.version
            )
        })
}

/// Unity を起動する。Unity 本体は終わらないので待たない。
pub(crate) fn launch(editor: &Path, project: &Path) -> Result<(), String> {
    let mut child = project_command(editor, project).spawn().map_err(|_| {
        "Unity を起動できませんでした。設定の「Unity」を確かめてください".to_string()
    })?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// 手で足すフォルダを確かめる。
pub(crate) fn validate_new_project(path: &str, os: Os) -> Result<String, String> {
    let path = clean_path(path, os);
    if path.is_empty() || !Path::new(&path).is_absolute() {
        return Err("プロジェクトのフォルダを選んでください".into());
    }
    if Path::new(&path)
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("プロジェクトのフォルダを選んでください".into());
    }
    let owned = PathBuf::from(&path);
    match crate::library_files::wait_for(READ_TIMEOUT, move || is_project_dir(&owned)) {
        Some(true) => Ok(path),
        Some(false) => Err(
            "Unity のプロジェクトではありません。Assets と ProjectSettings のあるフォルダを選んでください"
                .into(),
        ),
        None => Err("フォルダを読めませんでした。ドライブや共有の接続を確かめてください".into()),
    }
}

/// 起動中の Unity の（窓の見出し `Project - Scene - Platform`、版）。Windows 以外では `None`。
pub(crate) fn running_projects() -> Option<Vec<(String, String)>> {
    if Os::current() != Os::Windows {
        return None;
    }
    Some(
        crate::unity_import::running_instances()
            .into_iter()
            .map(|instance| (instance.heading, instance.version))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(version: &str) -> UnityEditor {
        UnityEditor {
            version: version.into(),
            path: format!("C:\\Unity\\{version}\\Editor\\Unity.exe"),
            source: "hub".into(),
        }
    }

    fn make_project(root: &Path, name: &str, version: &str, manifest: Option<&str>) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("Assets")).unwrap();
        std::fs::create_dir_all(dir.join("ProjectSettings")).unwrap();
        std::fs::write(
            dir.join(VERSION_FILE),
            format!("m_EditorVersion: {version}\nm_EditorVersionWithRevision: {version} (abc)\n"),
        )
        .unwrap();
        if let Some(manifest) = manifest {
            std::fs::create_dir_all(dir.join("Packages")).unwrap();
            std::fs::write(dir.join(VPM_MANIFEST), manifest).unwrap();
        }
        dir
    }

    #[test]
    fn user_projects_are_strings_only() {
        let json = "\u{feff}{\"userProjects\":[\"D:\\\\ALCOM\\\\Kip2\",3,\"\",\"  \",null,\"C:\\\\a\"],\"other\":1}";
        assert_eq!(parse_user_projects(json), vec!["D:\\ALCOM\\Kip2", "C:\\a"]);
        assert!(parse_user_projects("{").is_empty());
        assert!(parse_user_projects("{\"userProjects\":\"x\"}").is_empty());
        assert!(parse_user_projects("[]").is_empty());
    }

    #[test]
    fn project_version_needs_a_unity_version() {
        assert_eq!(
            parse_project_version("m_EditorVersion: 2022.3.22f1\nm_EditorVersionWithRevision: x"),
            Some("2022.3.22f1".into())
        );
        assert_eq!(parse_project_version("m_EditorVersion: latest"), None);
        assert_eq!(parse_project_version(""), None);
    }

    #[test]
    fn manifest_decides_the_kind() {
        let (kind, packages) = parse_vpm_manifest(
            r#"{"dependencies":{"com.vrchat.avatars":{"version":"3.10.6"}},
                "locked":{"com.vrchat.base":{"version":"3.10.6"},"com.vrchat.avatars":{"version":"3.10.6"},"jp.lilxyzw.liltoon":{"version":"2.4.1"}}}"#,
        );
        assert_eq!(kind, "avatar");
        assert_eq!(
            packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            [
                "com.vrchat.avatars",
                "com.vrchat.base",
                "jp.lilxyzw.liltoon"
            ]
        );
        assert_eq!(packages[2].version, "2.4.1");
        let (kind, packages) =
            parse_vpm_manifest(r#"{"dependencies":{"com.vrchat.worlds":{"version":"3.7.0"}}}"#);
        assert_eq!((kind, packages.len()), ("world", 1));
        assert_eq!(parse_vpm_manifest("{}").0, "unknown");
        assert_eq!(parse_vpm_manifest("not json").0, "unknown");
    }

    #[test]
    fn ids_follow_the_normalized_path() {
        let a = project_id("D:\\ALCOM\\Kip2", Os::Windows);
        assert_eq!(a.len(), 16);
        assert!(valid_project_id(&a));
        assert_eq!(a, project_id("d:/alcom/kip2/", Os::Windows));
        assert_eq!(a, project_id(" \"D:\\ALCOM\\Kip2\" ", Os::Windows));
        assert_ne!(a, project_id("D:\\ALCOM\\Kip3", Os::Windows));
        assert_ne!(
            project_id("/home/a/Kip", Os::Linux),
            project_id("/home/a/kip", Os::Linux)
        );
        assert!(!valid_project_id("../../etc"));
        assert_eq!(clean_path("C:\\", Os::Windows), "C:\\");
        assert_eq!(clean_path("/", Os::Linux), "/");
    }

    #[test]
    fn only_project_shaped_folders_are_read() {
        let root = tempfile::tempdir().unwrap();
        let project = make_project(
            root.path(),
            "Plum",
            "2022.3.22f1",
            Some(r#"{"locked":{"com.vrchat.avatars":{"version":"3.10.6"}}}"#),
        );
        let facts = read_project(&project).unwrap();
        assert_eq!(facts.version, "2022.3.22f1");
        assert_eq!(facts.kind, "avatar");
        assert!(facts.modified.is_some());
        std::fs::create_dir_all(root.path().join("NotProject").join("Assets")).unwrap();
        assert!(read_project(&root.path().join("NotProject")).is_none());
        assert!(read_project(&root.path().join("Gone")).is_none());
        let bare = make_project(root.path(), "Bare", "2022.3.22f1", None);
        assert_eq!(read_project(&bare).unwrap().kind, "unknown");
    }

    #[test]
    fn list_merges_sources_and_marks_state() {
        let root = tempfile::tempdir().unwrap();
        let plum = make_project(root.path(), "Plum", "2022.3.22f1", None);
        let old = make_project(root.path(), "Old", "2019.4.31f1", None);
        let plum_text = plum.display().to_string();
        let gone = root.path().join("Gone").display().to_string();
        let manual = make_project(root.path(), "Hand", "2022.3.22f1", None)
            .display()
            .to_string();
        let alcom = vec![
            plum_text.clone(),
            old.display().to_string(),
            gone.clone(),
            plum_text.to_uppercase(),
        ];
        let os = Os::current();
        let stored = vec![
            StoredProject {
                id: project_id(&manual, os),
                path: manual.clone(),
                manual: true,
                hidden: false,
            },
            StoredProject {
                id: project_id(&gone, os),
                path: gone.clone(),
                manual: false,
                hidden: true,
            },
        ];
        let mut links = HashMap::new();
        links.insert(
            project_id(&plum_text, os),
            vec![ProjectAvatar {
                id: "plum".into(),
                name: "プラム".into(),
            }],
        );
        let editors = vec![editor("2022.3.22f1")];
        let running = vec![("Plum".to_string(), "2022.3.22f1".to_string())];
        let input = ListInput {
            alcom: Some(&alcom),
            stored: &stored,
            links: &links,
            editors: &editors,
            running: Some(&running),
            os,
        };
        let found = candidates(&input);
        // Windows では大文字の重複がまとまる。
        let expected = if os == Os::Windows { 4 } else { 5 };
        assert_eq!(found.len(), expected);
        let paths: Vec<String> = found.iter().map(|(_, path, _)| path.clone()).collect();
        let reads = read_projects(&paths, READ_TIMEOUT, LIST_TIMEOUT);
        let list = build_list(&input, &found, &reads);
        let by_name = |name: &str| list.iter().find(|p| p.name == name).unwrap();
        let plum = by_name("Plum");
        assert_eq!(plum.source, "alcom");
        assert_eq!(plum.running, "open");
        assert!(plum.editor_available);
        assert_eq!(plum.avatars[0].id, "plum");
        let old = by_name("Old");
        assert!(!old.editor_available);
        assert_eq!(old.running, "closed");
        assert_eq!(by_name("Hand").source, "manual");
        let gone = by_name("Gone");
        assert_eq!(gone.state, "missing");
        assert!(gone.hidden);
        assert_eq!(gone.source, "alcom");
    }

    #[test]
    fn same_name_and_version_is_not_decided() {
        let root = tempfile::tempdir().unwrap();
        let a = make_project(&root.path().join("a"), "Plum", "2022.3.22f1", None);
        let b = make_project(&root.path().join("b"), "Plum", "2022.3.22f1", None);
        let alcom = vec![a.display().to_string(), b.display().to_string()];
        let running = vec![("Plum".to_string(), "2022.3.22f1".to_string())];
        let input = ListInput {
            alcom: Some(&alcom),
            stored: &[],
            links: &HashMap::new(),
            editors: &[],
            running: Some(&running),
            os: Os::current(),
        };
        let found = candidates(&input);
        let paths: Vec<String> = found.iter().map(|(_, path, _)| path.clone()).collect();
        let list = build_list(
            &input,
            &found,
            &read_projects(&paths, READ_TIMEOUT, LIST_TIMEOUT),
        );
        assert!(list.iter().all(|project| project.running == "maybe"));
        let unknown = ListInput {
            running: None,
            ..input
        };
        let list = build_list(
            &unknown,
            &found,
            &read_projects(&paths, READ_TIMEOUT, LIST_TIMEOUT),
        );
        assert!(list.iter().all(|project| project.running == "unknown"));
    }

    #[test]
    fn opening_needs_the_exact_version() {
        let mut project = UnityProject {
            id: "0".repeat(16),
            name: "Plum".into(),
            path: "C:\\Plum".into(),
            state: "ok",
            version: "2022.3.22f1".into(),
            kind: "avatar",
            packages: vec![],
            source: "alcom",
            hidden: false,
            running: "closed",
            editor_available: true,
            avatars: vec![],
            modified: None,
        };
        let editors = vec![editor("2022.3.6f1"), editor("2022.3.22f1")];
        assert_eq!(
            editor_for_open(&project, &editors)
                .unwrap()
                .unwrap()
                .version,
            "2022.3.22f1"
        );
        let other = vec![editor("2022.3.6f1")];
        let error = editor_for_open(&project, &other).unwrap_err();
        assert!(error.contains("2022.3.22f1"));
        project.running = "open";
        assert!(editor_for_open(&project, &other).unwrap().is_none());
        project.running = "maybe";
        assert!(editor_for_open(&project, &other).is_err());
        project.state = "missing";
        assert!(editor_for_open(&project, &editors).is_err());
    }

    #[test]
    fn world_products_get_world_projects_without_links() {
        let base = |id: &str, kind: &'static str, avatars: &[&str], hidden: bool| UnityProject {
            id: id.into(),
            name: id.into(),
            path: format!("C:\\{id}"),
            state: "ok",
            version: "2022.3.22f1".into(),
            kind,
            packages: vec![],
            source: "alcom",
            hidden,
            running: "closed",
            editor_available: true,
            avatars: avatars
                .iter()
                .map(|avatar| ProjectAvatar {
                    id: (*avatar).into(),
                    name: (*avatar).into(),
                })
                .collect(),
            modified: None,
        };
        let list = || {
            vec![
                base("Plum", "avatar", &["plum"], false),
                base("Room", "world", &[], false),
                base("OldRoom", "world", &[], true),
                base("Lime", "avatar", &["lime"], false),
                base("Mixed", "unknown", &[], false),
            ]
        };
        let names =
            |found: Vec<UnityProject>| found.into_iter().map(|p| p.name).collect::<Vec<_>>();
        let plum: HashSet<String> = ["plum".to_string()].into();
        assert_eq!(names(projects_for_product(list(), &plum, false)), ["Plum"]);
        assert_eq!(
            names(projects_for_product(list(), &HashSet::new(), true)),
            ["Room"]
        );
        assert_eq!(
            names(projects_for_product(list(), &plum, true)),
            ["Plum", "Room"]
        );
        assert!(projects_for_product(list(), &HashSet::new(), false).is_empty());
    }

    #[test]
    fn project_command_passes_the_folder_as_one_argument() {
        // パスは OS の区切りで組み立てる（Windows の書き方では、ほかの OS で親のフォルダが空になる）。
        let editor_dir = Path::new("Unity").join("Editor");
        let project = Path::new("My Projects").join("Plum & Lime");
        let command = project_command(&editor_dir.join("Unity.exe"), &project);
        let args: Vec<_> = command
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            ["-projectPath".to_string(), project.display().to_string()]
        );
        assert_eq!(command.get_current_dir(), Some(editor_dir.as_path()));
    }

    #[test]
    fn project_names_with_dashes_match_the_whole_window_heading() {
        let projects = |names: &[&str]| -> Vec<(String, String)> {
            names
                .iter()
                .map(|name| (name.to_string(), "2022.3.22f1".to_string()))
                .collect()
        };
        let window = |heading: &str| vec![(heading.to_string(), "2022.3.22f1".to_string())];
        // 「Avatar - Main」を開いている。名前を「 - 」で区切ると「Avatar」になってしまう。
        let open = window("Avatar - Main - SampleScene - Windows, Mac, Linux");
        assert_eq!(
            running_states(&projects(&["Avatar - Main", "Other"]), Some(&open)),
            ["open", "closed"]
        );
        // 「Avatar」と「Avatar - Main」の両方があると、どちらの窓か決められない。
        assert_eq!(
            running_states(&projects(&["Avatar", "Avatar - Main"]), Some(&open)),
            ["maybe", "maybe"]
        );
        // 名前の途中で切れているものは当てない。版が違うものも当てない。
        assert_eq!(
            running_states(
                &projects(&["Avatar - Ma", "Avatar"]),
                Some(&window("Avatar - Main - S - P"))
            ),
            ["closed", "open"]
        );
        let other_version = vec![("Avatar - S - P".to_string(), "2019.4.31f1".to_string())];
        assert_eq!(
            running_states(&projects(&["Avatar"]), Some(&other_version)),
            ["closed"]
        );
        assert_eq!(running_states(&projects(&["Avatar"]), None), ["unknown"]);
    }

    #[test]
    fn new_projects_must_be_project_folders() {
        let root = tempfile::tempdir().unwrap();
        let project = make_project(root.path(), "Plum", "2022.3.22f1", None);
        let os = Os::current();
        assert!(validate_new_project(&project.display().to_string(), os).is_ok());
        assert!(validate_new_project(&root.path().display().to_string(), os).is_err());
        assert!(validate_new_project("relative\\path", os).is_err());
        assert!(validate_new_project("", os).is_err());
        let sneaky = format!(
            "{}{}..{}Plum",
            project.display(),
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        assert!(validate_new_project(&sneaky, os).is_err());
    }

    /// この PC の ALCOM の一覧を読むだけの確認。`cargo test -- --ignored real_alcom` で動かす。
    /// 件数と状態だけを出し、名前とパスは出さない。
    #[test]
    #[ignore]
    fn real_alcom_projects() {
        let sources = Sources::for_current_os();
        let path = sources.alcom_settings.expect("no data folder");
        let json = std::fs::read_to_string(&path).expect("ALCOM settings not found");
        let alcom = parse_user_projects(&json);
        let os = Os::current();
        let editors =
            crate::unity::status("", &crate::unity::Sources::for_current_os(), os).editors;
        let running = running_projects();
        let input = ListInput {
            alcom: Some(&alcom),
            stored: &[],
            links: &HashMap::new(),
            editors: &editors,
            running: running.as_deref(),
            os,
        };
        let found = candidates(&input);
        let paths: Vec<String> = found.iter().map(|(_, path, _)| path.clone()).collect();
        let started = Instant::now();
        let reads = read_projects(&paths, READ_TIMEOUT, LIST_TIMEOUT);
        let took = started.elapsed();
        let list = build_list(&input, &found, &reads);
        let count = |f: &dyn Fn(&UnityProject) -> bool| list.iter().filter(|p| f(p)).count();
        eprintln!(
            "alcom={} listed={} ok={} missing={} timeout={} avatar={} world={} unknown={} editorAvailable={} running={:?} editors={} took={:?}",
            alcom.len(),
            list.len(),
            count(&|p| p.state == "ok"),
            count(&|p| p.state == "missing"),
            count(&|p| p.state == "timeout"),
            count(&|p| p.kind == "avatar"),
            count(&|p| p.kind == "world"),
            count(&|p| p.kind == "unknown"),
            count(&|p| p.editor_available),
            list.iter()
                .map(|p| p.running)
                .collect::<std::collections::BTreeSet<_>>(),
            editors.len(),
            took,
        );
        assert_eq!(list.len(), alcom.len());
    }

    #[test]
    fn reading_does_not_write_into_the_project() {
        let root = tempfile::tempdir().unwrap();
        let project = make_project(
            root.path(),
            "Plum",
            "2022.3.22f1",
            Some(r#"{"locked":{"com.vrchat.avatars":{"version":"3.10.6"}}}"#),
        );
        // 項目の並びと、ファイルの大きさ・中身・更新日時。フォルダの更新日時は Windows が遅れて
        // 書き換えることがあるので比べない（新しい項目ができれば並びで分かる）。
        let snapshot = |dir: &Path| {
            let mut entries: Vec<(PathBuf, Option<(Vec<u8>, std::time::SystemTime)>)> =
                walkdir::WalkDir::new(dir)
                    .into_iter()
                    .flatten()
                    .map(|entry| {
                        let file = entry.file_type().is_file().then(|| {
                            (
                                std::fs::read(entry.path()).unwrap(),
                                entry.metadata().unwrap().modified().unwrap(),
                            )
                        });
                        (entry.path().to_path_buf(), file)
                    })
                    .collect();
            entries.sort();
            entries
        };
        let before = snapshot(&project);
        let _ = read_project(&project);
        let _ = read_projects(&[project.display().to_string()], READ_TIMEOUT, LIST_TIMEOUT);
        assert_eq!(before, snapshot(&project));
    }
}
