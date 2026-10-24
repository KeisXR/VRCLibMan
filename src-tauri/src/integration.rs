//! Local connection metadata. No product text, cookies or pairing tokens are stored here.
use crate::model::{ImportReport, now_iso};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReceipt {
    pub at: String,
    pub products: u32,
    pub inserted: u32,
    pub updated: u32,
    pub unchanged: u32,
    pub skipped: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    pub mode: String,
    pub paired: bool,
    pub expected_extension_version: String,
    pub extension_version: Option<String>,
    pub extension_last_seen_at: Option<String>,
    pub extension_import: Option<ImportReceipt>,
    pub browser_import: Option<ImportReceipt>,
}

fn read(connection: &Connection, key: &str) -> Result<Option<String>, String> {
    connection
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(|err| err.to_string())
}

fn write(connection: &Connection, key: &str, value: &str) -> Result<(), String> {
    connection.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])
        .map(|_| ()).map_err(|err| err.to_string())
}

pub fn status(connection: &Connection) -> Result<IntegrationStatus, String> {
    let builtin = read(connection, "builtin_browser")?.as_deref() == Some("1");
    let mode = if !builtin {
        "extension"
    } else if read(connection, "booth_import_mode")?.as_deref() == Some("browser") {
        "browser"
    } else {
        "both"
    };
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../extension/manifest.json"))
            .map_err(|err| err.to_string())?;
    let receipt = |key| -> Result<Option<ImportReceipt>, String> {
        Ok(read(connection, key)?.and_then(|text| serde_json::from_str(&text).ok()))
    };
    Ok(IntegrationStatus {
        mode: mode.into(),
        paired: read(connection, "extension_origin")?.is_some_and(|value| !value.is_empty()),
        expected_extension_version: manifest["version"].as_str().unwrap_or_default().into(),
        extension_version: read(connection, "extension_version")?.filter(|value| !value.is_empty()),
        extension_last_seen_at: read(connection, "extension_last_seen_at")?,
        extension_import: receipt("extension_import")?,
        browser_import: receipt("browser_import")?,
    })
}

pub fn set_mode(connection: &mut Connection, mode: &str) -> Result<(), String> {
    if !matches!(mode, "extension" | "browser" | "both") {
        return Err("BOOTH の読み込み方が不正です".into());
    }
    let tx = connection.transaction().map_err(|err| err.to_string())?;
    write(&tx, "booth_import_mode", mode)?;
    write(
        &tx,
        "builtin_browser",
        if mode == "extension" { "0" } else { "1" },
    )?;
    tx.commit().map_err(|err| err.to_string())
}

// Call only after the existing Origin/token gate accepts a request.
pub fn record_extension_seen(connection: &Connection, version: Option<&str>) -> Result<(), String> {
    let version = version.filter(|value| {
        !value.is_empty()
            && value.len() <= 32
            && value.split('.').count() == 3
            && value
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    });
    write(connection, "extension_version", version.unwrap_or_default())?;
    write(connection, "extension_last_seen_at", &now_iso())
}

pub fn record_import(
    connection: &Connection,
    source: &str,
    report: &ImportReport,
) -> Result<(), String> {
    let key = match source {
        "extension" => "extension_import",
        "browser" => "browser_import",
        _ => return Err("不明な読み込み元です".into()),
    };
    let receipt = ImportReceipt {
        at: now_iso(),
        products: report.inserted + report.updated + report.unchanged,
        inserted: report.inserted,
        updated: report.updated,
        unchanged: report.unchanged,
        skipped: report.skipped,
    };
    write(
        connection,
        key,
        &serde_json::to_string(&receipt).map_err(|err| err.to_string())?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_choice_and_failed_write_leave_settings_consistent() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .unwrap();
        assert_eq!(status(&conn).unwrap().mode, "extension");
        write(&conn, "builtin_browser", "1").unwrap();
        assert_eq!(status(&conn).unwrap().mode, "both");
        conn.execute_batch("CREATE TRIGGER refuse_browser_change BEFORE UPDATE ON settings WHEN NEW.key = 'builtin_browser' BEGIN SELECT RAISE(ABORT, 'fixture failure'); END;").unwrap();
        assert!(set_mode(&mut conn, "extension").is_err());
        assert_eq!(status(&conn).unwrap().mode, "both");
        assert!(read(&conn, "booth_import_mode").unwrap().is_none());
        assert_eq!(
            read(&conn, "builtin_browser").unwrap().as_deref(),
            Some("1")
        );
    }

    #[test]
    fn import_choice_is_persistent_and_preserves_existing_model_and_pairing() {
        let dir = std::env::temp_dir().join(format!("vrclibman-mode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.sqlite");
        let mut conn = crate::db::open(&path).unwrap();
        write(&conn, "ollama_model", "custom:4b").unwrap();
        write(&conn, "extension_origin", "existing-origin").unwrap();
        for mode in ["browser", "both", "extension"] {
            set_mode(&mut conn, mode).unwrap();
            assert_eq!(status(&conn).unwrap().mode, mode);
            assert_eq!(
                crate::db::settings(&conn).unwrap().builtin_browser,
                mode != "extension"
            );
        }
        assert!(set_mode(&mut conn, "unknown").is_err());
        set_mode(&mut conn, "browser").unwrap();
        drop(conn);
        let conn = crate::db::open(&path).unwrap();
        assert_eq!(status(&conn).unwrap().mode, "browser");
        assert_eq!(
            crate::db::settings(&conn).unwrap().ollama_model,
            "custom:4b"
        );
        assert!(status(&conn).unwrap().paired);
        drop(conn);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn receipts_are_separate_and_do_not_keep_product_text() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .unwrap();
        let report = ImportReport {
            inserted: 1,
            updated: 2,
            unchanged: 3,
            skipped: 4,
            variations: 0,
            downloads: 0,
            unnamed_downloads: 0,
            warnings: vec!["private product title".into()],
        };
        record_import(&conn, "extension", &report).unwrap();
        let state = status(&conn).unwrap();
        assert_eq!(state.extension_import.unwrap().products, 6);
        assert!(state.browser_import.is_none());
        assert!(
            !read(&conn, "extension_import")
                .unwrap()
                .unwrap()
                .contains("private")
        );
        record_extension_seen(&conn, Some("0.1.10")).unwrap();
        assert_eq!(
            status(&conn).unwrap().extension_version.as_deref(),
            Some("0.1.10")
        );
        record_extension_seen(&conn, Some("token/invalid")).unwrap();
        assert!(status(&conn).unwrap().extension_version.is_none());
        assert!(status(&conn).unwrap().extension_last_seen_at.is_some());
    }
}
