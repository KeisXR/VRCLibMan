use crate::{AppState, db, library_files};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, mpsc};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    running: bool,
    copied: usize,
    skipped: usize,
    last_completed_at: Option<String>,
    error: Option<String>,
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
        for (booth_id, filename, source) in candidates {
            // Turning off sync or changing its destination stops the next copy.
            {
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
            }
            // A registered local file may have been removed since the last scan.
            if !source.is_file() {
                continue;
            }
            match library_files::push_missing_file(
                &source,
                Path::new(&settings.remote_library_folder),
                &booth_id,
                &filename,
            ) {
                Ok(true) => status.copied += 1,
                Ok(false) => status.skipped += 1,
                Err(error) => {
                    status.error = Some(error);
                    break;
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
                    ollama_model: Some("qwen3.5:4b".into()),
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
        let app = tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        app.manage(AppState {
            db: Mutex::new(connection),
            cancel_extract: std::sync::atomic::AtomicBool::new(false),
            extract_running: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            thumbs_dir: directory.path().join("thumbs"),
            previews_dir: directory.path().join("previews"),
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
        assert!(library_files::push_missing_file(&source, &remote, "9001", "file.zip").unwrap());
        assert_eq!(
            std::fs::read(remote.join("9001/file.zip")).unwrap(),
            b"complete file"
        );
        std::fs::write(&source, b"changed local file").unwrap();
        assert!(!library_files::push_missing_file(&source, &remote, "9001", "file.zip").unwrap());
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
                .filter_map(|worker| worker.join().unwrap().then_some(()))
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
