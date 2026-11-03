//! Correlate a late extension notification with a file already recovered at app startup.
use crate::model::DownloadFinished;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct StagedReceipt {
    booth_id: String,
    path: PathBuf,
    moved_at: i64,
}
fn key(source: &Path) -> String {
    let source = crate::library_files::without_verbatim_prefix(source)
        .display()
        .to_string();
    format!(
        "extension_staged:{}",
        if cfg!(windows) {
            source.to_lowercase()
        } else {
            source
        }
    )
}
pub(crate) fn remember(
    connection: &Connection,
    source: &Path,
    booth_id: &str,
    placed: &Path,
) -> Result<(), String> {
    let receipt = StagedReceipt {
        booth_id: booth_id.into(),
        path: placed.into(),
        moved_at: chrono::Utc::now().timestamp_millis(),
    };
    let value =
        serde_json::to_string(&receipt).map_err(|_| "移動先を記録できません".to_string())?;
    connection
        .execute(
            "INSERT OR REPLACE INTO settings(key,value) VALUES (?1,?2)",
            params![key(source), value],
        )
        .map_err(|_| "移動先を記録できません".to_string())?;
    connection.execute("DELETE FROM settings WHERE key IN (SELECT key FROM settings WHERE key GLOB 'extension_staged:*' ORDER BY rowid DESC LIMIT -1 OFFSET 2000)",[])
        .map_err(|_| "移動先を記録できません".to_string())?;
    Ok(())
}
pub(crate) fn recovered(
    connection: &Connection,
    notice: &DownloadFinished,
    started_at: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    // Existing sources must take the normal path, including symlink/path validation.
    match std::fs::symlink_metadata(&notice.path) {
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Ok(None),
    }
    let Some(started) = started_at
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp_millis())
    else {
        return Ok(None);
    };
    let value: Option<String> = connection
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [key(Path::new(&notice.path))],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "移動先を読めません".to_string())?;
    let Some(receipt) = value.and_then(|v| serde_json::from_str::<StagedReceipt>(&v).ok()) else {
        return Ok(None);
    };
    if started <= 0 || receipt.moved_at < started || receipt.booth_id != notice.booth_id {
        return Ok(None);
    }
    let saved = crate::db::settings(connection)?;
    if !saved.route_downloads || saved.library_folder.is_empty() {
        return Ok(None);
    }
    let excluded: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM product_exclusions WHERE booth_id=?1)",
            [&notice.booth_id],
            |r| r.get(0),
        )
        .map_err(|_| "商品を確認できません".to_string())?;
    if excluded {
        return Ok(None);
    }
    let Ok(metadata) = std::fs::symlink_metadata(&receipt.path) else {
        return Ok(None);
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || notice.size.is_some_and(|size| size != metadata.len())
    {
        return Ok(None);
    }
    let Ok(root) = std::fs::canonicalize(Path::new(&saved.library_folder).join(&notice.booth_id))
    else {
        return Ok(None);
    };
    let Ok(path) = std::fs::canonicalize(&receipt.path) else {
        return Ok(None);
    };
    if path.parent() != Some(root.as_path()) {
        return Ok(None);
    }
    let linked:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM local_files WHERE path=?1 AND linked_product_id=?2 AND is_dir=0)",params![receipt.path.display().to_string(),notice.booth_id],|r|r.get(0)).map_err(|_| "保存記録を読めません".to_string())?;
    Ok(if linked { Some(receipt.path) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_move_is_reconciled_only_with_current_exact_product_and_file_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let c = crate::db::open(&dir.path().join("test.sqlite")).unwrap();
        let library = dir.path().join("library");
        let destination = library.join("123456/sample.zip");
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&destination, b"test").unwrap();
        c.execute(
            "UPDATE settings SET value=?1 WHERE key='library_folder'",
            [library.display().to_string()],
        )
        .unwrap();
        c.execute_batch("UPDATE settings SET value='1' WHERE key='route_downloads';
            INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES ('123456','見本','','');
            INSERT INTO product_files(booth_id,variation,filename,downloadable_id) VALUES ('123456','A用','sample.zip','555555');").unwrap();
        crate::db::record_placed_download(&c, "123456", "", &destination, None).unwrap();
        let source = dir.path().join("Downloads/VRCLibMan/123456/sample.zip");
        let started = chrono::Utc::now().to_rfc3339();
        remember(&c, &source, "123456", &destination).unwrap();
        let mut notice = DownloadFinished {
            booth_id: "123456".into(),
            path: source.display().to_string(),
            filename: "sample.zip".into(),
            size: Some(4),
            is_free: None,
            is_adult: None,
            variation: "A用".into(),
            title: String::new(),
        };
        assert_eq!(
            recovered(&c, &notice, Some(&started)).unwrap(),
            Some(destination.clone())
        );
        crate::db::record_extension_download(
            &c,
            "123456",
            "A用",
            &destination,
            None,
            Some("555555"),
        )
        .unwrap();
        let count: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE booth_id='123456'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let future = (chrono::Utc::now() + chrono::Duration::days(1)).to_rfc3339();
        assert!(recovered(&c, &notice, Some(&future)).unwrap().is_none());
        assert!(recovered(&c, &notice, None).unwrap().is_none());
        notice.size = Some(5);
        assert!(recovered(&c, &notice, Some(&started)).unwrap().is_none());
        notice.size = Some(4);
        notice.booth_id = "234567".into();
        assert!(recovered(&c, &notice, Some(&started)).unwrap().is_none());
        notice.booth_id = "123456".into();
        c.execute_batch("UPDATE settings SET value='0' WHERE key='route_downloads'")
            .unwrap();
        assert!(recovered(&c, &notice, Some(&started)).unwrap().is_none());
    }
}
