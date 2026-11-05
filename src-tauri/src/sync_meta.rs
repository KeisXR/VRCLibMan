//! Who changed each syncable group of values, and when.
//!
//! A group is one row in `sync_meta`: a product's classification, a product's links, an avatar,
//! a file link, or one config key. `fingerprint` is the group's normalized value, so a change
//! is recorded only when the value differs. Startup and an identical re-resolve leave `at` alone.

use crate::db;
use crate::model::Settings;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;

pub(crate) const ENTITY_CLASSIFICATION: &str = "classification";
pub(crate) const ENTITY_LINKS: &str = "links";
pub(crate) const ENTITY_AVATAR: &str = "avatar";
pub(crate) const ENTITY_FILE_LINK: &str = "file_link";
pub(crate) const ENTITY_CONFIG: &str = "config";

pub(crate) const SYNC_META_TABLE: &str = "CREATE TABLE IF NOT EXISTS sync_meta (
    entity TEXT NOT NULL CHECK (entity IN ('classification','links','avatar','file_link','config')),
    key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    device_id TEXT,
    device_name TEXT,
    at TEXT,
    PRIMARY KEY (entity, key)
);";

const DEVICE_ID_KEY: &str = "device_id";
pub(crate) const DEVICE_NAME_KEY: &str = "device_name";
const DEVICE_NAME_MAX: usize = 64;
const DEVICE_NAME_FALLBACK: &str = "この端末";
const FILE_LINK_SEPARATOR: char = '\t';

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceIdentity {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncMeta {
    pub device_id: String,
    pub device_name: String,
    pub at: String,
}

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

fn new_device_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "端末IDを作れません".to_string())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

fn default_device_name() -> String {
    let host = gethostname::gethostname().to_string_lossy().into_owned();
    device_name_from_host(&host)
}

fn device_name_from_host(host: &str) -> String {
    let host = host.trim();
    let host = match host.len().checked_sub(".local".len()) {
        Some(end) if host.is_char_boundary(end) && host[end..].eq_ignore_ascii_case(".local") => {
            &host[..end]
        }
        _ => host,
    };
    let bounded: String = host.chars().take(DEVICE_NAME_MAX).collect();
    clean_device_name(&bounded).unwrap_or_else(|_| DEVICE_NAME_FALLBACK.into())
}

/// Create the device id and default name once. Later startups and updates keep the stored values.
pub(crate) fn ensure_device_identity(connection: &Connection) -> Result<(), String> {
    if db::setting(connection, DEVICE_ID_KEY)?.is_none_or(|value| value.is_empty()) {
        db::set_setting(connection, DEVICE_ID_KEY, &new_device_id()?)?;
    }
    if db::setting(connection, DEVICE_NAME_KEY)?.is_none_or(|value| value.is_empty()) {
        db::set_setting(connection, DEVICE_NAME_KEY, &default_device_name())?;
    }
    Ok(())
}

pub(crate) fn device_identity(connection: &Connection) -> Result<DeviceIdentity, String> {
    Ok(DeviceIdentity {
        id: db::setting(connection, DEVICE_ID_KEY)?.unwrap_or_default(),
        name: db::setting(connection, DEVICE_NAME_KEY)?.unwrap_or_default(),
    })
}

pub(crate) fn clean_device_name(value: &str) -> Result<String, String> {
    let name = value.trim();
    if name.is_empty() {
        return Err("端末名を入力してください".into());
    }
    if name.chars().count() > DEVICE_NAME_MAX {
        return Err(format!("端末名は {DEVICE_NAME_MAX} 文字以内にしてください"));
    }
    if name.chars().any(char::is_control) {
        return Err("端末名に制御文字は使えません".into());
    }
    Ok(name.to_string())
}

/// The only definition of which settings leave this device, in the export's fixed order.
/// A new settings key stays private until it is added here by hand.
pub(crate) fn config_entries(settings: &Settings) -> Vec<(&'static str, &'static str, Value)> {
    vec![
        ("llm_backend", "llmBackend", json!(settings.llm_backend)),
        ("ollama_model", "ollamaModel", json!(settings.ollama_model)),
        ("cloud_model", "cloudModel", json!(settings.cloud_model)),
        (
            "confidence_threshold",
            "confidenceThreshold",
            json!(settings.confidence_threshold),
        ),
        (
            "unload_ollama_after_classification",
            "unloadOllamaAfterClassification",
            json!(settings.unload_ollama_after_classification),
        ),
        ("avatar_hero", "avatarHero", json!(settings.avatar_hero)),
        (
            "booth_link_target",
            "boothLinkTarget",
            json!(settings.booth_link_target),
        ),
        ("preview_pane", "previewPane", json!(settings.preview_pane)),
        (
            "browser_force_dark",
            "browserForceDark",
            json!(settings.browser_force_dark),
        ),
        (
            "check_update_on_startup",
            "checkUpdateOnStartup",
            json!(settings.check_update_on_startup),
        ),
    ]
}

/// Fixed-width UTC, so `at` values sort as text.
pub(crate) fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// The value of one group as it is now.
struct Current {
    entity: &'static str,
    key: String,
    fingerprint: String,
    /// An empty group keeps its fingerprint up to date but never carries a device or time,
    /// so a re-import that only cleared a value cannot win over a manual value elsewhere.
    empty: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A missing row is a change.
    Touch,
    /// A missing row is a baseline, and a different fingerprint is a change.
    Reconcile,
    /// A missing row is a baseline, and existing rows stay as they are.
    Baseline,
}

fn store(connection: &Connection, rows: Vec<Current>, mode: Mode) -> Result<(), String> {
    let mut read = sql(connection
        .prepare_cached("SELECT fingerprint FROM sync_meta WHERE entity = ?1 AND key = ?2"))?;
    let mut write = sql(connection.prepare_cached(
        "INSERT INTO sync_meta (entity, key, fingerprint, device_id, device_name, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(entity, key) DO UPDATE SET fingerprint = excluded.fingerprint,
           device_id = excluded.device_id, device_name = excluded.device_name, at = excluded.at",
    ))?;
    let mut identity: Option<DeviceIdentity> = None;
    for row in rows {
        let stored: Option<String> = sql(read
            .query_row(params![row.entity, row.key], |found| found.get(0))
            .optional())?;
        let record = match (&stored, mode) {
            (Some(fingerprint), _) if *fingerprint == row.fingerprint => continue,
            (Some(_), Mode::Baseline) => continue,
            (Some(_), _) | (None, Mode::Touch) => !row.empty,
            (None, _) => false,
        };
        if record {
            if identity.is_none() {
                identity = Some(device_identity(connection)?);
            }
            let device = identity.as_ref().ok_or("端末情報を読めません")?;
            sql(write.execute(params![
                row.entity,
                row.key,
                row.fingerprint,
                device.id,
                device.name,
                timestamp()
            ]))?;
        } else {
            sql(write.execute(params![
                row.entity,
                row.key,
                row.fingerprint,
                Option::<String>::None,
                Option::<String>::None,
                Option::<String>::None
            ]))?;
        }
    }
    Ok(())
}

fn product_rows(connection: &Connection, booth_id: Option<&str>) -> Result<Vec<Current>, String> {
    let filter = if booth_id.is_some() { "WHERE booth_id = ?1" } else { "" };
    let mut statement = sql(connection.prepare_cached(&format!(
        "SELECT booth_id, category, category_locked, genre, genre_locked, confidence,
                classifier_version, user_confirmed, needs_review, is_generic, links_locked,
                compatible_avatars_json, unresolved_names_json
         FROM products {filter}"
    )))?;
    let filter = if booth_id.is_some() { "WHERE product_id = ?1" } else { "" };
    let mut links = sql(connection.prepare_cached(&format!(
        "SELECT product_id, avatar_id, source FROM product_avatars {filter}
         ORDER BY product_id, avatar_id"
    )))?;
    let mut linked: HashMap<String, Vec<Value>> = HashMap::new();
    let link_rows = sql(links.query_map(params_from_iter(booth_id), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }))?;
    for link in link_rows {
        let (product, avatar, source) = sql(link)?;
        linked.entry(product).or_default().push(json!([avatar, source]));
    }
    let mut rows = Vec::new();
    let products = sql(statement.query_map(params_from_iter(booth_id), |row| {
        let booth_id: String = row.get(0)?;
        let category: Option<String> = row.get(1)?;
        let category_locked: bool = row.get(2)?;
        let genre: Option<String> = row.get(3)?;
        let genre_locked: bool = row.get(4)?;
        let confidence: Option<f64> = row.get(5)?;
        let classifier_version: i64 = row.get(6)?;
        let user_confirmed: bool = row.get(7)?;
        let needs_review: bool = row.get(8)?;
        let is_generic: bool = row.get(9)?;
        let links_locked: bool = row.get(10)?;
        let compatible: String = row.get(11)?;
        let unresolved: String = row.get(12)?;
        let classification = json!([
            category,
            category_locked,
            genre,
            genre_locked,
            confidence,
            classifier_version,
            user_confirmed,
            needs_review,
            is_generic
        ]);
        let empty = category.is_none() && !category_locked && !genre_locked && !user_confirmed;
        Ok((booth_id, classification, empty, links_locked, compatible, unresolved))
    }))?;
    for product in products {
        let (booth_id, classification, empty, links_locked, compatible, unresolved) = sql(product)?;
        let items = linked.remove(&booth_id).unwrap_or_default();
        let links_empty = items.is_empty() && !links_locked;
        rows.push(Current {
            entity: ENTITY_CLASSIFICATION,
            key: booth_id.clone(),
            fingerprint: classification.to_string(),
            empty,
        });
        rows.push(Current {
            entity: ENTITY_LINKS,
            key: booth_id,
            fingerprint: json!([links_locked, items, compatible, unresolved]).to_string(),
            empty: links_empty,
        });
    }
    Ok(rows)
}

fn avatar_rows(connection: &Connection, id: Option<&str>) -> Result<Vec<Current>, String> {
    let filter = if id.is_some() { "WHERE id = ?1" } else { "" };
    let mut statement = sql(connection.prepare_cached(&format!(
        "SELECT id, display_name, base_body, aliases_json, owned, notes FROM avatars {filter}"
    )))?;
    let rows = sql(statement.query_map(params_from_iter(id), |row| {
        let key: String = row.get(0)?;
        let display_name: String = row.get(1)?;
        let base_body: Option<String> = row.get(2)?;
        let aliases: String = row.get(3)?;
        let owned: bool = row.get(4)?;
        let notes: String = row.get(5)?;
        Ok(Current {
            entity: ENTITY_AVATAR,
            key,
            fingerprint: json!([display_name, base_body, aliases, owned, notes]).to_string(),
            empty: false,
        })
    }))?;
    sql(rows.collect())
}

/// One link per product and file name, in a fixed order. Folders, names that are not plain file
/// names, and lower-ranked duplicates do not count. A file name can sit in several folders.
pub(crate) fn file_links(
    connection: &Connection,
    booth_id: Option<&str>,
) -> Result<Vec<(String, String, Option<String>)>, String> {
    fn rank(source: Option<&str>) -> u8 {
        match source {
            Some("manual") => 3,
            Some("download") => 2,
            Some("auto") => 1,
            _ => 0,
        }
    }
    let filter = if booth_id.is_some() {
        "linked_product_id = ?1"
    } else {
        "linked_product_id IS NOT NULL"
    };
    let mut statement = sql(connection.prepare_cached(&format!(
        "SELECT linked_product_id, filename, link_source FROM local_files
         WHERE {filter} AND is_dir = 0"
    )))?;
    let rows = sql(statement.query_map(params_from_iter(booth_id), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    }))?;
    let mut best: std::collections::BTreeMap<(String, String), Option<String>> = Default::default();
    for row in rows {
        let (product, filename, source) = sql(row)?;
        let Some(filename) = export_filename(&filename) else {
            continue;
        };
        let slot = (product, filename);
        let wins = best
            .get(&slot)
            .is_none_or(|held| rank(source.as_deref()) > rank(held.as_deref()));
        if wins {
            best.insert(slot, source);
        }
    }
    Ok(best
        .into_iter()
        .map(|((product, filename), source)| (product, filename, source))
        .collect())
}


/// Keep the exact on-disk name. Trimmed names would collide (`a.zip` vs ` a.zip`) and no longer match the scan row.
fn export_filename(value: &str) -> Option<String> {
    if value != value.trim() {
        return None;
    }
    crate::library_files::safe_filename(value)
}

pub(crate) fn file_link_key(booth_id: &str, filename: &str) -> String {
    format!("{booth_id}{FILE_LINK_SEPARATOR}{filename}")
}

fn file_link_rows(connection: &Connection, booth_id: Option<&str>) -> Result<Vec<Current>, String> {
    Ok(file_links(connection, booth_id)?
        .into_iter()
        .map(|(product, filename, source)| Current {
            entity: ENTITY_FILE_LINK,
            key: file_link_key(&product, &filename),
            fingerprint: json!(source).to_string(),
            empty: false,
        })
        .collect())
}

fn config_rows(connection: &Connection) -> Result<Vec<Current>, String> {
    Ok(config_entries(&db::settings(connection)?)
        .into_iter()
        .map(|(key, _, value)| Current {
            entity: ENTITY_CONFIG,
            key: key.to_string(),
            fingerprint: value.to_string(),
            empty: false,
        })
        .collect())
}

fn all_rows(connection: &Connection) -> Result<Vec<Current>, String> {
    let mut rows = product_rows(connection, None)?;
    rows.extend(avatar_rows(connection, None)?);
    rows.extend(file_link_rows(connection, None)?);
    rows.extend(config_rows(connection)?);
    Ok(rows)
}

pub(crate) fn touch_product(connection: &Connection, booth_id: &str) -> Result<(), String> {
    store(connection, product_rows(connection, Some(booth_id))?, Mode::Touch)
}

pub(crate) fn touch_avatar(connection: &Connection, id: &str) -> Result<(), String> {
    store(connection, avatar_rows(connection, Some(id))?, Mode::Touch)
}

pub(crate) fn touch_file_links(connection: &Connection, booth_id: &str) -> Result<(), String> {
    store(connection, file_link_rows(connection, Some(booth_id))?, Mode::Touch)
}

pub(crate) fn touch_config(connection: &Connection) -> Result<(), String> {
    store(connection, config_rows(connection)?, Mode::Touch)
}

pub(crate) fn reconcile_file_links(connection: &Connection) -> Result<(), String> {
    store(connection, file_link_rows(connection, None)?, Mode::Reconcile)
}

pub(crate) fn reconcile_all(connection: &Connection) -> Result<(), String> {
    store(connection, all_rows(connection)?, Mode::Reconcile)
}

pub(crate) fn baseline_missing(connection: &Connection) -> Result<(), String> {
    store(connection, all_rows(connection)?, Mode::Baseline)
}

/// Rows that have a recorded change. A baseline or empty row has no history, so it is absent here.
pub(crate) fn load_meta(
    connection: &Connection,
    entity: &str,
) -> Result<HashMap<String, SyncMeta>, String> {
    let mut statement = sql(connection.prepare_cached(
        "SELECT key, device_id, device_name, at FROM sync_meta
         WHERE entity = ?1 AND at IS NOT NULL AND device_id IS NOT NULL AND device_name IS NOT NULL",
    ))?;
    let rows = sql(statement.query_map(params![entity], |row| {
        Ok((
            row.get::<_, String>(0)?,
            SyncMeta {
                device_id: row.get(1)?,
                device_name: row.get(2)?,
                at: row.get(3)?,
            },
        ))
    }))?;
    sql(rows.collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProductUpdate, SettingsPatch};
    use crate::scan::{ScanEntry, ScanOutcome};
    use rusqlite::Connection;

    fn product_json(id: &str, title: &str, description: &str) -> String {
        json!({ "products": [{ "boothId": id, "title": title, "description": description }] })
            .to_string()
    }

    fn with_product(id: &str) -> Connection {
        let mut connection = db::open_memory().unwrap();
        db::import_export_json(&mut connection, &product_json(id, "見本の衣装", "見本の説明"))
            .unwrap();
        connection
    }

    fn meta_row(
        connection: &Connection,
        entity: &str,
        key: &str,
    ) -> Option<(Option<String>, Option<String>, Option<String>)> {
        connection
            .query_row(
                "SELECT device_id, device_name, at FROM sync_meta WHERE entity = ?1 AND key = ?2",
                params![entity, key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .unwrap()
    }

    fn at(connection: &Connection, entity: &str, key: &str) -> Option<String> {
        meta_row(connection, entity, key).and_then(|row| row.2)
    }

    fn manual_update(id: &str, category: &str) -> ProductUpdate {
        ProductUpdate {
            booth_id: id.into(),
            category: Some(category.into()),
            genre: None,
            is_generic: None,
            avatar_ids: None,
            confirm: None,
        }
    }

    fn file_scan(path: &str, filename: &str, label: &str) -> ScanOutcome {
        ScanOutcome {
            entries: vec![ScanEntry {
                path: path.into(),
                filename: filename.into(),
                is_dir: false,
                size: 4,
                labels: vec![label.into()],
            }],
            truncated: false,
        }
    }

    fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        (dir, path)
    }

    #[test]
    fn v7_database_migrates_to_v8_with_baseline() {
        let (_dir, path) = temp_db();
        {
            let mut connection = db::open(&path).unwrap();
            db::import_export_json(&mut connection, &product_json("9001", "既存", "説明")).unwrap();
            connection
                .execute_batch(
                    "UPDATE products SET category = 'outfit', category_locked = 1 WHERE booth_id = '9001';
                     INSERT INTO avatars (id, display_name) VALUES ('hand-made', '手作り');
                     DROP TABLE sync_meta;
                     DELETE FROM settings WHERE key IN ('device_id', 'device_name');
                     PRAGMA user_version = 7;",
                )
                .unwrap();
        }
        let connection = db::open(&path).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 8);
        for (entity, key) in [
            (ENTITY_CLASSIFICATION, "9001"),
            (ENTITY_LINKS, "9001"),
            (ENTITY_AVATAR, "hand-made"),
        ] {
            assert_eq!(meta_row(&connection, entity, key), Some((None, None, None)));
        }
        let keys: Vec<&str> = config_entries(&db::settings(&connection).unwrap())
            .iter()
            .map(|entry| entry.0)
            .collect();
        assert_eq!(keys.len(), 10);
        for key in keys {
            assert_eq!(meta_row(&connection, ENTITY_CONFIG, key), Some((None, None, None)));
        }
        let category: String = connection
            .query_row("SELECT category FROM products WHERE booth_id = '9001'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(category, "outfit");
    }

    #[test]
    fn device_identity_is_created_once() {
        let (_dir, path) = temp_db();
        let first = {
            let connection = db::open(&path).unwrap();
            device_identity(&connection).unwrap()
        };
        let parts: Vec<&str> = first.id.split('-').collect();
        assert_eq!(parts.iter().map(|part| part.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert!(parts[2].starts_with('4'));
        assert!(matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')));
        assert!(first.id.chars().all(|ch| ch == '-' || ch.is_ascii_hexdigit()));
        assert!(!first.name.is_empty());
        let connection = db::open(&path).unwrap();
        assert_eq!(device_identity(&connection).unwrap(), first);
    }

    #[test]
    fn device_name_defaults_from_the_host() {
        assert_eq!(device_name_from_host("kei-mac.local"), "kei-mac");
        assert_eq!(device_name_from_host("KEI-MAC.LOCAL"), "KEI-MAC");
        assert_eq!(device_name_from_host("  desk  "), "desk");
        assert_eq!(device_name_from_host(""), DEVICE_NAME_FALLBACK);
        assert_eq!(device_name_from_host(".local"), DEVICE_NAME_FALLBACK);
        assert_eq!(device_name_from_host("机"), "机");
        assert_eq!(device_name_from_host(&"あ".repeat(70)).chars().count(), 64);
    }

    #[test]
    fn device_name_is_validated_and_saved_through_patch() {
        let connection = db::open_memory().unwrap();
        let patch = |name: &str| SettingsPatch {
            device_name: Some(name.into()),
            ..Default::default()
        };
        let before = db::settings(&connection).unwrap().device_name;
        assert!(db::update_settings(&connection, &patch("   ")).is_err());
        assert!(db::update_settings(&connection, &patch(&"a".repeat(65))).is_err());
        assert!(db::update_settings(&connection, &patch("a\u{7}b")).is_err());
        assert_eq!(db::settings(&connection).unwrap().device_name, before);
        let saved = db::update_settings(&connection, &patch("  kei-desk-2 ")).unwrap();
        assert_eq!(saved.device_name, "kei-desk-2");
        assert_eq!(db::settings(&connection).unwrap().device_name, "kei-desk-2");
        assert_eq!(db::update_settings(&connection, &patch(&"あ".repeat(64))).unwrap().device_name.chars().count(), 64);
    }

    #[test]
    fn full_settings_save_keeps_device_name() {
        let connection = db::open_memory().unwrap();
        db::update_settings(
            &connection,
            &SettingsPatch {
                device_name: Some("kei-desk-2".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let input = SettingsPatch::default().apply(db::settings(&connection).unwrap());
        db::save_settings(&connection, &input).unwrap();
        assert_eq!(db::settings(&connection).unwrap().device_name, "kei-desk-2");
    }

    #[test]
    fn reopening_does_not_advance_meta() {
        let (_dir, path) = temp_db();
        {
            let mut connection = db::open(&path).unwrap();
            db::import_export_json(&mut connection, &product_json("9001", "見本の衣装", "説明")).unwrap();
            db::update_product(&mut connection, &manual_update("9001", "outfit")).unwrap();
            db::update_settings(
                &connection,
                &SettingsPatch {
                    avatar_hero: Some("large".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let dump = |connection: &Connection| -> Vec<(String, String, String, Option<String>)> {
            let mut statement = connection
                .prepare("SELECT entity, key, fingerprint, at FROM sync_meta ORDER BY entity, key")
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let first = dump(&db::open(&path).unwrap());
        assert!(first.iter().any(|row| row.0 == ENTITY_CLASSIFICATION && row.3.is_some()));
        let second = dump(&db::open(&path).unwrap());
        assert_eq!(first, second);
    }

    #[test]
    fn identical_reresolve_does_not_advance_meta() {
        let mut connection = with_product("9001");
        db::update_product(&mut connection, &manual_update("9001", "outfit")).unwrap();
        db::reresolve_all(&mut connection).unwrap();
        let before = at(&connection, ENTITY_CLASSIFICATION, "9001");
        assert!(before.is_some());
        db::reresolve_all(&mut connection).unwrap();
        db::reresolve_all(&mut connection).unwrap();
        assert_eq!(at(&connection, ENTITY_CLASSIFICATION, "9001"), before);
    }

    #[test]
    fn manual_classification_records_device_and_time() {
        let mut connection = with_product("9001");
        let device = device_identity(&connection).unwrap();
        db::update_product(
            &mut connection,
            &ProductUpdate {
                avatar_ids: Some(vec!["lime".into()]),
                ..manual_update("9001", "outfit")
            },
        )
        .unwrap();
        for entity in [ENTITY_CLASSIFICATION, ENTITY_LINKS] {
            let (id, name, time) = meta_row(&connection, entity, "9001").unwrap();
            assert_eq!(id.as_deref(), Some(device.id.as_str()));
            assert_eq!(name.as_deref(), Some(device.name.as_str()));
            let time = time.unwrap();
            assert!(time.ends_with('Z'));
            assert!(chrono::DateTime::parse_from_rfc3339(&time).is_ok());
        }
    }

    #[test]
    fn genre_move_and_undo_are_recorded() {
        let mut connection = with_product("9001");
        connection
            .execute("UPDATE products SET category = 'outfit' WHERE booth_id = '9001'", [])
            .unwrap();
        let snapshot = db::move_product_genre(&mut connection, "9001", "asset", "hair").unwrap();
        let moved = at(&connection, ENTITY_CLASSIFICATION, "9001");
        assert!(moved.is_some());
        db::restore_product_classification(&mut connection, &snapshot).unwrap();
        let restored = at(&connection, ENTITY_CLASSIFICATION, "9001");
        assert!(restored > moved, "{restored:?} {moved:?}");
    }

    /// The empty rule outranks the undo: an unclassified product stays without history.
    #[test]
    fn undoing_a_move_back_to_empty_clears_meta() {
        let mut connection = with_product("9001");
        let snapshot = db::move_product_genre(&mut connection, "9001", "asset", "hair").unwrap();
        assert!(at(&connection, ENTITY_CLASSIFICATION, "9001").is_some());
        db::restore_product_classification(&mut connection, &snapshot).unwrap();
        assert_eq!(at(&connection, ENTITY_CLASSIFICATION, "9001"), None);
    }

    #[test]
    fn owned_body_records_avatar_and_product() {
        let mut connection = with_product("9001");
        let result = db::register_manual_owned_body(&mut connection, "9001", Some("見本ちゃん")).unwrap();
        let crate::model::RegisterOwnedBodyResult::Registered { avatar_id, .. } = result else {
            panic!("名前を渡したので登録される");
        };
        assert!(at(&connection, ENTITY_AVATAR, &avatar_id).is_some());
        assert!(at(&connection, ENTITY_CLASSIFICATION, "9001").is_some());
        assert!(at(&connection, ENTITY_LINKS, "9001").is_some());
    }

    #[test]
    fn file_link_meta_survives_rescan() {
        let mut connection = with_product("9001");
        let scan = file_scan("/library/9001/outfit.zip", "outfit.zip", "9001_outfit");
        db::apply_scan(&mut connection, "/library", &scan).unwrap();
        let key = file_link_key("9001", "outfit.zip");
        assert_eq!(at(&connection, ENTITY_FILE_LINK, &key), None);
        db::link_file(&connection, "/library/9001/outfit.zip", Some("9001")).unwrap();
        let linked = at(&connection, ENTITY_FILE_LINK, &key);
        assert!(linked.is_some());
        db::apply_scan(&mut connection, "/library", &scan).unwrap();
        db::apply_scan(&mut connection, "/library", &scan).unwrap();
        assert_eq!(at(&connection, ENTITY_FILE_LINK, &key), linked);
    }

    #[test]
    fn manual_file_link_records_meta() {
        let mut connection = with_product("9001");
        let scan = file_scan("/secret/place/plain.zip", "plain.zip", "plain");
        db::apply_scan(&mut connection, "/secret/place", &scan).unwrap();
        db::link_file(&connection, "/secret/place/plain.zip", Some("9001")).unwrap();
        assert!(at(&connection, ENTITY_FILE_LINK, &file_link_key("9001", "plain.zip")).is_some());
        let stored: String = connection
            .query_row(
                "SELECT group_concat(key || fingerprint) FROM sync_meta WHERE entity = 'file_link'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!stored.contains("secret"));
    }

    #[test]
    fn folder_links_and_unsafe_names_have_no_file_link_meta() {
        let connection = with_product("9001");
        connection
            .execute_batch(
                "INSERT INTO local_files (path, filename, is_dir, linked_product_id, link_source, scanned_at)
                 VALUES ('/l/folder', 'folder', 1, '9001', 'manual', 't'),
                        ('/l/bad', 'a/b.zip', 0, '9001', 'manual', 't');",
            )
            .unwrap();
        assert!(file_links(&connection, None).unwrap().is_empty());
    }

    #[test]
    fn same_name_prefers_manual_then_download_then_auto() {
        let connection = with_product("9001");
        connection
            .execute_batch(
                "INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at)
                 VALUES ('/a/x.zip', 'x.zip', '9001', 'auto', 't'),
                        ('/b/x.zip', 'x.zip', '9001', 'download', 't');",
            )
            .unwrap();
        let top = |connection: &Connection| file_links(connection, Some("9001")).unwrap()[0].2.clone();
        assert_eq!(top(&connection).as_deref(), Some("download"));
        connection
            .execute_batch(
                "INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at)
                 VALUES ('/c/x.zip', 'x.zip', '9001', 'manual', 't');",
            )
            .unwrap();
        assert_eq!(file_links(&connection, None).unwrap().len(), 1);
        assert_eq!(top(&connection).as_deref(), Some("manual"));
    }

    #[test]
    fn config_touch_records_only_changed_allowlisted_keys() {
        let connection = db::open_memory().unwrap();
        db::update_settings(
            &connection,
            &SettingsPatch {
                avatar_hero: Some("large".into()),
                ui_rail_width: Some(300),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(at(&connection, ENTITY_CONFIG, "avatar_hero").is_some());
        assert_eq!(at(&connection, ENTITY_CONFIG, "ollama_model"), None);
        let recorded: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_meta WHERE entity = 'config' AND at IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(recorded, 1);
        let blocked: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sync_meta WHERE key IN ('ui_rail_width', 'library_folder')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(blocked, 0);
    }

    #[test]
    fn empty_classification_never_carries_meta() {
        let mut connection = db::open_memory().unwrap();
        db::import_export_json(&mut connection, &product_json("9001", "見本", "最初の説明")).unwrap();
        db::update_product(&mut connection, &manual_update("9001", "outfit")).unwrap();
        connection
            .execute("UPDATE products SET category_locked = 0, genre_locked = 0, user_confirmed = 1 WHERE booth_id = '9001'", [])
            .unwrap();
        db::reresolve_all(&mut connection).unwrap();
        assert!(at(&connection, ENTITY_CLASSIFICATION, "9001").is_some());
        db::import_export_json(&mut connection, &product_json("9001", "見本", "変わった説明")).unwrap();
        assert_eq!(
            meta_row(&connection, ENTITY_CLASSIFICATION, "9001"),
            Some((None, None, None))
        );
        assert_eq!(
            meta_row(&connection, ENTITY_LINKS, "9001"),
            Some((None, None, None))
        );
    }

    #[test]
    fn reconcile_records_only_real_differences() {
        let connection = with_product("9001");
        reconcile_all(&connection).unwrap();
        assert_eq!(at(&connection, ENTITY_CLASSIFICATION, "9001"), None);
        connection
            .execute("UPDATE products SET genre = 'hair' WHERE booth_id = '9001'", [])
            .unwrap();
        baseline_missing(&connection).unwrap();
        assert_eq!(at(&connection, ENTITY_CLASSIFICATION, "9001"), None);
        connection
            .execute("UPDATE products SET category = 'hair' WHERE booth_id = '9001'", [])
            .unwrap();
        reconcile_all(&connection).unwrap();
        assert!(at(&connection, ENTITY_CLASSIFICATION, "9001").is_some());
    }
}
