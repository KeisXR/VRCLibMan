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

/// 索引にするファイル。登録済みで除外していない商品の、ライブラリかリモートの中の zip と unitypackage。
pub(crate) fn sources(connection: &rusqlite::Connection) -> Result<Vec<Source>, String> {
    let settings = crate::db::settings(connection)?;
    let roots = [settings.library_folder, settings.remote_library_folder];
    let mut statement = connection
        .prepare(
            "SELECT booth_id, local_path FROM product_files
             WHERE local_path IS NOT NULL AND local_path != ''
             AND booth_id IN (SELECT booth_id FROM products)
             AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
             UNION
             SELECT linked_product_id, path FROM local_files
             WHERE is_dir = 0 AND linked_product_id IN (SELECT booth_id FROM products)
             AND linked_product_id NOT IN (SELECT booth_id FROM product_exclusions)
             ORDER BY 1, 2",
        )
        .map_err(|err| format!("索引の対象を読めません: {err}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| format!("索引の対象を読めません: {err}"))?;
    let mut rows_found = Vec::new();
    for row in rows {
        rows_found.push(row.map_err(|err| format!("索引の対象を読めません: {err}"))?);
    }
    Ok(pick_sources(rows_found, &roots[0], &roots[1]))
}

/// ライブラリかリモートの中の zip と unitypackage だけを残す。手元とリモートに同じ商品の同じ名前の
/// ファイルがあれば、手元のものだけを読む（リモート自動同期で写したものを 2 回読まないため）。
pub(crate) fn pick_sources(
    rows: Vec<(String, String)>,
    library: &str,
    remote: &str,
) -> Vec<Source> {
    let file_key = |booth_id: &str, path: &str| {
        let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
        (booth_id.to_string(), path_key(name))
    };
    let local: std::collections::HashSet<(String, String)> = rows
        .iter()
        .filter(|(_, path)| inside(path, library))
        .map(|(booth_id, path)| file_key(booth_id, path))
        .collect();
    let mut found: Vec<Source> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (booth_id, path) in rows {
        if !is_package_source(&path) {
            continue;
        }
        let in_library = inside(&path, library);
        let in_remote = !in_library && inside(&path, remote);
        if !in_library && !in_remote {
            continue;
        }
        if in_remote && local.contains(&file_key(&booth_id, &path)) {
            continue;
        }
        if seen.insert(path_key(&path)) {
            found.push(Source {
                booth_id,
                path: PathBuf::from(path),
            });
        }
    }
    found
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
    let sources = {
        let state = app.state::<AppState>();
        let library = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        sources(&library)?
    };
    // 進捗は、読むファイルがあるときだけ出す（最初の 1 件を読む前に作る）。
    // 届かないファイル（切れた共有）は `store::refresh` が読まず、索引からも消さない。
    let progress: std::cell::RefCell<Option<ProgressGuard>> = std::cell::RefCell::new(None);
    let counted = store::refresh_counted(
        connection,
        &sources,
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
        let sep = std::path::MAIN_SEPARATOR;
        let lib = format!("{sep}lib");
        let remote = format!("{sep}remote");
        let at =
            |root: &str, rest: &str| format!("{root}{sep}{}", rest.replace('/', &sep.to_string()));
        let picked = pick_sources(
            vec![
                ("100".into(), at(&lib, "100/Outfit.zip")),
                ("100".into(), at(&remote, "100/Outfit.zip")),
                ("100".into(), at(&remote, "100/Extra.unitypackage")),
                ("200".into(), at(&remote, "200/Outfit.zip")),
                ("300".into(), at(&lib, "300/readme.txt")),
                ("400".into(), format!("{sep}elsewhere{sep}a.zip")),
            ],
            &lib,
            &remote,
        );
        let names: Vec<String> = picked
            .iter()
            .map(|s| s.path.display().to_string())
            .collect();
        // 手元にある同じ名前のファイルはリモートから読まない。別の商品の同じ名前は別のファイル。
        assert_eq!(
            names,
            [
                at(&lib, "100/Outfit.zip"),
                at(&remote, "100/Extra.unitypackage"),
                at(&remote, "200/Outfit.zip")
            ]
        );
        assert!(
            is_package_source("x.UnityPackage")
                && is_package_source("x.ZIP")
                && !is_package_source("x.png")
        );
    }
}
