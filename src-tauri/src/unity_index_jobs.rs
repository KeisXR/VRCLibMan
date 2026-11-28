//! 導入済みの判定の索引を作るワーカーと、コマンドから使う入口（docs/unity-projects.md の段階 2）。
//!
//! - 索引を作るのは専用のワーカー 1 本。起動の少しあと、ダウンロードの完了、フォルダスキャンの完了で起き、
//!   まだ読んでいない・変わったファイルだけを読む。分類・全取得・ダウンロードの最中は待つ。
//! - `library.sqlite` のロックは、対象のファイルを並べるあいだだけ持つ。読むのはロックの外。
//! - 進捗（`job-progress` の `unity-index`）は、読むファイルがあるときだけ送る。

use crate::AppState;
use crate::progress::ProgressGuard;
use crate::unity_index_store::{self as store, Source};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub(crate) const JOB: &str = "unity-index";
/// 索引が変わった・プロジェクトを調べたあとに送る。画面は読み直す。
pub(crate) const CHANGED_EVENT: &str = "unity-index-changed";
/// 起動直後はほかの仕事を先に通す。
const STARTUP_DELAY: Duration = Duration::from_secs(20);
/// ほかの仕事の最中に起きたら、この間をおいて試し直す。
const BUSY_RETRY: Duration = Duration::from_secs(60);

struct Worker {
    wake: mpsc::SyncSender<()>,
}

pub(crate) fn index_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "アプリのデータフォルダが見つかりません".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|_| "アプリのデータフォルダを作れません".to_string())?;
    Ok(dir.join(store::FILE_NAME))
}

/// Windows は大文字小文字を区別しない。区切りも `\` にそろえる。
fn path_key(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\").to_lowercase()
    } else {
        path.to_string()
    }
}

/// 索引のファイル（`source_path`）が、商品のどのファイル行か。記録した場所が同じ行、無ければ
/// 同じファイル名の行（リモートの `<商品ID>/<名前>` で読んだとき）。同じ名前の行が 2 つ以上
/// （別のバリエーションに同じ名前）なら決めない。
pub(crate) fn file_order(
    rows: &[crate::db::PackageFileRow],
    booth_id: &str,
    source_path: &str,
) -> Option<crate::unity_index::FileOrder> {
    let key = path_key(source_path);
    let mine: Vec<&crate::db::PackageFileRow> =
        rows.iter().filter(|row| row.booth_id == booth_id).collect();
    let by_path = mine.iter().find(|row| {
        row.local_path
            .as_deref()
            .is_some_and(|path| !path.is_empty() && path_key(path) == key)
    });
    let row = match by_path {
        Some(row) => *row,
        None => {
            let name = source_path
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(source_path);
            let mut named = mine.iter().filter(|row| row.filename == name);
            let row = named.next()?;
            if named.next().is_some() {
                return None;
            }
            *row
        }
    };
    Some(crate::unity_index::FileOrder {
        variation: row.variation.clone(),
        downloadable: row
            .downloadable_id
            .as_deref()
            .and_then(|id| id.trim().parse::<u64>().ok()),
        filename: row.filename.clone(),
    })
}

/// `path` が `root` の中にあるか（文字の上だけで比べる。届かない共有に触らない）。
pub(crate) fn inside(path: &str, root: &str) -> bool {
    let root = path_key(root.trim().trim_end_matches(['\\', '/']));
    if root.is_empty() {
        return false;
    }
    let path = path_key(path);
    let separator = if cfg!(windows) { '\\' } else { '/' };
    path.len() > root.len() && path.starts_with(&root) && path[root.len()..].starts_with(separator)
}

fn is_package_source(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".unitypackage") || lower.ends_with(".zip")
}

/// 索引にしてよいファイルの候補。データベースのロックの中で集め、ファイルがあるかはロックの外で確かめる。
pub(crate) struct Candidates {
    library: String,
    remote: String,
    /// 記録された場所（`product_files.local_path` と `local_files`）。（商品 ID、パス）
    recorded: Vec<(String, String)>,
    /// 商品のファイル名（`product_files.filename`）。（商品 ID、ファイル名）
    names: Vec<(String, String)>,
}

/// ファイルが今あるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    Here,
    Gone,
    /// 届かない（切れた共有）。読まないが、索引からも消さない。
    Unknown,
}

/// 選んだファイル。`read` は読んでよいもの、`keep` は届かないので読まないが索引に残すもの。
#[derive(Debug, Default)]
pub(crate) struct Picked {
    pub read: Vec<Source>,
    pub keep: Vec<Source>,
}

fn query_pairs(
    connection: &rusqlite::Connection,
    query: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut statement = connection
        .prepare(query)
        .map_err(|err| format!("索引の対象を読めません: {err}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| format!("索引の対象を読めません: {err}"))?;
    let mut found = Vec::new();
    for row in rows {
        found.push(row.map_err(|err| format!("索引の対象を読めません: {err}"))?);
    }
    Ok(found)
}

/// 索引にするファイルの候補。登録済みで除外していない商品の、ライブラリかリモートの中の zip と unitypackage。
pub(crate) fn candidates(connection: &rusqlite::Connection) -> Result<Candidates, String> {
    let settings = crate::db::settings(connection)?;
    let recorded = query_pairs(
        connection,
        "SELECT booth_id, local_path FROM product_files
         WHERE local_path IS NOT NULL AND local_path != ''
         AND booth_id IN (SELECT booth_id FROM products)
         AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
         UNION
         SELECT linked_product_id, path FROM local_files
         WHERE is_dir = 0 AND linked_product_id IN (SELECT booth_id FROM products)
         AND linked_product_id NOT IN (SELECT booth_id FROM product_exclusions)
         ORDER BY 1, 2",
    )?;
    let names = query_pairs(
        connection,
        "SELECT DISTINCT booth_id, filename FROM product_files
         WHERE filename != ''
         AND booth_id IN (SELECT booth_id FROM products)
         AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
         ORDER BY 1, 2",
    )?;
    Ok(Candidates {
        library: settings.library_folder,
        remote: settings.remote_library_folder,
        recorded,
        names,
    })
}

/// ファイルがあるかを確かめる。リモートのフォルダに届かないときは、その中のファイルを確かめない
/// （切れた共有で 1 件ずつ待たないため）。
fn present(path: &Path, remote_root: &str, remote_reachable: bool) -> Presence {
    use crate::library_files::{PROBE_TIMEOUT, Probe};
    if inside(&path.to_string_lossy(), remote_root) && !remote_reachable {
        return Presence::Unknown;
    }
    match crate::library_files::probe_file(path, PROBE_TIMEOUT) {
        Probe::Present => Presence::Here,
        Probe::Absent => Presence::Gone,
        Probe::Unreachable => Presence::Unknown,
    }
}

/// 商品とファイル名ごとの候補（ライブラリの中、リモートの中）。並びは最初に見た順。
#[derive(Default)]
struct Groups {
    index: std::collections::HashMap<(String, String), usize>,
    list: Vec<(String, Vec<PathBuf>, Vec<PathBuf>)>,
}

impl Groups {
    fn add(&mut self, booth_id: &str, name: &str, in_library: bool, path: PathBuf) {
        let key = (booth_id.to_string(), path_key(name));
        let at = *self.index.entry(key).or_insert_with(|| {
            self.list
                .push((booth_id.to_string(), Vec::new(), Vec::new()));
            self.list.len() - 1
        });
        let (_, local, far) = &mut self.list[at];
        let list = if in_library { local } else { far };
        let text = path_key(&path.to_string_lossy());
        if !list
            .iter()
            .any(|known| path_key(&known.to_string_lossy()) == text)
        {
            list.push(path);
        }
    }
}

/// 候補から、読むファイルを選ぶ。商品とファイル名ごとに、
/// 1. ライブラリの中に記録した場所にあれば、それを読む（リモート自動同期で写したものを 2 回読まないため）。
/// 2. 無ければ、リモートに記録した場所、リモートの `<商品ID>/<ファイル名>`、`<ファイル名>` の順に探し、
///    最初に見つかったものを読む。画面のファイル欄（`library_files::fill_places`）と同じ探し方で、
///    記録がダウンロードフォルダのまま・別の端末で受け取ったファイルも見つける。
/// 3. 届かない場所（切れた共有）は読まず、索引に残す。
pub(crate) fn pick_sources(candidates: &Candidates, present: impl Fn(&Path) -> Presence) -> Picked {
    let library = candidates.library.as_str();
    let remote = candidates.remote.as_str();
    let file_name = |path: &str| path.rsplit(['\\', '/']).next().unwrap_or(path).to_string();
    // （商品 ID、ファイル名）ごとに、ライブラリの候補とリモートの候補を並べる。
    let mut groups = Groups::default();
    for (booth_id, path) in &candidates.recorded {
        if !is_package_source(path) {
            continue;
        }
        let in_library = inside(path, library);
        let in_remote = !in_library && inside(path, remote);
        if in_library || in_remote {
            groups.add(booth_id, &file_name(path), in_library, PathBuf::from(path));
        }
    }
    let remote_root = remote.trim();
    if !remote_root.is_empty() {
        let root = Path::new(remote_root);
        for (booth_id, name) in &candidates.names {
            if !is_package_source(name)
                || !crate::library_files::valid_booth_id(booth_id)
                || crate::library_files::safe_filename(name).as_deref() != Some(name.as_str())
            {
                continue;
            }
            groups.add(booth_id, name, false, root.join(booth_id).join(name));
            groups.add(booth_id, name, false, root.join(name));
        }
    }

    let mut picked = Picked::default();
    let mut seen = std::collections::HashSet::new();
    let mut push = |into: &mut Vec<Source>, booth_id: &str, path: &Path| {
        if seen.insert(path_key(&path.to_string_lossy())) {
            into.push(Source {
                booth_id: booth_id.to_string(),
                path: path.to_path_buf(),
            });
        }
    };
    for (booth_id, local, far) in &groups.list {
        let booth_id = booth_id.as_str();
        let checked: Vec<(&PathBuf, Presence)> =
            local.iter().map(|path| (path, present(path))).collect();
        let here: Vec<&PathBuf> = checked
            .iter()
            .filter(|(_, presence)| *presence == Presence::Here)
            .map(|(path, _)| *path)
            .collect();
        if !here.is_empty() {
            for path in here {
                push(&mut picked.read, booth_id, path);
            }
            continue;
        }
        let mut unknown: Vec<&PathBuf> = checked
            .iter()
            .filter(|(_, presence)| *presence == Presence::Unknown)
            .map(|(path, _)| *path)
            .collect();
        let mut found = None;
        for path in far.iter() {
            match present(path) {
                Presence::Here => {
                    found = Some(path);
                    break;
                }
                Presence::Gone => {}
                Presence::Unknown => unknown.push(path),
            }
        }
        if let Some(path) = found {
            push(&mut picked.read, booth_id, path);
        }
        for path in unknown {
            push(&mut picked.keep, booth_id, path);
        }
    }
    picked
}

fn busy(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    state.extract_running.load(Ordering::SeqCst)
        || crate::browser::library_walk_running()
        || crate::browser::download_job_armed()
}

/// 索引を新しくする。`PASS` のロックを取ってから呼ぶ。読んだ数を返す。
pub(crate) fn refresh_index(
    app: &AppHandle,
    connection: &mut rusqlite::Connection,
) -> Result<usize, String> {
    let candidates = {
        let state = app.state::<AppState>();
        let library = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        candidates(&library)?
    };
    // ファイルがあるかはロックの外で確かめる。リモートのフォルダは 1 回だけ確かめる。
    let remote_root = candidates.remote.trim().to_string();
    let remote_reachable = !remote_root.is_empty()
        && crate::library_files::probe_path(
            Path::new(&remote_root),
            crate::library_files::PROBE_TIMEOUT,
        ) == crate::library_files::Probe::Present;
    let picked = pick_sources(&candidates, |path| {
        present(path, &remote_root, remote_reachable)
    });
    // 進捗は、読むファイルがあるときだけ出す（最初の 1 件を読む前に作る）。
    // 届かないファイル（切れた共有）は読まず、索引からも消さない（`picked.keep`）。
    let progress: std::cell::RefCell<Option<ProgressGuard>> = std::cell::RefCell::new(None);
    let counted = store::refresh_counted(
        connection,
        &picked.read,
        &picked.keep,
        |done, total| {
            let mut slot = progress.borrow_mut();
            let guard = slot.get_or_insert_with(|| ProgressGuard::new(app.clone(), JOB));
            guard.update(
                done as u32,
                total as u32,
                crate::progress::counted_message(
                    "Unity パッケージを調べています",
                    done as u32,
                    total as u32,
                    "",
                ),
            );
        },
        || busy(app),
    );
    if let Some(progress) = progress.borrow().as_ref() {
        match &counted {
            Ok((count, _)) => progress.done_with(
                *count as u32,
                format!("Unity パッケージ {count} 件を調べました"),
            ),
            Err(error) => progress.fail(error.clone()),
        }
    }
    let (read, removed) = counted?;
    // 消しただけでも、前に調べた結果は古くなっている（開いている画面に読み直させる）。
    if read > 0 || removed > 0 {
        let _ = app.emit(CHANGED_EVENT, ());
    }
    Ok(read)
}

fn run_pass(app: &AppHandle) -> Result<bool, String> {
    if busy(app) {
        return Ok(false);
    }
    let _pass = store::PASS
        .lock()
        .map_err(|_| "索引を更新できません".to_string())?;
    let mut connection = store::open(&index_path(app)?)?;
    refresh_index(app, &mut connection)?;
    Ok(!busy(app))
}

/// ワーカーを始める。起動の少しあとに 1 回、以降は `wake` で起きる。
pub(crate) fn start(app: &AppHandle) {
    let (sender, receiver) = mpsc::sync_channel(1);
    app.manage(Worker { wake: sender });
    let app = app.clone();
    std::thread::spawn(move || {
        let mut wait = STARTUP_DELAY;
        loop {
            match receiver.recv_timeout(wait) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            wait = match run_pass(&app) {
                // ほかの仕事に譲ったので、少しおいて続きを読む。
                Ok(false) => BUSY_RETRY,
                // 次は起こされるまで待つ。
                Ok(true) | Err(_) => Duration::from_secs(24 * 60 * 60),
            };
        }
    });
}

/// 索引を作り直すよう起こす（ダウンロードやスキャンのあと）。
pub(crate) fn wake(app: &AppHandle) {
    if let Some(worker) = app.try_state::<Worker>() {
        let _ = worker.wake.try_send(());
    }
}

/// プロジェクトを調べる。索引を先に新しくしてから、`Assets` を歩く。
pub(crate) fn scan_project(
    app: &AppHandle,
    project_id: &str,
    project_path: &Path,
    vpm: &[(String, String)],
) -> Result<store::ProjectContents, String> {
    let _pass = store::PASS
        .lock()
        .map_err(|_| "索引を更新できません".to_string())?;
    let mut connection = store::open(&index_path(app)?)?;
    refresh_index(app, &mut connection)?;
    let progress = ProgressGuard::new(app.clone(), "unity-project-scan");
    progress.update(0, 0, "プロジェクトを調べています");
    match store::scan_project(&mut connection, project_id, project_path, vpm) {
        Ok(()) => progress.done("プロジェクトを調べました"),
        Err(error) => {
            progress.fail(error.clone());
            return Err(error);
        }
    }
    let contents = store::project_contents(&connection, project_id)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(contents)
}

/// そのプロジェクトへパッケージを渡したので、前に調べた結果を古いとする。索引を書くほかの仕事と同じく
/// `PASS` を取ってから書く。調べている最中なら調べ終えてから書くので、調べ終わりの書き込みで「古い」が
/// 消えない。調べ終わるまで「Unityで開く」を待たせないよう、別のスレッドで行う。
pub(crate) fn mark_project_stale(app: &AppHandle, project_id: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        let _pass = store::PASS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let marked = index_path(&app)
            .and_then(|path| store::open(&path))
            .and_then(|connection| store::mark_project_stale(&connection, &project_id));
        match marked {
            Ok(()) => {
                let _ = app.emit(CHANGED_EVENT, ());
            }
            Err(error) => {
                eprintln!("[unity-index] 渡したプロジェクトの結果を古いとできません: {error}")
            }
        }
    });
}

/// 前に調べた結果を読む。索引が無ければ空。
pub(crate) fn read<T>(
    app: &AppHandle,
    work: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    let connection = store::open(&index_path(app)?)?;
    work(&connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_compares_text_only() {
        if cfg!(windows) {
            assert!(inside("C:\\Lib\\100\\a.zip", "c:/lib/"));
            assert!(!inside("C:\\Library\\a.zip", "C:\\Lib"));
            assert!(!inside("C:\\Lib", "C:\\Lib"));
        } else {
            assert!(inside("/lib/100/a.zip", "/lib/"));
            assert!(!inside("/library/a.zip", "/lib"));
        }
        assert!(!inside("/a/b.zip", ""));
        assert!(
            is_package_source("x.UnityPackage")
                && is_package_source("x.ZIP")
                && !is_package_source("x.png")
        );
    }

    fn at(root: &str, rest: &str) -> String {
        let sep = std::path::MAIN_SEPARATOR;
        format!("{root}{sep}{}", rest.replace('/', &sep.to_string()))
    }

    fn picked_paths(sources: &[Source]) -> Vec<String> {
        sources
            .iter()
            .map(|source| source.path.display().to_string())
            .collect()
    }

    #[test]
    fn index_files_find_their_file_rows() {
        let row = |variation: &str, filename: &str, id: Option<&str>, path: Option<&str>| {
            crate::db::PackageFileRow {
                booth_id: "1100".into(),
                variation: variation.into(),
                filename: filename.into(),
                downloadable_id: id.map(Into::into),
                local_path: path.map(Into::into),
            }
        };
        let sep = std::path::MAIN_SEPARATOR;
        let rows = [
            // 同じ名前を避けて改名して置いたファイル。
            row(
                "",
                "a.zip",
                Some("10"),
                Some(&format!("L{sep}1100{sep}a (1).zip")),
            ),
            row("", "b.zip", Some("20"), None),
            row("Airi", "same.zip", Some("30"), None),
            row("Plum", "same.zip", Some("31"), None),
            row("", "c.zip", Some("x"), None),
        ];
        let order = |path: &str| file_order(&rows, "1100", path);
        let by_path = order(&format!("L{sep}1100{sep}a (1).zip")).unwrap();
        assert_eq!(
            (by_path.filename.as_str(), by_path.downloadable),
            ("a.zip", Some(10))
        );
        let by_name = order(&format!("R{sep}1100{sep}b.zip")).unwrap();
        assert_eq!(by_name.downloadable, Some(20));
        // 2 つのバリエーションに同じ名前があれば決めない。
        assert!(order(&format!("R{sep}1100{sep}same.zip")).is_none());
        // 番号の読めないファイルは並びが分からない。
        assert_eq!(order(&format!("R{sep}c.zip")).unwrap().downloadable, None);
        // ほかの商品の行は見ない。
        assert!(file_order(&rows, "1200", &format!("R{sep}b.zip")).is_none());
    }

    #[test]
    fn sources_prefer_the_library_then_find_the_remote_copy() {
        let sep = std::path::MAIN_SEPARATOR;
        let lib = format!("{sep}lib");
        let remote = format!("{sep}remote");
        let candidates = Candidates {
            library: lib.clone(),
            remote: remote.clone(),
            recorded: vec![
                ("1100".into(), at(&lib, "1100/Outfit.zip")),
                ("1100".into(), at(&remote, "1100/Outfit.zip")),
                ("1100".into(), at(&remote, "1100/Extra.unitypackage")),
                ("1200".into(), at(&remote, "1200/Outfit.zip")),
                ("1300".into(), at(&lib, "1300/readme.txt")),
                ("1400".into(), format!("{sep}elsewhere{sep}a.zip")),
                // 記録が手元のまま、ファイルはもう無い。リモートに写してある。
                ("1500".into(), at(&lib, "1500/Hair.zip")),
            ],
            names: vec![
                ("1100".into(), "Outfit.zip".into()),
                ("1500".into(), "Hair.zip".into()),
                // 記録の場所はダウンロードフォルダのまま。リモートの商品フォルダにある。
                ("1600".into(), "Body.unitypackage".into()),
                // リモートの一番上に置かれたもの。
                ("1700".into(), "Gimmick.zip".into()),
                ("1800".into(), "Nowhere.zip".into()),
                ("1900".into(), "texture.png".into()),
                ("12000".into(), "..\\x.zip".into()),
            ],
        };
        let here = [
            at(&lib, "1100/Outfit.zip"),
            at(&remote, "1100/Outfit.zip"),
            at(&remote, "1100/Extra.unitypackage"),
            at(&remote, "1200/Outfit.zip"),
            at(&remote, "1500/Hair.zip"),
            at(&remote, "1600/Body.unitypackage"),
            at(&remote, "Gimmick.zip"),
        ];
        let picked = pick_sources(&candidates, |path| {
            if here.contains(&path.display().to_string()) {
                Presence::Here
            } else {
                Presence::Gone
            }
        });
        // 手元にある同じ名前のファイルはリモートから読まない。別の商品の同じ名前は別のファイル。
        assert_eq!(
            picked_paths(&picked.read),
            [
                at(&lib, "1100/Outfit.zip"),
                at(&remote, "1100/Extra.unitypackage"),
                at(&remote, "1200/Outfit.zip"),
                at(&remote, "1500/Hair.zip"),
                at(&remote, "1600/Body.unitypackage"),
                at(&remote, "Gimmick.zip"),
            ]
        );
        assert!(picked.keep.is_empty());
    }

    #[test]
    fn unreachable_remote_files_are_kept_but_not_read() {
        let sep = std::path::MAIN_SEPARATOR;
        let lib = format!("{sep}lib");
        let remote = format!("{sep}remote");
        let candidates = Candidates {
            library: lib.clone(),
            remote: remote.clone(),
            recorded: vec![("1100".into(), at(&lib, "1100/Outfit.zip"))],
            names: vec![
                ("1100".into(), "Outfit.zip".into()),
                ("1200".into(), "Hair.zip".into()),
            ],
        };
        let picked = pick_sources(&candidates, |path| {
            if path.display().to_string().starts_with(&remote) {
                Presence::Unknown
            } else {
                Presence::Gone
            }
        });
        assert!(picked.read.is_empty());
        assert_eq!(
            picked_paths(&picked.keep),
            [
                at(&remote, "1100/Outfit.zip"),
                at(&remote, "Outfit.zip"),
                at(&remote, "1200/Hair.zip"),
                at(&remote, "Hair.zip"),
            ]
        );
        // 届かないリモートのファイルは、1 件ずつ確かめない。
        assert_eq!(
            present(Path::new(&at(&remote, "1100/Outfit.zip")), &remote, false),
            Presence::Unknown
        );
    }

    /// 実際のライブラリで、索引に入るファイルを数える。`VRCLIBMAN_REAL_DB` に `library.sqlite` を渡す
    /// （読み取り専用で開き、一時ファイルへ写してから読む）。商品名やパスは出さない。
    #[test]
    #[ignore]
    fn real_library_sources() {
        let db = std::env::var("VRCLIBMAN_REAL_DB").expect("VRCLIBMAN_REAL_DB");
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("copy.sqlite");
        let source =
            rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        source
            .execute("VACUUM INTO ?1", [copy.to_string_lossy().to_string()])
            .unwrap();
        let connection = rusqlite::Connection::open(&copy).unwrap();
        let candidates = candidates(&connection).unwrap();
        let remote_root = candidates.remote.trim().to_string();
        let reachable = !remote_root.is_empty()
            && crate::library_files::probe_path(
                Path::new(&remote_root),
                crate::library_files::PROBE_TIMEOUT,
            ) == crate::library_files::Probe::Present;
        let picked = pick_sources(&candidates, |path| present(path, &remote_root, reachable));
        let in_remote = picked
            .read
            .iter()
            .filter(|source| inside(&source.path.to_string_lossy(), &remote_root))
            .count();
        let products: std::collections::HashSet<&str> = picked
            .read
            .iter()
            .map(|source| source.booth_id.as_str())
            .collect();
        println!(
            "remote reachable={reachable} read={} (remote {in_remote}, products {}) keep={}",
            picked.read.len(),
            products.len(),
            picked.keep.len()
        );
        // リモートにあって読まないもの。理由ごとに数える。
        let picked_keys: std::collections::HashSet<String> = picked
            .read
            .iter()
            .map(|source| path_key(&source.path.to_string_lossy()))
            .collect();
        let registered: std::collections::HashSet<String> = candidates
            .names
            .iter()
            .map(|(booth, name)| format!("{booth}/{}", path_key(name)))
            .collect();
        let products: std::collections::HashSet<String> = candidates
            .names
            .iter()
            .map(|(booth, _)| booth.clone())
            .collect();
        let mut reasons: std::collections::BTreeMap<&str, usize> = Default::default();
        for entry in walkdir::WalkDir::new(&remote_root)
            .max_depth(3)
            .into_iter()
            .flatten()
        {
            let path = entry.path().to_string_lossy().to_string();
            if !entry.file_type().is_file() || !is_package_source(&path) {
                continue;
            }
            if picked_keys.contains(&path_key(&path)) {
                *reasons.entry("read").or_default() += 1;
                continue;
            }
            let parent = entry
                .path()
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let name = path_key(&entry.file_name().to_string_lossy());
            let reason = if picked.read.iter().any(|s| {
                s.booth_id == parent
                    && path_key(&s.path.to_string_lossy())
                        .ends_with(&format!("{}{name}", std::path::MAIN_SEPARATOR))
            }) {
                "same name read locally"
            } else if !crate::library_files::valid_booth_id(&parent) {
                "not in a product folder"
            } else if !products.contains(&parent) {
                "product has no file rows / excluded / unregistered"
            } else if !registered.contains(&format!("{parent}/{name}")) {
                "file name not in product_files"
            } else {
                "other"
            };
            *reasons.entry(reason).or_default() += 1;
        }
        println!("remote files: {reasons:?}");
    }

    /// 版と改変の見込みを、この PC のライブラリと ALCOM の全プロジェクトで数える。数だけを出し、
    /// 商品名・ID・パスは出さない。`VRCLIBMAN_REAL_DB` を指定して
    /// `cargo test --lib -- --ignored real_version_notes --nocapture` で動かす。索引は一時フォルダ
    /// （`VRCLIBMAN_REAL_INDEX_DIR` があればそこ）に作る。
    #[test]
    #[ignore]
    fn real_version_notes() {
        use crate::unity_index::{Verdict, VersionNote};
        use std::collections::{HashMap, HashSet};
        let db = std::env::var("VRCLIBMAN_REAL_DB").expect("VRCLIBMAN_REAL_DB");
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path().join("copy.sqlite");
        rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .execute("VACUUM INTO ?1", [copy.to_string_lossy().to_string()])
            .unwrap();
        let library = rusqlite::Connection::open(&copy).unwrap();
        let candidates = candidates(&library).unwrap();
        let remote_root = candidates.remote.trim().to_string();
        let reachable = !remote_root.is_empty()
            && crate::library_files::probe_path(
                Path::new(&remote_root),
                crate::library_files::PROBE_TIMEOUT,
            ) == crate::library_files::Probe::Present;
        let picked = pick_sources(&candidates, |path| present(path, &remote_root, reachable));
        let index_dir = std::env::var("VRCLIBMAN_REAL_INDEX_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| dir.path().to_path_buf());
        let mut index = store::open(&index_dir.join(store::FILE_NAME)).unwrap();
        let started = std::time::Instant::now();
        let (read, _) =
            store::refresh_counted(&mut index, &picked.read, &picked.keep, |_, _| {}, || false)
                .unwrap();
        println!(
            "indexed {} files (read {read}) in {:?}",
            picked.read.len(),
            started.elapsed()
        );
        let settings = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap())
            .join("VRChatCreatorCompanion")
            .join("settings.json");
        let projects = crate::unity_projects::parse_user_projects(
            &std::fs::read_to_string(settings).unwrap_or_default(),
        );
        // 単体のファイルの zip ごとの（ファイル数、名前だけ当たった最大数、名前と大きさが当たった最大数）。
        let mut loose_diag: std::collections::BTreeMap<i64, (usize, usize, usize)> =
            Default::default();
        for (number, path) in projects.iter().enumerate() {
            let path = std::path::PathBuf::from(path);
            if !path.join("Assets").is_dir() {
                continue;
            }
            let manifest = std::fs::read_to_string(path.join("Packages").join("vpm-manifest.json"))
                .unwrap_or_default();
            let (_, packages) = crate::unity_projects::parse_vpm_manifest(&manifest);
            let vpm: Vec<(String, String)> =
                packages.into_iter().map(|p| (p.name, p.version)).collect();
            let id = format!("p{number}");
            let started = std::time::Instant::now();
            if store::scan_project(&mut index, &id, &path, &vpm).is_err() {
                println!("{id}: could not scan");
                continue;
            }
            let contents = store::project_contents(&index, &id).unwrap();
            let ids: Vec<String> = contents
                .products
                .iter()
                .map(|p| p.booth_id.clone())
                .collect();
            let rows = crate::db::package_file_rows(&library, &ids).unwrap();
            let (mut present, mut older, mut differs_one, mut differs_many, mut adjusted) =
                (0, 0, 0, 0, 0);
            let mut differ_files = Vec::new();
            let (mut loose_only, mut loose_partial) = (0, 0);
            let loose_packages = contents
                .products
                .iter()
                .flat_map(|p| p.packages.iter())
                .filter(|p| p.loose)
                .count();
            for product in &contents.products {
                if !matches!(product.verdict, Verdict::Installed | Verdict::Partial) {
                    continue;
                }
                present += 1;
                // 単体のファイルの zip だけで入っていると分かった商品と、その中で一部のもの。
                let by_package = product
                    .packages
                    .iter()
                    .filter(|p| matches!(p.verdict, Verdict::Installed | Verdict::Partial));
                if by_package.clone().all(|p| p.loose) {
                    loose_only += 1;
                    if product.verdict == Verdict::Partial {
                        loose_partial += 1;
                    }
                }
                let sizes: Vec<_> = product.packages.iter().map(|p| p.sizes.clone()).collect();
                let report = crate::unity_index::version_report(&sizes, |source| {
                    file_order(&rows, &product.booth_id, source)
                });
                match report.note {
                    Some(VersionNote::Older { .. }) => older += 1,
                    Some(VersionNote::Differs { files, versions }) => {
                        differ_files.push(files);
                        if versions >= 2 {
                            differs_many += 1;
                        } else {
                            differs_one += 1;
                        }
                    }
                    None => {}
                }
                adjusted += usize::from(report.adjusted > 0);
            }
            differ_files.sort_unstable();
            // どの種類のファイルが、手元のどの版とも大きさが違うか（`VRCLIBMAN_REAL_DIFF_EXT` があるとき）。
            if std::env::var("VRCLIBMAN_REAL_DIFF_EXT").is_ok() {
                let mut sizes: HashMap<Vec<u8>, (HashSet<i64>, String)> = HashMap::new();
                let mut statement = index
                    .prepare("SELECT guid, asset_size, ext FROM package_guids WHERE asset_size IS NOT NULL")
                    .unwrap();
                for (guid, size, ext) in statement
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, Vec<u8>>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    })
                    .unwrap()
                    .flatten()
                {
                    let entry = sizes.entry(guid).or_insert_with(|| (HashSet::new(), ext));
                    entry.0.insert(size);
                }
                let scan = crate::unity_index::scan_metas(
                    &path.join("Assets"),
                    std::time::Instant::now() + crate::unity_index::PROJECT_TIMEOUT,
                    crate::unity_index::MAX_PROJECT_FILES,
                );
                let mut by_ext: std::collections::BTreeMap<String, (usize, usize)> =
                    Default::default();
                for (guid, asset) in &scan.found {
                    let Some((known, ext)) = sizes.get(guid.as_slice()) else {
                        continue;
                    };
                    let Some(on_disk) = scan.sizes.get(asset) else {
                        continue;
                    };
                    let entry = by_ext.entry(ext.clone()).or_default();
                    entry.0 += 1;
                    if !known.contains(&(*on_disk as i64)) {
                        entry.1 += 1;
                    }
                }
                let differing: Vec<String> = by_ext
                    .iter()
                    .filter(|(_, (_, differ))| *differ > 0)
                    .map(|(ext, (all, differ))| format!("{ext}:{differ}/{all}"))
                    .collect();
                println!("{id}: differ by type {differing:?}");
            }
            if std::env::var("VRCLIBMAN_REAL_LOOSE_DIAG").is_ok() {
                let mut files: HashMap<i64, Vec<(String, i64)>> = HashMap::new();
                let mut statement = index
                    .prepare("SELECT package_id, name, size FROM package_files")
                    .unwrap();
                for (package, name, size) in statement
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    })
                    .unwrap()
                    .flatten()
                {
                    files.entry(package).or_default().push((name, size));
                }
                let scan = crate::unity_index::scan_metas(
                    &path.join("Assets"),
                    std::time::Instant::now() + crate::unity_index::PROJECT_TIMEOUT,
                    crate::unity_index::MAX_PROJECT_FILES,
                );
                let mut on_disk: HashMap<String, HashSet<i64>> = HashMap::new();
                for (file, size) in &scan.sizes {
                    on_disk
                        .entry(crate::unity_index::loose_key(&file.to_string_lossy()))
                        .or_default()
                        .insert(*size as i64);
                }
                for (package, list) in &files {
                    let name_hits = list.iter().filter(|(n, _)| on_disk.contains_key(n)).count();
                    let both = list
                        .iter()
                        .filter(|(n, s)| on_disk.get(n).is_some_and(|sizes| sizes.contains(s)))
                        .count();
                    // 名前は当たって大きさが当たらないとき、同じ商品が unitypackage で入っているか。
                    let booth: Option<String> = index
                        .query_row(
                            "SELECT booth_id FROM packages WHERE id = ?1",
                            [package],
                            |r| r.get(0),
                        )
                        .unwrap_or(None);
                    let same_product_installed = contents.products.iter().any(|p| {
                        Some(&p.booth_id) == booth.as_ref()
                            && p.packages
                                .iter()
                                .any(|k| !k.loose && k.verdict == Verdict::Installed)
                    });
                    if name_hits >= 5 && both == 0 {
                        println!(
                            "{id}: loose zip with {name_hits}/{} names but no sizes; same product installed by unitypackage={same_product_installed}",
                            list.len()
                        );
                    }
                    let entry = loose_diag.entry(*package).or_insert((list.len(), 0, 0));
                    entry.1 = entry.1.max(name_hits);
                    entry.2 = entry.2.max(both);
                }
            }
            println!(
                "{id}: complete={} scan={:?} present={present} older={older} differs(1 version)={differs_one} differs(2+)={differs_many} materials-changed={adjusted} differing files={differ_files:?} loose zips={loose_packages} found by name only={loose_only} (partial {loose_partial})",
                contents.complete,
                started.elapsed()
            );
        }
        for (files, names, both) in loose_diag.values() {
            println!("loose zip: files={files} best name hits={names} best name+size hits={both}");
        }
    }
}
