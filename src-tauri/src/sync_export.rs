//! The `vrclibman-sync` export file. Columns are an explicit list: descriptions, thumbnails, and
//! local paths are never read here, and settings leave only through `config_entries`.

use crate::db;
use crate::sync_meta::{self, SyncMeta};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::Path;

const FORMAT: &str = "vrclibman-sync";
const VERSION: u32 = 1;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncDevice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncFile {
    pub format: &'static str,
    pub version: u32,
    pub device: SyncDevice,
    pub app_version: String,
    pub exported_at: String,
    pub config: SyncConfig,
    pub products: Vec<SyncProduct>,
    pub avatars: Vec<SyncAvatar>,
    pub file_links: Vec<SyncFileLink>,
    pub counts: SyncCounts,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncConfig {
    pub values: BTreeMap<String, Value>,
    pub meta: BTreeMap<String, Option<SyncMeta>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncProduct {
    pub booth_id: String,
    pub title: String,
    pub url: String,
    pub classification: SyncClassification,
    pub links: SyncLinks,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncClassification {
    pub category: Option<String>,
    pub category_locked: bool,
    pub genre: Option<String>,
    pub genre_locked: bool,
    pub confidence: Option<f64>,
    pub classifier_version: i64,
    pub user_confirmed: bool,
    pub needs_review: bool,
    pub is_generic: bool,
    pub meta: Option<SyncMeta>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncLinks {
    pub items: Vec<SyncLink>,
    pub locked: bool,
    pub names: Vec<String>,
    pub unresolved_names: Vec<String>,
    pub meta: Option<SyncMeta>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncLink {
    pub avatar_id: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncAvatar {
    pub id: String,
    pub display_name: String,
    pub base_body: Option<String>,
    pub owned: bool,
    pub aliases: Vec<String>,
    pub notes: String,
    pub meta: Option<SyncMeta>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncFileLink {
    pub booth_id: String,
    pub filename: String,
    pub link_source: Option<String>,
    pub meta: Option<SyncMeta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncCounts {
    pub products: usize,
    pub links: usize,
    pub avatars: usize,
    pub file_links: usize,
    pub config: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncExportReport {
    pub counts: SyncCounts,
}

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|err| err.to_string())
}

fn parse_names(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn product_url(booth_id: &str) -> String {
    format!("https://booth.pm/ja/items/{booth_id}")
}

fn excluded_ids(connection: &Connection) -> Result<HashSet<String>, String> {
    let mut statement = sql(connection.prepare("SELECT booth_id FROM product_exclusions"))?;
    let rows = sql(statement.query_map([], |row| row.get(0)))?;
    sql(rows.collect())
}

fn load_links(connection: &Connection) -> Result<HashMap<String, Vec<SyncLink>>, String> {
    let mut statement = sql(connection.prepare(
        "SELECT product_id, avatar_id, source FROM product_avatars ORDER BY product_id, avatar_id",
    ))?;
    let rows = sql(statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            SyncLink {
                avatar_id: row.get(1)?,
                source: row.get(2)?,
            },
        ))
    }))?;
    let mut links: HashMap<String, Vec<SyncLink>> = HashMap::new();
    for row in rows {
        let (product, link) = sql(row)?;
        links.entry(product).or_default().push(link);
    }
    Ok(links)
}

fn load_products(connection: &Connection) -> Result<Vec<SyncProduct>, String> {
    let mut classification_meta = sync_meta::load_meta(connection, sync_meta::ENTITY_CLASSIFICATION)?;
    let mut links_meta = sync_meta::load_meta(connection, sync_meta::ENTITY_LINKS)?;
    let mut links = load_links(connection)?;
    let mut statement = sql(connection.prepare(
        "SELECT booth_id, title, category, category_locked, genre, genre_locked, confidence,
                classifier_version, user_confirmed, needs_review, is_generic, links_locked,
                compatible_avatars_json, unresolved_names_json
         FROM products
         WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)
         ORDER BY booth_id",
    ))?;
    let rows = sql(statement.query_map([], |row| {
        let booth_id: String = row.get(0)?;
        let classification = SyncClassification {
            category: row.get(2)?,
            category_locked: row.get(3)?,
            genre: row.get(4)?,
            genre_locked: row.get(5)?,
            confidence: row.get(6)?,
            classifier_version: row.get(7)?,
            user_confirmed: row.get(8)?,
            needs_review: row.get(9)?,
            is_generic: row.get(10)?,
            meta: None,
        };
        let group = SyncLinks {
            items: Vec::new(),
            locked: row.get(11)?,
            names: parse_names(&row.get::<_, String>(12)?),
            unresolved_names: parse_names(&row.get::<_, String>(13)?),
            meta: None,
        };
        Ok(SyncProduct {
            url: product_url(&booth_id),
            booth_id,
            title: row.get(1)?,
            classification,
            links: group,
        })
    }))?;
    let mut products = Vec::new();
    for row in rows {
        let mut product = sql(row)?;
        product.classification.meta = classification_meta.remove(&product.booth_id);
        product.links.meta = links_meta.remove(&product.booth_id);
        product.links.items = links.remove(&product.booth_id).unwrap_or_default();
        products.push(product);
    }
    Ok(products)
}

/// Avatars another device needs: owned, linked, user-made, or changed here. The untouched bundled
/// catalog is left out because every install already has it.
fn load_avatars(connection: &Connection, products: &[SyncProduct]) -> Result<Vec<SyncAvatar>, String> {
    let mut meta = sync_meta::load_meta(connection, sync_meta::ENTITY_AVATAR)?;
    let referenced: HashSet<&str> = products
        .iter()
        .flat_map(|product| product.links.items.iter())
        .map(|link| link.avatar_id.as_str())
        .collect();
    let mut statement = sql(connection.prepare(
        "SELECT id, display_name, base_body, aliases_json, owned, notes, is_builtin
         FROM avatars ORDER BY id",
    ))?;
    let rows = sql(statement.query_map([], |row| {
        Ok((
            SyncAvatar {
                id: row.get(0)?,
                display_name: row.get(1)?,
                base_body: row.get(2)?,
                aliases: parse_names(&row.get::<_, String>(3)?),
                owned: row.get(4)?,
                notes: row.get(5)?,
                meta: None,
            },
            row.get::<_, bool>(6)?,
        ))
    }))?;
    let mut avatars = Vec::new();
    for row in rows {
        let (mut avatar, builtin) = sql(row)?;
        let changed = meta.remove(&avatar.id);
        if avatar.owned
            || !builtin
            || changed.is_some()
            || referenced.contains(avatar.id.as_str())
        {
            avatar.meta = changed;
            avatars.push(avatar);
        }
    }
    Ok(avatars)
}

fn load_file_links(connection: &Connection) -> Result<Vec<SyncFileLink>, String> {
    let excluded = excluded_ids(connection)?;
    let mut meta = sync_meta::load_meta(connection, sync_meta::ENTITY_FILE_LINK)?;
    Ok(sync_meta::file_links(connection, None)?
        .into_iter()
        .filter(|(booth_id, _, _)| !excluded.contains(booth_id))
        .map(|(booth_id, filename, link_source)| SyncFileLink {
            meta: meta.remove(&sync_meta::file_link_key(&booth_id, &filename)),
            booth_id,
            filename,
            link_source,
        })
        .collect())
}

fn load_config(connection: &Connection) -> Result<SyncConfig, String> {
    let mut meta = sync_meta::load_meta(connection, sync_meta::ENTITY_CONFIG)?;
    let mut config = SyncConfig {
        values: BTreeMap::new(),
        meta: BTreeMap::new(),
    };
    for (key, name, value) in sync_meta::config_entries(&db::settings(connection)?) {
        config.values.insert(name.to_string(), value);
        config.meta.insert(name.to_string(), meta.remove(key));
    }
    Ok(config)
}

pub(crate) fn build(connection: &mut Connection, app_version: &str) -> Result<SyncFile, String> {
    let tx = sql(connection.transaction())?;
    // A hook that was missed still reaches the export, stamped with this moment.
    sync_meta::reconcile_all(&tx)?;
    let identity = sync_meta::device_identity(&tx)?;
    let config = load_config(&tx)?;
    let products = load_products(&tx)?;
    let avatars = load_avatars(&tx, &products)?;
    let file_links = load_file_links(&tx)?;
    sql(tx.commit())?;
    let counts = SyncCounts {
        products: products.len(),
        links: products.iter().map(|product| product.links.items.len()).sum(),
        avatars: avatars.len(),
        file_links: file_links.len(),
        config: config.values.len(),
    };
    Ok(SyncFile {
        format: FORMAT,
        version: VERSION,
        device: SyncDevice {
            id: identity.id,
            name: identity.name,
        },
        app_version: app_version.to_string(),
        exported_at: sync_meta::timestamp(),
        config,
        products,
        avatars,
        file_links,
        counts,
    })
}

pub(crate) fn to_json(file: &SyncFile) -> Result<String, String> {
    let mut json = serde_json::to_string_pretty(file).map_err(|_| "JSONにできません".to_string())?;
    json.push('\n');
    Ok(json)
}

/// Write beside the destination and rename, so a failure never leaves a partial file there.
pub(crate) fn save(path: &Path, body: &str) -> Result<(), String> {
    let destination = db::checked_json_destination(path)?;
    let parent = destination.parent().ok_or("保存先が不正です")?;
    let failed = |_| "ファイルを保存できません".to_string();
    let mut temp = tempfile::Builder::new()
        .prefix(".vrclibman-export-")
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(failed)?;
    temp.write_all(body.as_bytes()).map_err(failed)?;
    temp.as_file().sync_all().map_err(failed)?;
    temp.persist(&destination)
        .map_err(|_| "ファイルを保存できません".to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProductUpdate, SettingsPatch};
    use rusqlite::params;
    use serde_json::json;

    const CONFIG_KEYS: [&str; 10] = [
        "avatarHero",
        "boothLinkTarget",
        "browserForceDark",
        "checkUpdateOnStartup",
        "cloudModel",
        "confidenceThreshold",
        "llmBackend",
        "ollamaModel",
        "previewPane",
        "unloadOllamaAfterClassification",
    ];

    fn import(connection: &mut Connection, id: &str, title: &str, description: &str) {
        let body = json!({ "products": [{ "boothId": id, "title": title, "description": description }] });
        db::import_export_json(connection, &body.to_string()).unwrap();
    }

    fn export(connection: &mut Connection) -> Value {
        let file = build(connection, "9.9.9").unwrap();
        serde_json::from_str(&to_json(&file).unwrap()).unwrap()
    }

    fn lock_as_outfit(connection: &mut Connection, id: &str, avatars: Option<Vec<String>>) {
        db::update_product(
            connection,
            &ProductUpdate {
                booth_id: id.into(),
                category: Some("outfit".into()),
                genre: None,
                is_generic: None,
                avatar_ids: avatars,
                confirm: None,
            },
        )
        .unwrap();
    }

    fn sample_with_links() -> Connection {
        let mut connection = db::open_memory().unwrap();
        import(&mut connection, "1001", "見本の衣装", "見本の説明本文");
        lock_as_outfit(&mut connection, "1001", Some(vec!["lime".into()]));
        connection
    }

    fn product<'a>(file: &'a Value, id: &str) -> &'a Value {
        file["products"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["boothId"] == id)
            .unwrap()
    }

    fn avatar_ids(file: &Value) -> Vec<&str> {
        file["avatars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn export_of_empty_db() {
        let mut connection = db::open_memory().unwrap();
        let file = export(&mut connection);
        for key in [
            "format", "version", "device", "appVersion", "exportedAt", "config", "products",
            "avatars", "fileLinks", "counts",
        ] {
            assert!(file.get(key).is_some(), "{key}");
        }
        assert_eq!(file["format"], "vrclibman-sync");
        assert_eq!(file["version"], 1);
        assert_eq!(file["appVersion"], "9.9.9");
        assert_eq!(file["products"].as_array().unwrap().len(), 0);
        assert_eq!(file["config"]["values"].as_object().unwrap().len(), 10);
        assert_eq!(file["config"]["meta"].as_object().unwrap().len(), 10);
        assert!(file["config"]["meta"]["avatarHero"].is_null());
        assert!(!file["device"]["id"].as_str().unwrap().is_empty());
    }

    #[test]
    fn export_of_sample_products() {
        let mut connection = sample_with_links();
        import(&mut connection, "1002", "未分類の見本", "別の説明");
        connection
            .execute_batch(
                "UPDATE products SET compatible_avatars_json = '[\"ライム\"]',
                 unresolved_names_json = '[\"架空ちゃん\"]' WHERE booth_id = '1001';",
            )
            .unwrap();
        let file = export(&mut connection);
        let item = product(&file, "1001");
        assert_eq!(item["title"], "見本の衣装");
        assert_eq!(item["classification"]["category"], "outfit");
        assert_eq!(item["links"]["items"], json!([{ "avatarId": "lime", "source": "manual" }]));
        assert_eq!(item["links"]["locked"], true);
        assert_eq!(item["links"]["names"], json!(["ライム"]));
        assert_eq!(item["links"]["unresolvedNames"], json!(["架空ちゃん"]));
        let unclassified = product(&file, "1002");
        assert!(unclassified["classification"]["category"].is_null());
        assert!(unclassified["classification"]["meta"].is_null());
        assert!(unclassified["links"]["meta"].is_null());
        assert!(avatar_ids(&file).contains(&"lime"));
    }

    #[test]
    fn export_keeps_locked_classification() {
        let mut connection = db::open_memory().unwrap();
        import(&mut connection, "1001", "見本", "説明");
        db::move_product_genre(&mut connection, "1001", "asset", "hair").unwrap();
        let device = sync_meta::device_identity(&connection).unwrap();
        let file = export(&mut connection);
        let classification = &product(&file, "1001")["classification"];
        assert_eq!(classification["categoryLocked"], true);
        assert_eq!(classification["genreLocked"], true);
        assert_eq!(classification["genre"], "hair");
        assert_eq!(classification["meta"]["deviceId"], device.id.as_str());
        assert_eq!(classification["meta"]["deviceName"], device.name.as_str());
        assert!(classification["meta"]["at"].is_string());
    }

    #[test]
    fn export_lists_avatars_referenced_by_links() {
        let mut connection = sample_with_links();
        let owned: bool = connection
            .query_row("SELECT owned FROM avatars WHERE id = 'lime'", [], |row| row.get(0))
            .unwrap();
        assert!(!owned);
        let file = export(&mut connection);
        let lime = file["avatars"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "lime")
            .unwrap();
        assert_eq!(lime["owned"], false);
        assert!(lime["displayName"].is_string());
        assert_eq!(lime["baseBody"], "komado");
    }

    #[test]
    fn export_omits_untouched_builtin_avatars() {
        let mut connection = db::open_memory().unwrap();
        connection
            .execute("INSERT INTO avatars (id, display_name) VALUES ('hand-made', '手作り')", [])
            .unwrap();
        let file = export(&mut connection);
        assert_eq!(avatar_ids(&file), vec!["hand-made"]);
    }

    #[test]
    fn export_skips_excluded_products() {
        let mut connection = db::open_memory().unwrap();
        import(&mut connection, "1001", "残す", "説明");
        import(&mut connection, "1002", "除外する", "説明");
        connection
            .execute_batch(
                "INSERT INTO product_exclusions (booth_id, excluded_at) VALUES ('1002', 't');
                 INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at)
                 VALUES ('/l/a.zip', 'a.zip', '1001', 'manual', 't'),
                        ('/l/b.zip', 'b.zip', '1002', 'manual', 't');",
            )
            .unwrap();
        let file = export(&mut connection);
        assert_eq!(file["products"].as_array().unwrap().len(), 1);
        assert_eq!(file["fileLinks"].as_array().unwrap().len(), 1);
        assert_eq!(file["fileLinks"][0]["boothId"], "1001");
        assert!(!to_json(&build(&mut connection, "1").unwrap()).unwrap().contains("除外する"));
    }

    #[test]
    fn export_config_has_exactly_the_allowlist() {
        let mut connection = db::open_memory().unwrap();
        let file = export(&mut connection);
        for side in ["values", "meta"] {
            let keys: Vec<&str> = file["config"][side]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(keys, CONFIG_KEYS);
        }
    }

    #[test]
    fn export_does_not_leak_blocked_settings() {
        let mut connection = db::open_memory().unwrap();
        for key in [
            "ui_rail_width",
            "ui_genre_width",
            "preview_pane_width",
            "right_panel_tabs",
            "library_folder",
            "remote_library_folder",
            "ollama_base_url",
            "cloud_base_url",
            "extension_token",
            "extension_origin",
            "booth_page_interval_ms",
            "browser_bookmarks",
            "unity_editor_path",
            "asset_update_watch_v1",
        ] {
            db::set_setting(&connection, key, &format!("LEAK-{key}")).unwrap();
        }
        let text = to_json(&build(&mut connection, "1").unwrap()).unwrap();
        assert!(!text.contains("LEAK-"), "{text}");
        assert!(!text.contains("uiRailWidth"));
        assert!(!text.contains("libraryFolder"));
    }

    #[test]
    fn export_has_no_private_data() {
        let mut connection = db::open_memory().unwrap();
        let body = json!({ "products": [{
            "boothId": "1001",
            "title": "見本の衣装",
            "description": "DESCRIPTION-SECRET 本文",
            "url": "https://booth.pm/ja/items/1001?token=URL-SECRET",
            "thumbnailUrl": "https://booth.pximg.net/c/150x150/THUMB-SECRET.jpg",
        }] });
        db::import_export_json(&mut connection, &body.to_string()).unwrap();
        db::set_setting(&connection, "extension_token", "TOKEN-SECRET").unwrap();
        db::update_settings(
            &connection,
            &SettingsPatch {
                ollama_base_url: Some("http://127.0.0.1:54321".into()),
                cloud_base_url: Some("https://api.example.test/v1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        connection
            .execute_batch(
                "UPDATE products SET thumbnail_path = '/home/kei/PATH-SECRET/thumb.jpg' WHERE booth_id = '1001';
                 INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at)
                 VALUES ('/home/kei/PATH-SECRET/a.zip', 'a.zip', '1001', 'manual', 't');
                 INSERT INTO product_files (booth_id, filename, local_path)
                 VALUES ('1001', 'a.zip', '/home/kei/PATH-SECRET/a.zip');
                 UPDATE avatars SET hero_image_path = '/home/kei/PATH-SECRET/hero_images/lime.png';",
            )
            .unwrap();
        let text = to_json(&build(&mut connection, "1").unwrap()).unwrap();
        for secret in [
            "DESCRIPTION-SECRET",
            "URL-SECRET",
            "THUMB-SECRET",
            "PATH-SECRET",
            "TOKEN-SECRET",
            "54321",
            "api.example.test",
            "thumbnailUrl",
            "thumbnail_path",
            "thumbnailPath",
            "hero_image_path",
            "heroImagePath",
            "hero_images",
            "isAdult",
            "/home/kei",
        ] {
            assert!(!text.contains(secret), "{secret}");
        }
        let file: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(file["fileLinks"][0]["filename"], "a.zip");
    }

    #[test]
    fn export_builds_canonical_url() {
        let mut connection = db::open_memory().unwrap();
        let body = json!({ "products": [{
            "boothId": "1001",
            "title": "見本",
            "url": "https://shop.booth.pm/items/1001?utm=abc#top",
        }] });
        db::import_export_json(&mut connection, &body.to_string()).unwrap();
        let file = export(&mut connection);
        assert_eq!(product(&file, "1001")["url"], "https://booth.pm/ja/items/1001");
    }

    #[test]
    fn export_counts_match_arrays() {
        let mut connection = sample_with_links();
        import(&mut connection, "1002", "別の見本", "説明");
        connection
            .execute_batch(
                "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES ('1001', 'plum', 'manual');
                 INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at)
                 VALUES ('/l/a.zip', 'a.zip', '1001', 'auto', 't');",
            )
            .unwrap();
        let file = export(&mut connection);
        let products = file["products"].as_array().unwrap();
        let links: usize = products
            .iter()
            .map(|item| item["links"]["items"].as_array().unwrap().len())
            .sum();
        assert_eq!(links, 2);
        assert_eq!(file["counts"]["products"], products.len());
        assert_eq!(file["counts"]["links"], links);
        assert_eq!(file["counts"]["avatars"], file["avatars"].as_array().unwrap().len());
        assert_eq!(file["counts"]["fileLinks"], file["fileLinks"].as_array().unwrap().len());
        assert_eq!(file["counts"]["config"], 10);
    }

    #[test]
    fn export_order_is_stable() {
        let mut connection = sample_with_links();
        import(&mut connection, "1000", "先の見本", "説明");
        import(&mut connection, "1003", "後の見本", "説明");
        connection
            .execute_batch(
                "INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at)
                 VALUES ('/l/z.zip', 'z.zip', '1001', 'auto', 't'),
                        ('/l/a.zip', 'a.zip', '1001', 'auto', 't');",
            )
            .unwrap();
        let mut first = export(&mut connection);
        let mut second = export(&mut connection);
        let ids: Vec<&str> = first["products"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["boothId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["1000", "1001", "1003"]);
        assert_eq!(first["fileLinks"][0]["filename"], "a.zip");
        first["exportedAt"] = Value::Null;
        second["exportedAt"] = Value::Null;
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        );
    }

    #[test]
    fn export_reconciles_missed_changes() {
        let mut connection = db::open_memory().unwrap();
        import(&mut connection, "1001", "見本", "説明");
        assert!(product(&export(&mut connection), "1001")["classification"]["meta"].is_null());
        connection
            .execute("UPDATE products SET category = 'outfit' WHERE booth_id = '1001'", [])
            .unwrap();
        let file = export(&mut connection);
        assert_eq!(product(&file, "1001")["classification"]["category"], "outfit");
        assert!(product(&file, "1001")["classification"]["meta"]["at"].is_string());
    }

    #[test]
    fn reimport_keeps_locks() {
        let mut connection = db::open_memory().unwrap();
        import(&mut connection, "1001", "見本", "最初の説明");
        lock_as_outfit(&mut connection, "1001", Some(vec!["lime".into()]));
        import(&mut connection, "1002", "見本二", "最初の説明");
        db::move_product_genre(&mut connection, "1002", "asset", "hair").unwrap();
        connection
            .execute("UPDATE products SET category_locked = 0 WHERE booth_id = '1002'", [])
            .unwrap();
        let genre_before: Option<String> = connection
            .query_row("SELECT genre FROM products WHERE booth_id = '1001'", [], |row| row.get(0))
            .unwrap();
        import(&mut connection, "1001", "見本", "変わった説明");
        import(&mut connection, "1002", "見本二", "変わった説明");
        let read = |id: &str| -> (Option<String>, bool, Option<String>, bool, i64) {
            connection
                .query_row(
                    "SELECT category, category_locked, genre, genre_locked,
                       (SELECT COUNT(*) FROM product_avatars WHERE product_id = booth_id AND source = 'manual')
                     FROM products WHERE booth_id = ?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
                )
                .unwrap()
        };
        let locked = read("1001");
        assert_eq!(locked.0.as_deref(), Some("outfit"));
        assert!(locked.1);
        assert_eq!(locked.2, genre_before);
        assert_eq!(locked.4, 1);
        let genre_only = read("1002");
        assert_eq!(genre_only.0, None);
        assert_eq!(genre_only.2.as_deref(), Some("hair"));
        assert!(genre_only.3);
    }

    #[test]
    fn save_rejects_non_json_and_symlink() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save(&dir.path().join("export.txt"), "{}").is_err());
        assert!(save(&dir.path().join(".json"), "{}").is_err());
        assert!(save(&dir.path().join("missing").join("export.json"), "{}").is_err());
        #[cfg(unix)]
        {
            let target = dir.path().join("target.json");
            std::fs::write(&target, "kept").unwrap();
            let link = dir.path().join("link.json");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert_eq!(
                save(&link, "{}").unwrap_err(),
                "シンボリックリンクには保存できません"
            );
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "kept");
        }
        // A directory at the destination makes the final rename fail.
        std::fs::create_dir(dir.path().join("taken.json")).unwrap();
        assert!(save(&dir.path().join("taken.json"), "{}").is_err());
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn save_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export.json");
        std::fs::write(&path, "older and much longer content than the new body").unwrap();
        save(&path, "{}\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");
        let names = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(names, 1);
    }
}
