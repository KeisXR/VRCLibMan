use crate::{AppState, db, library_files};
use library_files::CopyOutcome;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const INTERVAL: Duration = Duration::from_secs(60);

/// One pass's result, split so a person can see what to fix. Each list keeps the first
/// `LISTED` files as "id/filename"; the counts are complete.
#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    running: bool,
    copied: usize,
    // Already on the remote: recorded and still there, or found with the same size.
    skipped: usize,
    // Recorded files that are no longer at their remote path. They are reported, never
    // copied again automatically.
    missing: usize,
    missing_files: Vec<String>,
    // A different file (another size) already has the name. Nothing is written or recorded,
    // so the next pass looks again after the person resolves it.
    conflicts: usize,
    conflict_files: Vec<SyncConflict>,
    // The registered local file is gone, so there is nothing to copy.
    source_missing: usize,
    source_missing_files: Vec<String>,
    // Copies that failed. Each keeps its own error; one failure does not hide another.
    failed: usize,
    failures: Vec<SyncFailure>,
    last_completed_at: Option<String>,
    // A problem with the whole pass, such as an unreachable remote folder.
    error: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConflict {
    file: String,
    local_size: u64,
    remote_size: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncFailure {
    file: String,
    error: String,
}

const LISTED: usize = 20;

fn listed<T>(list: &mut Vec<T>, item: T) {
    if list.len() < LISTED {
        list.push(item);
    }
}

struct SyncState {
    wake: mpsc::SyncSender<()>,
    status: Mutex<Status>,
}

pub(crate) fn validate_root(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(
            "自動同期をオンにする前に、リモートライブラリのフォルダを選んでください".into(),
        );
    }
    let root = Path::new(value);
    if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(
            "リモートライブラリには、親ディレクトリを含まない絶対パスを指定してください".into(),
        );
    }
    Ok(())
}

// v11. Files that automatic sync copied to, or found in, one remote folder. A sync client,
// another device, or the user may later rename, move, or delete the remote copy. Copying it
// again on every pass recreated the product folder each time, and Nextcloud kept renaming the
// new one to "7770415 2", "7770415 3", ... So a recorded file is only reported. Manual copies
// are recorded too. This is local state and stays out of the data export.
pub(crate) const PUSHES_TABLE: &str = "CREATE TABLE IF NOT EXISTS remote_pushes (
    remote_root TEXT NOT NULL,
    booth_id TEXT NOT NULL,
    filename TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (remote_root, booth_id, filename)
);";

fn is_recorded(
    connection: &rusqlite::Connection,
    root: &str,
    booth_id: &str,
    filename: &str,
) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM remote_pushes
             WHERE remote_root = ?1 AND booth_id = ?2 AND filename = ?3)",
            rusqlite::params![root.trim(), booth_id, filename],
            |row| row.get(0),
        )
        .map_err(|err| format!("同期の記録を読めません: {err}"))
}

pub(crate) fn remember(
    connection: &mut rusqlite::Connection,
    root: &str,
    booth_id: &str,
    filename: &str,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO remote_pushes (remote_root, booth_id, filename, recorded_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                root.trim(),
                booth_id,
                filename,
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .map(|_| ())
        .map_err(|err| format!("同期の記録を保存できません: {err}"))
}

// Exactly one worker owns all automatic copies. Slow shares never hold the DB
// lock or create overlapping retry workers.
pub(crate) fn start(app: &AppHandle) {
    let (sender, receiver) = mpsc::sync_channel(1);
    app.manage(SyncState {
        wake: sender,
        status: Mutex::new(Status::default()),
    });
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            if let Err(error) = run_pass(&app) {
                if let Ok(mut status) = app.state::<SyncState>().status.lock() {
                    status.running = false;
                    status.error = Some(error);
                }
            }
            if matches!(
                receiver.recv_timeout(INTERVAL),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ) {
                break;
            }
        }
    });
}

pub(crate) fn wake(app: &AppHandle) {
    if let Some(state) = app.try_state::<SyncState>() {
        let _ = state.wake.try_send(());
    }
}

pub(crate) fn status(app: &AppHandle) -> Result<Status, String> {
    app.state::<SyncState>()
        .status
        .lock()
        .map(|s| s.clone())
        .map_err(|_| "同期状態を読めません".into())
}

fn run_pass<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (settings, candidates) = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません")?;
        let settings = db::settings(&connection)?;
        if !settings.remote_auto_sync {
            return Ok(());
        }
        let candidates = candidates(&connection)?;
        (settings, candidates)
    };
    let sync = app.state::<SyncState>();
    let mut status = Status {
        running: true,
        ..Default::default()
    };
    *sync.status.lock().map_err(|_| "同期状態を更新できません")? = status.clone();
    let result = (|| {
        validate_root(&settings.remote_library_folder)?;
        let root = Path::new(&settings.remote_library_folder);
        // An unreachable share must not make every recorded file look removed.
        if !root.is_dir() {
            return Err("リモートのフォルダに届きません。接続後に再試行します".to_string());
        }
        for (booth_id, filename, source) in candidates {
            // Turning off sync or changing its destination stops the next copy.
            let recorded = {
                let connection = state
                    .db
                    .lock()
                    .map_err(|_| "データベースをロックできません")?;
                let current = db::settings(&connection)?;
                if !current.remote_auto_sync
                    || current.remote_library_folder != settings.remote_library_folder
                {
                    break;
                }
                if db::product_excluded(&connection, &booth_id)? {
                    continue;
                }
                is_recorded(
                    &connection,
                    &settings.remote_library_folder,
                    &booth_id,
                    &filename,
                )?
            };
            let file = format!("{booth_id}/{filename}");
            // A registered local file may have been removed since the last scan.
            if !source.is_file() {
                status.source_missing += 1;
                listed(&mut status.source_missing_files, file);
                *sync.status.lock().map_err(|_| "同期状態を更新できません")? = status.clone();
                continue;
            }
            if recorded {
                // Only look. Creating the product folder again is what made the sync
                // client add another numbered copy.
                match library_files::remote_copy_present(root, &booth_id, &filename) {
                    Some(true) => status.skipped += 1,
                    Some(false) => {
                        status.missing += 1;
                        listed(&mut status.missing_files, file);
                    }
                    None => {}
                }
            } else {
                match library_files::push_missing_file(&source, root, &booth_id, &filename) {
                    Ok(CopyOutcome::Conflict {
                        source_size,
                        existing_size,
                    }) => {
                        status.conflicts += 1;
                        listed(
                            &mut status.conflict_files,
                            SyncConflict {
                                file,
                                local_size: source_size,
                                remote_size: existing_size,
                            },
                        );
                    }
                    Ok(outcome) => {
                        if outcome == CopyOutcome::Copied {
                            status.copied += 1;
                        } else {
                            status.skipped += 1;
                        }
                        let mut connection = state
                            .db
                            .lock()
                            .map_err(|_| "データベースをロックできません")?;
                        remember(
                            &mut connection,
                            &settings.remote_library_folder,
                            &booth_id,
                            &filename,
                        )?;
                    }
                    Err(error) => {
                        // A bad source must not block unrelated files in this pass, and
                        // the next failure must not hide this one.
                        status.failed += 1;
                        listed(&mut status.failures, SyncFailure { file, error });
                    }
                }
            }
            *sync.status.lock().map_err(|_| "同期状態を更新できません")? = status.clone();
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = result {
        status.error = Some(error);
    }
    status.running = false;
    status.last_completed_at = Some(chrono::Utc::now().to_rfc3339());
    *sync.status.lock().map_err(|_| "同期状態を更新できません")? = status;
    Ok(())
}

// Both automatically organized downloads and manually linked/scanned local
// files participate. Excluded products never become automatic copy sources.
fn candidates(connection: &rusqlite::Connection) -> Result<Vec<(String, String, PathBuf)>, String> {
    let mut query = connection
        .prepare(
            "SELECT booth_id, filename, local_path FROM product_files
         WHERE local_path IS NOT NULL AND local_path != ''
         AND booth_id IN (SELECT booth_id FROM products)
         AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
         UNION
         SELECT linked_product_id, filename, path FROM local_files
         WHERE is_dir = 0 AND linked_product_id IN (SELECT booth_id FROM products)
         AND linked_product_id NOT IN (SELECT booth_id FROM product_exclusions)
         ORDER BY 1, 2, 3",
        )
        .map_err(|err| format!("同期対象を読めません: {err}"))?;
    let rows = query
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                PathBuf::from(row.get::<_, String>(2)?),
            ))
        })
        .map_err(|err| format!("同期対象を読めません: {err}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|err| format!("同期対象を読めません: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync_app(
        directory: &Path,
        connection: rusqlite::Connection,
    ) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(AppState {
            db: Mutex::new(connection),
            cancel_extract: std::sync::atomic::AtomicBool::new(false),
            extract_running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            thumbs_dir: directory.join("thumbs"),
            previews_dir: directory.join("previews"),
            hero_images_dir: directory.join("hero_images"),
            receiver_error: Mutex::new(None),
            import_guard: Mutex::new(crate::local_import::ImportGuard::default()),
            background_mode: std::sync::atomic::AtomicBool::new(false),
            staged_left: Mutex::new(Vec::new()),
        });
        let (sender, _receiver) = mpsc::sync_channel(1);
        app.manage(SyncState {
            wake: sender,
            status: Mutex::new(Status::default()),
        });
        app
    }

    #[test]
    fn settings_are_opt_in_persist_and_allow_disabling_offline() {
        let directory = tempfile::tempdir().unwrap();
        let connection = db::open(&directory.path().join("library.sqlite")).unwrap();
        assert!(!db::settings(&connection).unwrap().remote_auto_sync);
        let enable = crate::model::SettingsPatch {
            remote_auto_sync: Some(true),
            ..Default::default()
        };
        assert!(db::update_settings(&connection, &enable).is_err());
        let folder = std::env::temp_dir().join("vrclibman-disconnected-sync");
        let saved = db::update_settings(
            &connection,
            &crate::model::SettingsPatch {
                remote_library_folder: Some(folder.display().to_string()),
                ..enable
            },
        )
        .unwrap();
        assert!(saved.remote_auto_sync);
        drop(connection);
        let connection = db::open(&directory.path().join("library.sqlite")).unwrap();
        assert!(db::settings(&connection).unwrap().remote_auto_sync);
        assert!(
            db::update_settings(
                &connection,
                &crate::model::SettingsPatch {
                    avatar_hero: Some("small".into()),
                    ..Default::default()
                }
            )
            .unwrap()
            .remote_auto_sync
        );
        let saved = db::update_settings(
            &connection,
            &crate::model::SettingsPatch {
                remote_auto_sync: Some(false),
                remote_library_folder: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!saved.remote_auto_sync);
        assert!(validate_root("relative/path").is_err());
        assert!(validate_root(&folder.join("../escape").display().to_string()).is_err());
    }

    #[test]
    fn recorded_files_are_reported_but_not_copied_again_after_the_remote_copy_moves() {
        let directory = tempfile::tempdir().unwrap();
        let connection = db::open(&directory.path().join("library.sqlite")).unwrap();
        let source = directory.path().join("source.zip");
        let remote = directory.path().join("remote");
        std::fs::write(&source, b"synthetic package").unwrap();
        // Another device already put this one (same size) in the remote folder.
        std::fs::create_dir_all(remote.join("9002")).unwrap();
        std::fs::write(remote.join("9002/found.zip"), b"synthetic package").unwrap();
        connection.execute_batch("INSERT INTO products (booth_id,title,imported_at,updated_at) VALUES ('9001','A','t','t'), ('9002','B','t','t');").unwrap();
        for (booth_id, filename) in [("9001", "file.zip"), ("9002", "found.zip")] {
            connection
                .execute(
                    "INSERT INTO product_files (booth_id,filename,local_path) VALUES (?1,?2,?3)",
                    rusqlite::params![booth_id, filename, source.display().to_string()],
                )
                .unwrap();
        }
        db::update_settings(
            &connection,
            &crate::model::SettingsPatch {
                remote_library_folder: Some(remote.display().to_string()),
                remote_auto_sync: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let app = sync_app(directory.path(), connection);
        let status = || app.state::<SyncState>().status.lock().unwrap().clone();

        run_pass(app.handle()).unwrap();
        let first = status();
        assert_eq!((first.copied, first.skipped, first.missing), (1, 1, 0));
        assert!(first.error.is_none());

        // Nextcloud renamed the product folder; the user deleted the other copy.
        std::fs::rename(remote.join("9001"), remote.join("9001 2")).unwrap();
        std::fs::remove_file(remote.join("9002/found.zip")).unwrap();
        for _ in 0..2 {
            run_pass(app.handle()).unwrap();
            let current = status();
            assert_eq!(
                (current.copied, current.skipped, current.missing),
                (0, 0, 2)
            );
            assert_eq!(current.missing_files, ["9001/file.zip", "9002/found.zip"]);
            assert!(current.error.is_none());
            assert!(!remote.join("9001").exists());
            assert!(!remote.join("9002/found.zip").exists());
        }
        assert_eq!(std::fs::read_dir(&remote).unwrap().count(), 2);

        // A manual copy puts it back, and later passes see it again.
        library_files::push_file(&source, &remote, "9001", "file.zip", false).unwrap();
        run_pass(app.handle()).unwrap();
        let current = status();
        assert_eq!(
            (current.copied, current.skipped, current.missing),
            (0, 1, 1)
        );

        // Records belong to one remote folder. A new folder gets its first copies.
        let other = directory.path().join("other");
        std::fs::create_dir(&other).unwrap();
        db::update_settings(
            &app.state::<AppState>().db.lock().unwrap(),
            &crate::model::SettingsPatch {
                remote_library_folder: Some(other.display().to_string()),
                remote_auto_sync: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        run_pass(app.handle()).unwrap();
        assert_eq!(status().copied, 2);
        assert!(other.join("9001/file.zip").is_file());
        assert!(other.join("9002/found.zip").is_file());
    }

    #[test]
    fn a_different_remote_file_is_a_conflict_that_is_reported_and_never_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let connection = db::open(&directory.path().join("library.sqlite")).unwrap();
        let source = directory.path().join("source.zip");
        let remote = directory.path().join("remote");
        std::fs::write(&source, b"synthetic package").unwrap();
        std::fs::create_dir_all(remote.join("9001")).unwrap();
        std::fs::write(remote.join("9001/file.zip"), b"older").unwrap();
        connection.execute_batch("INSERT INTO products (booth_id,title,imported_at,updated_at) VALUES ('9001','A','t','t'), ('9002','B','t','t');").unwrap();
        connection
            .execute(
                "INSERT INTO product_files (booth_id,filename,local_path) VALUES ('9001','file.zip',?1)",
                [source.display().to_string()],
            )
            .unwrap();
        // Registered, then removed from disk before the pass.
        let gone = directory.path().join("gone.zip");
        connection
            .execute(
                "INSERT INTO product_files (booth_id,filename,local_path) VALUES ('9002','gone.zip',?1)",
                [gone.display().to_string()],
            )
            .unwrap();
        db::update_settings(
            &connection,
            &crate::model::SettingsPatch {
                remote_library_folder: Some(remote.display().to_string()),
                remote_auto_sync: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let app = sync_app(directory.path(), connection);
        let status = || app.state::<SyncState>().status.lock().unwrap().clone();

        for _ in 0..2 {
            run_pass(app.handle()).unwrap();
            let current = status();
            assert_eq!((current.copied, current.skipped, current.conflicts), (0, 0, 1));
            assert_eq!(current.conflict_files[0].file, "9001/file.zip");
            assert_eq!(current.conflict_files[0].local_size, 17);
            assert_eq!(current.conflict_files[0].remote_size, 5);
            assert_eq!((current.source_missing, current.failed), (1, 0));
            assert_eq!(current.source_missing_files, ["9002/gone.zip"]);
            assert!(current.error.is_none());
            assert_eq!(std::fs::read(remote.join("9001/file.zip")).unwrap(), b"older");
            assert_eq!(std::fs::read_dir(remote.join("9001")).unwrap().count(), 1);
        }
        // A conflict is not recorded: once the person removes the other file, the next pass copies.
        std::fs::remove_file(remote.join("9001/file.zip")).unwrap();
        run_pass(app.handle()).unwrap();
        assert_eq!((status().copied, status().conflicts), (1, 0));
        assert_eq!(std::fs::read(remote.join("9001/file.zip")).unwrap(), b"synthetic package");
    }

    #[test]
    fn each_failed_copy_keeps_its_own_entry() {
        let directory = tempfile::tempdir().unwrap();
        let connection = db::open(&directory.path().join("library.sqlite")).unwrap();
        let remote = directory.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        // A file where product 9001's folder should be: both of its copies fail.
        std::fs::write(remote.join("9001"), b"not a folder").unwrap();
        let source = directory.path().join("source.zip");
        std::fs::write(&source, b"synthetic package").unwrap();
        connection.execute_batch("INSERT INTO products (booth_id,title,imported_at,updated_at) VALUES ('9001','A','t','t'), ('9002','B','t','t');").unwrap();
        for (booth_id, filename) in [("9001", "a.zip"), ("9001", "b.zip"), ("9002", "c.zip")] {
            connection
                .execute(
                    "INSERT INTO product_files (booth_id,filename,local_path) VALUES (?1,?2,?3)",
                    rusqlite::params![booth_id, filename, source.display().to_string()],
                )
                .unwrap();
        }
        db::update_settings(
            &connection,
            &crate::model::SettingsPatch {
                remote_library_folder: Some(remote.display().to_string()),
                remote_auto_sync: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        let app = sync_app(directory.path(), connection);
        run_pass(app.handle()).unwrap();
        let current = app.state::<SyncState>().status.lock().unwrap().clone();
        assert_eq!((current.copied, current.source_missing, current.failed), (1, 0, 2));
        let files: Vec<&str> = current.failures.iter().map(|failure| failure.file.as_str()).collect();
        assert_eq!(files, ["9001/a.zip", "9001/b.zip"]);
        assert!(current.failures.iter().all(|failure| !failure.error.is_empty()));
        assert!(current.error.is_none());
        assert!(remote.join("9002/c.zip").is_file());
    }

    #[test]
    fn candidates_include_registered_downloads_and_links_but_exclude_directories_and_exclusions() {
        let directory = tempfile::tempdir().unwrap();
        let connection = db::open(&directory.path().join("library.sqlite")).unwrap();
        connection.execute_batch("INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('9001','A','t','t'), ('9002','B','t','t');
            INSERT INTO product_files (booth_id, filename, local_path) VALUES ('9001','download.zip','/local/download.zip'), ('9002','excluded.zip','/local/excluded.zip');
            INSERT INTO local_files (path, filename, is_dir, linked_product_id, scanned_at) VALUES ('/local/linked.zip','linked.zip',0,'9001','t'), ('/local/folder','folder',1,'9001','t'), ('/local/unlinked.zip','unlinked.zip',0,NULL,'t');
            INSERT INTO product_exclusions (booth_id, excluded_at) VALUES ('9002','t');").unwrap();
        let rows = candidates(&connection).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(id, _, _)| id == "9001"));
        assert!(rows.iter().any(|(_, filename, _)| filename == "linked.zip"));
        assert!(
            rows.iter()
                .any(|(_, filename, _)| filename == "download.zip")
        );
    }

    #[test]
    fn background_pass_respects_opt_in_and_finds_new_files_after_offline_retry() {
        let directory = tempfile::tempdir().unwrap();
        let connection = db::open(&directory.path().join("library.sqlite")).unwrap();
        let source = directory.path().join("source.zip");
        let remote = directory.path().join("remote");
        std::fs::write(&source, b"synthetic package").unwrap();
        connection.execute_batch("INSERT INTO products (booth_id,title,imported_at,updated_at) VALUES ('9001','A','t','t');").unwrap();
        connection.execute("INSERT INTO product_files (booth_id,filename,local_path) VALUES ('9001','file.zip',?1)", [source.display().to_string()]).unwrap();
        db::update_settings(
            &connection,
            &crate::model::SettingsPatch {
                remote_library_folder: Some(remote.display().to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        let app = sync_app(directory.path(), connection);
        run_pass(app.handle()).unwrap();
        assert!(!remote.exists());
        assert!(
            app.state::<SyncState>()
                .status
                .lock()
                .unwrap()
                .last_completed_at
                .is_none()
        );
        let state = app.state::<AppState>();
        db::update_settings(
            &state.db.lock().unwrap(),
            &crate::model::SettingsPatch {
                remote_auto_sync: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        run_pass(app.handle()).unwrap();
        assert!(
            app.state::<SyncState>()
                .status
                .lock()
                .unwrap()
                .error
                .is_some()
        );
        std::fs::create_dir(&remote).unwrap();
        run_pass(app.handle()).unwrap();
        assert_eq!(
            std::fs::read(remote.join("9001/file.zip")).unwrap(),
            b"synthetic package"
        );
        assert!(
            app.state::<SyncState>()
                .status
                .lock()
                .unwrap()
                .error
                .is_none()
        );
        assert_eq!(app.state::<SyncState>().status.lock().unwrap().copied, 1);
        state.db.lock().unwrap().execute("INSERT INTO product_files (booth_id,filename,local_path) VALUES ('9001','new.zip',?1)", [source.display().to_string()]).unwrap();
        run_pass(app.handle()).unwrap();
        assert!(remote.join("9001/new.zip").exists());
        let current = app.state::<SyncState>();
        let status = current.status.lock().unwrap();
        assert_eq!(status.copied, 1);
        assert_eq!(status.skipped, 1);
        assert!(!status.running);
        drop(status);
        db::update_settings(
            &state.db.lock().unwrap(),
            &crate::model::SettingsPatch {
                remote_auto_sync: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::remove_file(remote.join("9001/new.zip")).unwrap();
        run_pass(app.handle()).unwrap();
        assert!(!remote.join("9001/new.zip").exists());
    }

    #[test]
    fn copies_once_preserves_both_sides_and_retries_after_reconnection() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.zip");
        let remote = directory.path().join("remote");
        std::fs::write(&source, b"complete file").unwrap();
        assert!(library_files::push_missing_file(&source, &remote, "9001", "file.zip").is_err());
        std::fs::create_dir(&remote).unwrap();
        assert_eq!(
            library_files::push_missing_file(&source, &remote, "9001", "file.zip").unwrap(),
            CopyOutcome::Copied
        );
        assert_eq!(
            std::fs::read(remote.join("9001/file.zip")).unwrap(),
            b"complete file"
        );
        assert_eq!(
            library_files::push_missing_file(&source, &remote, "9001", "file.zip").unwrap(),
            CopyOutcome::AlreadyThere
        );
        std::fs::write(&source, b"changed local file").unwrap();
        assert_eq!(
            library_files::push_missing_file(&source, &remote, "9001", "file.zip").unwrap(),
            CopyOutcome::Conflict {
                source_size: 18,
                existing_size: 13
            }
        );
        assert_eq!(
            std::fs::read(remote.join("9001/file.zip")).unwrap(),
            b"complete file"
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"changed local file");
        assert!(
            library_files::push_missing_file(&source, &remote, "../escape", "file.zip").is_err()
        );
        assert_eq!(std::fs::read_dir(remote.join("9001")).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_copies_never_overwrite_the_winner_or_leave_a_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let remote = directory.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = [b'A', b'B']
            .into_iter()
            .map(|byte| {
                let source = directory.path().join(format!("{byte}.zip"));
                std::fs::write(&source, vec![byte; 64 * 1024]).unwrap();
                let remote = remote.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    library_files::push_missing_file(&source, &remote, "9001", "file.zip").unwrap()
                })
            })
            .collect();
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .filter(|outcome| *outcome == CopyOutcome::Copied)
                .count(),
            1
        );
        let bytes = std::fs::read(remote.join("9001/file.zip")).unwrap();
        assert!(bytes == vec![b'A'; 64 * 1024] || bytes == vec![b'B'; 64 * 1024]);
        assert_eq!(std::fs::read_dir(remote.join("9001")).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_sources_and_escaping_destinations() {
        let directory = tempfile::tempdir().unwrap();
        let remote = directory.path().join("remote");
        let outside = directory.path().join("outside");
        std::fs::create_dir(&remote).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let source = directory.path().join("source.zip");
        std::fs::write(&source, b"safe").unwrap();
        std::os::unix::fs::symlink(&outside, remote.join("9001")).unwrap();
        assert!(library_files::push_missing_file(&source, &remote, "9001", "file.zip").is_err());
        assert!(!outside.join("file.zip").exists());
        let link = directory.path().join("link.zip");
        std::os::unix::fs::symlink(&source, &link).unwrap();
        assert!(library_files::push_missing_file(&link, &remote, "9002", "file.zip").is_err());
    }
}
