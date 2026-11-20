use crate::import_data::ParsedExport;
use crate::matching::{LabelScore, choose_match, contains_booth_id, score_label};
use crate::model::{
    Avatar, AvatarInput, BaseBody, BaseBodyInput, CatalogInfo, CatalogRefresh, Category,
    ClassificationSnapshot, DownloadFact, DownloadFinished, Extraction, ImportReport,
    IncomingDownload, IncomingProduct, IncomingVariation, Knowledge, LibraryStats, Link, LocalFile,
    ManagedProduct, OwnedAvatarRestore, Product, ProductFlag, ProductUpdate,
    RegisterOwnedBodyResult, Settings, SettingsInput, SettingsPatch, SetupStatus, SnapshotLink,
    VariationFact, now_iso,
};
use crate::resolve::{ResolveInput, resolve_product};
use crate::scan::ScanOutcome;
use crate::view::Snapshot;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

mod classification_log;
#[cfg(test)]
mod classification_regressions;

#[cfg(test)]
pub(crate) use classification_log::KIND_IMPLICIT;
use classification_log::observe_all;
pub(crate) use classification_log::{
    CLASSIFIER_CATALOG, CLASSIFIER_NONE, CLASSIFIER_RULES, ClassificationRun, KIND_BOOTH_DETAIL,
    KIND_CATALOG, KIND_EDIT, KIND_IMPORT, KIND_RECLASSIFY, KIND_STARTUP, observe,
};
pub(crate) use classification_log::{
    ClassificationEntryView, ClassificationRunPage, UnresolvedNameCount, run_entries, run_page,
    search_entries, undo_entry, unresolved_name_counts,
};

mod auto_register;
mod classification_edit;
mod downloads;
mod extract_jobs;
mod extraction;
mod import;
mod knowledge;
mod local_files;
mod owned_body;
mod queries;
mod schema;
mod settings;
#[cfg(test)]
mod tests;

pub(crate) use auto_register::*;
pub(crate) use classification_edit::*;
pub(crate) use downloads::*;
pub(crate) use extract_jobs::*;
pub(crate) use extraction::*;
pub(crate) use import::*;
pub(crate) use knowledge::*;
pub(crate) use local_files::*;
pub(crate) use owned_body::*;
pub(crate) use queries::*;
pub(crate) use schema::*;
pub(crate) use settings::*;

pub(crate) fn known_booth_ids(connection: &Connection) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("SELECT booth_id FROM products ORDER BY imported_at DESC")
        .sql()?;
    let ids = statement
        .query_map([], |row| row.get(0))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    Ok(ids)
}

pub(crate) fn catalog_info(connection: &Connection) -> Result<CatalogInfo, String> {
    let count = setting(connection, "catalog_count")?
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    Ok(CatalogInfo {
        count,
        fetched_on: setting(connection, "catalog_fetched")?.unwrap_or_default(),
        upstream_updated: setting(connection, "catalog_upstream")?.unwrap_or_default(),
        source_url: crate::catalog::SOURCE_URL.into(),
        terms_url: crate::catalog::TERMS_URL.into(),
        author: crate::catalog::AUTHOR.into(),
        credit: crate::catalog::CREDIT.into(),
    })
}

pub(crate) fn setup_status(connection: &Connection) -> Result<SetupStatus, String> {
    Ok(SetupStatus {
        complete: setting(connection, "setup_complete")?.as_deref() == Some("1"),
    })
}

pub(crate) fn set_setup_complete(connection: &Connection, complete: bool) -> Result<(), String> {
    set_setting(
        connection,
        "setup_complete",
        if complete { "1" } else { "0" },
    )
}

fn tokyo_date() -> String {
    let offset = chrono::FixedOffset::east_opt(9 * 3600)
        .unwrap_or(chrono::FixedOffset::east_opt(0).unwrap());
    chrono::Utc::now()
        .with_timezone(&offset)
        .format("%Y-%m-%d")
        .to_string()
}

fn decide_category(
    existing: &Product,
    proposed: Category,
    confidence: f64,
    title_fill: bool,
) -> Option<Category> {
    if existing.category_locked {
        return existing.category;
    }
    // Title phrases fill a product the BOOTH category and tags left unset.
    // A settled BOOTH label calls this with title_fill = false and stays put.
    // Manual category locks above stay ahead of both.
    if title_fill {
        if let Some(category) = crate::compat::strong_category_override(&existing.title) {
            return Some(category);
        }
        if proposed == Category::Avatar && crate::normalize::is_avatar_count_title(&existing.title)
        {
            return Some(Category::Other);
        }
        if crate::compat::world_hint_overrides(
            &existing.title,
            &existing.description_snippet,
            proposed,
            confidence,
        ) {
            return Some(Category::World);
        }
        if let Some(category) = crate::compat::title_fill_category(
            &existing.title,
            &existing.description_snippet,
            proposed,
        ) {
            return Some(category);
        }
    }
    Some(proposed)
}

/// Categories whose avatar links may come from download file names.
fn file_names_apply(category: Category) -> bool {
    matches!(
        category,
        Category::Outfit
            | Category::Hair
            | Category::Accessory
            | Category::Texture
            | Category::Gimmick
    )
}

/// Variation and download file names recorded for one product, in a stable order.
fn product_file_labels(connection: &Connection, booth_id: &str) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM product_variations WHERE booth_id = ?1
             UNION SELECT variation FROM product_files WHERE booth_id = ?1 AND variation <> ''
             UNION SELECT filename FROM product_files WHERE booth_id = ?1
             ORDER BY 1",
        )
        .sql()?;
    let rows = statement
        .query_map(params![booth_id], |row| row.get::<_, String>(0))
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn review_from_product(
    connection: &Connection,
    product: &Product,
    category: Option<Category>,
    unresolved: &[String],
    linked_ids: &[String],
    threshold: f64,
) -> Result<bool, String> {
    let avatars = load_avatars(connection)?;
    let mut view = product.clone();
    view.category = category;
    view.unresolved_names = unresolved.to_vec();
    Ok(needs_review_for(&view, linked_ids, &avatars, threshold))
}

fn needs_review_for(
    product: &Product,
    linked_ids: &[String],
    avatars: &[Avatar],
    threshold: f64,
) -> bool {
    crate::resolve::product_needs_review(&crate::resolve::ReviewFacts {
        category: product.category,
        placeholder: linked_ids.iter().any(|id| id.starts_with("owned-")),
        link_count: linked_ids.len(),
        unresolved: &product.unresolved_names,
        is_generic: product.is_generic,
        grounded: crate::resolve::links_grounded(
            linked_ids,
            avatars,
            &product.title,
            &product.description_snippet,
        ),
        user_confirmed: product.user_confirmed,
        threshold,
    })
}

fn note_unavailable(connection: &Connection, booth_id: &str) -> Result<(), String> {
    let now = now_iso();
    connection
        .execute(
            "UPDATE products SET
                category = CASE WHEN category_locked = 1 THEN category ELSE 'other' END,
                compatible_avatars_json = '[]',
                unresolved_names_json = '[]',
                is_generic = 0,
                confidence = NULL,
                needs_review = 0,
                user_confirmed = 1,
                extracted_at = COALESCE(extracted_at, ?1),
                updated_at = ?1
             WHERE booth_id = ?2",
            params![now, booth_id],
        )
        .sql()?;
    let locked: i64 = connection
        .query_row(
            "SELECT links_locked FROM products WHERE booth_id = ?1",
            params![booth_id],
            |row| row.get(0),
        )
        .sql()?;
    if locked == 0 {
        delete_links(connection, booth_id)?;
    }
    crate::sync_meta::touch_product(connection, booth_id)
}

fn close_unavailable_products(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id FROM products
             WHERE description_snippet = ?1 AND (category IS NULL OR user_confirmed = 0 OR needs_review = 1)",
        )
        .sql()?;
    let ids: Vec<String> = statement
        .query_map(params![crate::model::UNAVAILABLE_DESCRIPTION], |row| {
            row.get(0)
        })
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    drop(statement);
    for id in ids {
        note_unavailable(connection, &id)?;
    }
    Ok(())
}

fn link_ids(connection: &Connection, product_id: &str) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare("SELECT avatar_id FROM product_avatars WHERE product_id = ?1")
        .sql()?;
    let ids = statement
        .query_map(params![product_id], |row| row.get(0))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    Ok(ids)
}

fn remember_readings(connection: &Connection, readings: &[(String, String)]) -> Result<(), String> {
    for (avatar_id, alias) in readings {
        if !crate::normalize::is_name_alias(alias) {
            continue;
        }
        let avatars = load_avatars(connection)?;
        if let Some(existing) = crate::resolve::resolve_name(alias, &avatars) {
            if existing.id != *avatar_id {
                continue;
            }
            continue;
        }
        let current: String = connection
            .query_row(
                "SELECT aliases_json FROM avatars WHERE id = ?1",
                params![avatar_id],
                |row| row.get(0),
            )
            .optional()
            .sql()?
            .unwrap_or_else(|| "[]".into());
        let mut aliases = parse_list(&current);
        if aliases.iter().any(|existing| existing == alias) {
            continue;
        }
        aliases.push(alias.clone());
        connection
            .execute(
                "UPDATE avatars SET aliases_json = ?1 WHERE id = ?2",
                params![to_json(&aliases)?, avatar_id],
            )
            .sql()?;
    }
    Ok(())
}

fn insert_placeholder(
    connection: &Connection,
    id: &str,
    display_name: &str,
    aliases: &[String],
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
             VALUES (?1, ?2, NULL, ?3, 0, '')",
            params![id, display_name, to_json(aliases)?],
        )
        .sql()?;
    Ok(())
}

fn replace_links(
    connection: &Connection,
    product_id: &str,
    avatar_ids: &[String],
    source: &str,
) -> Result<(), String> {
    delete_links(connection, product_id)?;
    for avatar_id in avatar_ids {
        connection
            .execute(
                "INSERT OR IGNORE INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, ?3)",
                params![product_id, avatar_id, source],
            )
            .sql()?;
    }
    Ok(())
}

fn delete_links(connection: &Connection, product_id: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM product_avatars WHERE product_id = ?1",
            params![product_id],
        )
        .sql()?;
    Ok(())
}

fn require_product(connection: &Connection, booth_id: &str) -> Result<Product, String> {
    product_by_id(connection, booth_id)?.ok_or_else(|| "商品が見つかりません".into())
}

pub(crate) struct ProductPreviewSource {
    pub thumbnail_url: Option<String>,
    pub is_adult: bool,
}

pub(crate) fn product_preview_source(
    connection: &Connection,
    booth_id: &str,
) -> Result<Option<ProductPreviewSource>, String> {
    connection
        .query_row(
            "SELECT thumbnail_url, is_adult FROM products WHERE booth_id = ?1",
            params![booth_id],
            |row| {
                let adult: Option<i64> = row.get(1)?;
                Ok(ProductPreviewSource {
                    thumbnail_url: row.get(0)?,
                    is_adult: adult.unwrap_or(0) != 0,
                })
            },
        )
        .optional()
        .sql()
}

fn product_by_id(connection: &Connection, booth_id: &str) -> Result<Option<Product>, String> {
    connection
        .query_row(
            &format!("{PRODUCT_SELECT} WHERE booth_id = ?1"),
            params![booth_id],
            map_product,
        )
        .optional()
        .sql()
}

fn load_products(connection: &Connection) -> Result<Vec<Product>, String> {
    let mut statement = connection
        .prepare(&format!(
            "{PRODUCT_SELECT} WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)"
        ))
        .sql()?;
    let rows = statement.query_map([], map_product).sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(crate) fn load_avatars(connection: &Connection) -> Result<Vec<Avatar>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, display_name, base_body, aliases_json, is_builtin, owned, notes, hero_image_path FROM avatars",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            let aliases: String = row.get(3)?;
            Ok(Avatar {
                id: row.get(0)?,
                display_name: row.get(1)?,
                base_body: row.get(2)?,
                aliases: parse_list(&aliases),
                is_builtin: row.get::<_, i64>(4)? != 0,
                owned: row.get::<_, i64>(5)? != 0,
                notes: row.get(6)?,
                hero_image_path: row.get(7)?,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn load_bases(connection: &Connection) -> Result<Vec<BaseBody>, String> {
    let mut statement = connection
        .prepare("SELECT id, display_name, note FROM base_bodies")
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok(BaseBody {
                id: row.get(0)?,
                display_name: row.get(1)?,
                note: row.get(2)?,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn load_links(connection: &Connection) -> Result<Vec<Link>, String> {
    let mut statement = connection
        .prepare("SELECT product_id, avatar_id, source FROM product_avatars")
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok(Link {
                product_id: row.get(0)?,
                avatar_id: row.get(1)?,
                source: row.get(2)?,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn load_files(connection: &Connection) -> Result<Vec<LocalFile>, String> {
    let mut statement = connection
        .prepare(
            "SELECT path, filename, is_dir, size, linked_product_id, candidate_product_id, match_score, link_source
             FROM local_files",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            let size: i64 = row.get(3)?;
            Ok(LocalFile {
                path: row.get(0)?,
                filename: row.get(1)?,
                is_dir: row.get::<_, i64>(2)? != 0,
                size: size.max(0) as u64,
                linked_product_id: row.get(4)?,
                candidate_product_id: row.get(5)?,
                match_score: row.get(6)?,
                link_source: row.get(7)?,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

const PRODUCT_SELECT: &str = "
SELECT booth_id, title, shop, description_snippet, url, thumbnail_url, thumbnail_path,
       category, category_locked, genre, genre_locked, compatible_avatars_json, unresolved_names_json,
       is_generic, confidence, links_locked, user_confirmed, needs_review,
       imported_at, updated_at, extracted_at
FROM products
";

fn map_product(row: &rusqlite::Row<'_>) -> rusqlite::Result<Product> {
    let category: Option<String> = row.get(7)?;
    let compatible: String = row.get(11)?;
    let unresolved: String = row.get(12)?;
    Ok(Product {
        booth_id: row.get(0)?,
        title: row.get(1)?,
        shop: row.get(2)?,
        description_snippet: row.get(3)?,
        url: row.get(4)?,
        thumbnail_url: row.get(5)?,
        thumbnail_path: row.get(6)?,
        category: category.as_deref().and_then(Category::parse),
        category_locked: row.get::<_, i64>(8)? != 0,
        genre: row.get(9)?,
        genre_locked: row.get::<_, i64>(10)? != 0,
        compatible_avatars: parse_list(&compatible),
        unresolved_names: parse_list(&unresolved),
        is_generic: row.get::<_, i64>(13)? != 0,
        confidence: row.get(14)?,
        links_locked: row.get::<_, i64>(15)? != 0,
        user_confirmed: row.get::<_, i64>(16)? != 0,
        needs_review: row.get::<_, i64>(17)? != 0,
        imported_at: row.get(18)?,
        updated_at: row.get(19)?,
        extracted_at: row.get(20)?,
    })
}

fn avatar_by_id(connection: &Connection, id: &str) -> Result<Option<Avatar>, String> {
    let avatars = load_avatars(connection)?;
    Ok(avatars.into_iter().find(|avatar| avatar.id == id))
}

fn base_body_exists(connection: &Connection, id: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM base_bodies WHERE id = ?1)",
            params![id],
            |row| row.get::<_, bool>(0),
        )
        .sql()
}

fn avatar_exists(connection: &Connection, id: &str) -> Result<bool, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(count > 0)
}

fn normalize_base(connection: &Connection, value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM base_bodies WHERE id = ?1",
            params![value],
            |row| row.get(0),
        )
        .sql()?;
    if count == 0 {
        return Err("素体が見つかりません".into());
    }
    Ok(Some(value.to_string()))
}

fn clean_aliases(raw: &[String], display: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for alias in raw {
        let trimmed = alias.trim();
        if trimmed.is_empty() || trimmed == display || trimmed.chars().count() > 80 {
            continue;
        }
        if aliases.iter().any(|existing: &String| existing == trimmed) {
            continue;
        }
        aliases.push(trimmed.to_string());
        if aliases.len() == 32 {
            break;
        }
    }
    aliases
}

fn generate_id(connection: &Connection, display: &str) -> Result<String, String> {
    let mut base: String = crate::normalize::nfkc(display)
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect();
    if base.len() < 2 {
        base = format!("a{:08x}", fnv(display));
    }
    base.truncate(48);
    if !base.starts_with("owned-")
        && valid_id(&base)
        && !avatar_exists(connection, &base)?
        && !base_exists(connection, &base)?
    {
        return Ok(base);
    }
    for number in 2..500 {
        let id = format!("{base}-{number}");
        if valid_id(&id) && !avatar_exists(connection, &id)? && !base_exists(connection, &id)? {
            return Ok(id);
        }
    }
    Err("IDを作れませんでした".into())
}

fn base_exists(connection: &Connection, id: &str) -> Result<bool, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM base_bodies WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(count > 0)
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && id.len() <= 64
        && id
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-')
}

pub(crate) fn fnv(text: &str) -> u32 {
    let mut hash = 2_166_136_261u32;
    for byte in text.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    hash
}

fn to_json(values: &[String]) -> Result<String, String> {
    serde_json::to_string(values).map_err(|err| err.to_string())
}

fn parse_list(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn normalize_path_key(path: &str) -> String {
    normalize_path_key_os(path, cfg!(windows))
}

fn normalize_path_key_os(path: &str, windows: bool) -> String {
    if windows {
        path.replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    } else {
        path.trim_end_matches('/').to_string()
    }
}

fn is_under(path: &str, root: &str) -> bool {
    is_under_os(path, root, cfg!(windows))
}

fn is_under_os(path: &str, root: &str, windows: bool) -> bool {
    let path = normalize_path_key_os(path, windows);
    let root = normalize_path_key_os(root, windows);
    let separator = if windows { "\\" } else { "/" };
    path.starts_with(&(root + separator))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedFile {
    /// Highest base-body revision. Raise it when a seed adds a base-body group.
    #[serde(default = "first_seed_revision")]
    revision: u32,
    base_bodies: Vec<SeedBody>,
    avatars: Vec<SeedAvatar>,
}

fn first_seed_revision() -> u32 {
    1
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedBody {
    id: String,
    display_name: String,
    note: String,
    /// Seed revision that added this group; existing libraries receive it once.
    #[serde(default = "first_seed_revision")]
    revision: u32,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedAvatar {
    id: String,
    display_name: String,
    base_body: Option<String>,
    aliases: Vec<String>,
    /// Booth item of the avatar, matching the bundled もちふぃった～ catalog.
    #[serde(default)]
    booth_id: Option<String>,
    /// Seed revision that added this avatar. An avatar added to an older group
    /// reaches existing libraries once, like a new group.
    #[serde(default = "first_seed_revision")]
    revision: u32,
}

#[cfg(test)]
mod builtin_browser_cache_tests {
    use super::*;

    #[test]
    fn the_browser_flag_is_kept_in_memory_and_cleared_by_every_write() {
        let connection = open_memory().unwrap();
        set_setting(&connection, "builtin_browser", "1").unwrap();
        assert!(builtin_browser(&connection).unwrap());
        // Other tests may save settings at the same time, which clears the value; it never flips.
        assert_ne!(cached_builtin_browser(), Some(false));
        set_setting(&connection, "builtin_browser", "0").unwrap();
        assert!(!builtin_browser(&connection).unwrap());
        assert_ne!(cached_builtin_browser(), Some(true));
        forget_builtin_browser();
        assert_eq!(cached_builtin_browser(), None);

        // Both writers clear it before writing, while they hold the connection.
        let source = include_str!("db/settings.rs").replace("\r\n", "\n");
        let save = &source[source.find("fn save_settings_values(").unwrap()..];
        let save = &save[..save.find("\n}\n").unwrap()];
        let forget = save
            .find("forget_builtin_browser();")
            .expect("settings save clears the flag");
        let write = save
            .find("\"builtin_browser\",")
            .expect("settings save writes the flag");
        assert!(forget < write);
        let integration = include_str!("integration.rs").replace("\r\n", "\n");
        let mode = &integration[integration.find("pub fn set_mode(").unwrap()..];
        let mode = &mode[..mode.find("\n}\n").unwrap()];
        assert!(
            mode.find("forget_builtin_browser();").unwrap()
                < mode.find("\"builtin_browser\",").unwrap()
        );
        // The browser commands use the kept value.
        let browser = include_str!("browser.rs").replace("\r\n", "\n");
        let enabled = &browser[browser.find("pub(crate) fn browser_enabled(").unwrap()..];
        assert!(enabled[..enabled.find("\n}\n").unwrap()].contains("db::cached_builtin_browser()"));
    }
}
