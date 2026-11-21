//! Opt-in public metadata checks. Never downloads files or uses a BOOTH session.
use crate::model::{IncomingDownload, IncomingProduct};
use crate::{AppState, booth_detail, db};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// The switches for the whole feature: one small settings row. The per-product observations are
/// rows of `asset_watch`, so a check writes the one product it looked at.
const STATE_KEY: &str = "asset_update_state_v2";
/// Until DB version 15 everything was one JSON value under this key. With every product as a
/// target it grew to megabytes and was rewritten for each product checked.
const LEGACY_KEY: &str = "asset_update_watch_v1";
const HISTORY_LIMIT: usize = 200;
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CHECK_DONE: AtomicUsize = AtomicUsize::new(0);
static CHECK_TOTAL: AtomicUsize = AtomicUsize::new(0);

/// `baseline` and `latest` are JSON snapshots (a digest of the description and the free file
/// list). The file columns are JSON arrays. Local state, outside the sync export.
pub(crate) const TABLES: &str = "
CREATE TABLE IF NOT EXISTS asset_watch (
    booth_id TEXT PRIMARY KEY REFERENCES products(booth_id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 1,
    generation INTEGER NOT NULL DEFAULT 0,
    attempted_at INTEGER,
    checked_at TEXT,
    error TEXT,
    baseline TEXT,
    latest TEXT,
    new_files TEXT NOT NULL DEFAULT '[]',
    current_files TEXT,
    history_library_files TEXT
);
CREATE TABLE IF NOT EXISTS asset_watch_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    booth_id TEXT NOT NULL,
    detected_at TEXT NOT NULL,
    source TEXT NOT NULL,
    description_changed INTEGER NOT NULL,
    removed TEXT NOT NULL,
    added TEXT NOT NULL
);
";

const READ_ERROR: &str = "更新確認の設定を読めません";
const WRITE_ERROR: &str = "更新確認の設定を保存できません";

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    #[serde(default)]
    automatic: bool,
    #[serde(default)]
    all_products: bool,
    #[serde(default)]
    retry_after: Option<i64>,
    #[serde(default)]
    generation: u64,
}
#[derive(Clone)]
struct Row {
    booth_id: String,
    enabled: bool,
    generation: u64,
    attempted_at: Option<i64>,
    checked_at: Option<String>,
    error: Option<String>,
    baseline: Option<Snapshot>,
    latest: Option<Snapshot>,
    new_files: Vec<String>,
    current_files: Option<Vec<String>>,
    history_library_files: Option<Vec<UpdateFile>>,
}
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateFile {
    downloadable_id: Option<String>,
    variation: String,
    filename: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateRecord {
    booth_id: String,
    detected_at: String,
    source: String,
    description_changed: bool,
    removed: Vec<UpdateFile>,
    added: Vec<UpdateFile>,
}

fn update_file(id: &str, variation: &str, filename: &str) -> UpdateFile {
    UpdateFile {
        downloadable_id: ((1..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()))
            .then(|| id.to_owned()),
        variation: variation.to_owned(),
        filename: filename.to_owned(),
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|_| WRITE_ERROR.to_string())
}

fn record_delta(
    c: &Connection,
    booth_id: &str,
    source: &str,
    before: &[UpdateFile],
    after: &[UpdateFile],
    description_changed: bool,
) -> Result<bool, String> {
    let mut removed: Vec<_> = before
        .iter()
        .filter(|f| !after.contains(f))
        .cloned()
        .collect();
    let mut added: Vec<_> = after
        .iter()
        .filter(|f| !before.contains(f))
        .cloned()
        .collect();
    if removed.is_empty() && added.is_empty() && !description_changed {
        return Ok(false);
    }
    removed.sort();
    removed.dedup();
    added.sort();
    added.dedup();
    push_history(
        c,
        &UpdateRecord {
            booth_id: booth_id.to_owned(),
            detected_at: crate::model::now_iso(),
            source: source.to_owned(),
            description_changed,
            removed,
            added,
        },
    )?;
    Ok(true)
}

/// Keeps the newest `HISTORY_LIMIT` records.
fn push_history(c: &Connection, record: &UpdateRecord) -> Result<(), String> {
    c.execute(
        "INSERT INTO asset_watch_history (booth_id, detected_at, source, description_changed, removed, added)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            record.booth_id,
            record.detected_at,
            record.source,
            record.description_changed,
            to_json(&record.removed)?,
            to_json(&record.added)?,
        ],
    )
    .map_err(|_| WRITE_ERROR.to_string())?;
    c.execute(
        "DELETE FROM asset_watch_history WHERE id <= (
             SELECT id FROM asset_watch_history ORDER BY id DESC LIMIT 1 OFFSET ?1
         )",
        params![HISTORY_LIMIT as i64],
    )
    .map_err(|_| WRITE_ERROR.to_string())?;
    Ok(())
}

pub(crate) fn history(c: &Connection) -> Result<Vec<UpdateRecord>, String> {
    let titles = products(c)?;
    let mut q = c
        .prepare(
            "SELECT booth_id, detected_at, source, description_changed, removed, added
             FROM asset_watch_history ORDER BY id DESC",
        )
        .map_err(|_| READ_ERROR.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok(UpdateRecord {
                booth_id: r.get(0)?,
                detected_at: r.get(1)?,
                source: r.get(2)?,
                description_changed: r.get(3)?,
                removed: json_column(r.get(4)?).unwrap_or_default(),
                added: json_column(r.get(5)?).unwrap_or_default(),
            })
        })
        .map_err(|_| READ_ERROR.to_string())?;
    let mut records = Vec::new();
    for row in rows {
        let record = row.map_err(|_| READ_ERROR.to_string())?;
        if titles.contains_key(&record.booth_id) {
            records.push(record);
        }
    }
    Ok(records)
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    // A digest, not the text: the description is only compared with the one before. Empty for
    // adult or unknown-age items. No tags, URL, cookies, prices, popularity, or signed links.
    description_digest: String,
    files: Vec<(String, String, String)>,
}

/// A stable digest of the compared description (FNV-1a, 64 bit). It must give the same value in
/// every version of the app, because stored baselines are compared with later observations.
fn description_digest(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

struct FetchedObservation {
    snapshot: Snapshot,
    free_files: Vec<IncomingDownload>,
    is_free: Option<bool>,
    is_adult: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProductFileWatch {
    pub new_files: HashSet<String>,
    pub current_files: Option<HashSet<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    automatic: bool,
    all_products: bool,
    total_products: usize,
    running: bool,
    progress: Option<CheckProgress>,
    rows: Vec<RowStatus>,
}
#[derive(Serialize)]
struct CheckProgress {
    done: usize,
    total: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RowStatus {
    booth_id: String,
    title: String,
    enabled: bool,
    checked_at: Option<String>,
    error: Option<String>,
    changes: Vec<String>,
    revision: u64,
    candidate_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configure {
    automatic: Option<bool>,
    all_products: Option<bool>,
    booth_id: Option<String>,
    enabled: Option<bool>,
    acknowledge: Option<bool>,
}
fn products(c: &Connection) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut q = c.prepare("SELECT p.booth_id, p.title FROM products p WHERE NOT EXISTS (SELECT 1 FROM product_exclusions e WHERE e.booth_id = p.booth_id) ORDER BY p.booth_id")
        .map_err(|_| "商品を確認できません")?;
    let rows = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|_| "商品を確認できません")?;
    let mut result = std::collections::BTreeMap::new();
    for row in rows {
        let (id, title) = row.map_err(|_| "商品を確認できません")?;
        if booth_detail::item_json_url(&id).is_ok() {
            result.insert(id, title);
        }
    }
    Ok(result)
}
fn new_row(id: String) -> Row {
    Row {
        booth_id: id,
        enabled: true,
        generation: 0,
        attempted_at: None,
        checked_at: None,
        error: None,
        baseline: None,
        latest: None,
        new_files: Vec::new(),
        current_files: None,
        history_library_files: None,
    }
}

fn load_meta(c: &Connection) -> Result<Meta, String> {
    match db::setting(c, STATE_KEY)? {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| READ_ERROR.to_string()),
        None => Ok(Meta::default()),
    }
}
fn save_meta(c: &Connection, meta: &Meta) -> Result<(), String> {
    db::set_setting(c, STATE_KEY, &to_json(meta)?)
}

const ROW_COLUMNS: &str = "booth_id, enabled, generation, attempted_at, checked_at, error, baseline, latest, new_files, current_files, history_library_files";

/// A JSON column. A value that cannot be read counts as absent; the next check writes it again.
fn json_column<T: serde::de::DeserializeOwned>(raw: Option<String>) -> Option<T> {
    raw.and_then(|raw| serde_json::from_str(&raw).ok())
}
fn read_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        booth_id: r.get(0)?,
        enabled: r.get(1)?,
        generation: r.get::<_, i64>(2)?.max(0) as u64,
        attempted_at: r.get(3)?,
        checked_at: r.get(4)?,
        error: r.get(5)?,
        baseline: json_column(r.get(6)?),
        latest: json_column(r.get(7)?),
        new_files: json_column(r.get(8)?).unwrap_or_default(),
        current_files: json_column(r.get(9)?),
        history_library_files: json_column(r.get(10)?),
    })
}
/// The rows that were written, oldest first.
fn stored_rows(c: &Connection) -> Result<Vec<Row>, String> {
    let mut q = c
        .prepare(&format!(
            "SELECT {ROW_COLUMNS} FROM asset_watch ORDER BY rowid"
        ))
        .map_err(|_| READ_ERROR.to_string())?;
    let rows = q
        .query_map([], read_row)
        .map_err(|_| READ_ERROR.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| READ_ERROR.to_string())
}
fn stored_row(c: &Connection, id: &str) -> Result<Option<Row>, String> {
    c.query_row(
        &format!("SELECT {ROW_COLUMNS} FROM asset_watch WHERE booth_id = ?1"),
        [id],
        read_row,
    )
    .optional()
    .map_err(|_| READ_ERROR.to_string())
}
/// Every target. Whole-library consent includes later imports: a product without a written row
/// is a target with the defaults, and existing disabled rows remain exceptions.
fn target_rows(c: &Connection, meta: &Meta) -> Result<Vec<Row>, String> {
    let mut rows = stored_rows(c)?;
    if meta.all_products {
        let known: HashSet<String> = rows.iter().map(|r| r.booth_id.clone()).collect();
        for id in products(c)?.into_keys() {
            if !known.contains(&id) {
                rows.push(new_row(id));
            }
        }
    }
    Ok(rows)
}
fn target_row(c: &Connection, meta: &Meta, id: &str) -> Result<Option<Row>, String> {
    if let Some(row) = stored_row(c, id)? {
        return Ok(Some(row));
    }
    if meta.all_products && booth_detail::item_json_url(id).is_ok() && active(c, id)? {
        return Ok(Some(new_row(id.to_owned())));
    }
    Ok(None)
}
fn save_row(c: &Connection, row: &Row) -> Result<(), String> {
    let optional = |value: &Option<Snapshot>| value.as_ref().map(to_json).transpose();
    c.execute(
        "INSERT INTO asset_watch (booth_id, enabled, generation, attempted_at, checked_at, error, baseline, latest, new_files, current_files, history_library_files)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(booth_id) DO UPDATE SET
             enabled = excluded.enabled,
             generation = excluded.generation,
             attempted_at = excluded.attempted_at,
             checked_at = excluded.checked_at,
             error = excluded.error,
             baseline = excluded.baseline,
             latest = excluded.latest,
             new_files = excluded.new_files,
             current_files = excluded.current_files,
             history_library_files = excluded.history_library_files",
        params![
            row.booth_id,
            row.enabled,
            row.generation as i64,
            row.attempted_at,
            row.checked_at,
            row.error,
            optional(&row.baseline)?,
            optional(&row.latest)?,
            to_json(&row.new_files)?,
            row.current_files.as_ref().map(to_json).transpose()?,
            row.history_library_files.as_ref().map(to_json).transpose()?,
        ],
    )
    .map_err(|_| WRITE_ERROR.to_string())?;
    Ok(())
}
/// Runs several writes as one commit. Inside a caller's transaction it just runs them.
fn in_one_commit<T>(
    c: &Connection,
    work: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    if !c.is_autocommit() {
        return work(c);
    }
    let tx = c
        .unchecked_transaction()
        .map_err(|_| WRITE_ERROR.to_string())?;
    let value = work(&tx)?;
    tx.commit().map_err(|_| WRITE_ERROR.to_string())?;
    Ok(value)
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyWatch {
    automatic: bool,
    all_products: bool,
    retry_after: Option<i64>,
    generation: u64,
    rows: Vec<LegacyRow>,
    history: Vec<UpdateRecord>,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LegacyRow {
    booth_id: String,
    enabled: bool,
    generation: u64,
    attempted_at: Option<i64>,
    checked_at: Option<String>,
    error: Option<String>,
    baseline: Option<LegacySnapshot>,
    latest: Option<LegacySnapshot>,
    new_files: Vec<String>,
    current_files: Option<Vec<String>>,
    history_library_files: Option<Vec<UpdateFile>>,
}
#[derive(Deserialize)]
struct LegacySnapshot {
    description: String,
    files: Vec<(String, String, String)>,
}

/// Moves the single JSON value of older versions into the tables (DB version 15). The stored
/// descriptions become digests of the same text, so a baseline still matches an unchanged page.
/// A value that cannot be read is left where it is and the feature starts empty, as a library
/// must still open. Rows of products that are gone are dropped.
pub(crate) fn move_legacy_watch(c: &Connection) -> Result<(), String> {
    let Some(raw) = db::setting(c, LEGACY_KEY)? else {
        return Ok(());
    };
    let Ok(legacy) = serde_json::from_str::<LegacyWatch>(&raw) else {
        return Ok(());
    };
    let snapshot = |old: Option<LegacySnapshot>| {
        old.map(|old| Snapshot {
            description_digest: description_digest(&old.description),
            files: old.files,
        })
    };
    for old in legacy.rows {
        let exists: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM products WHERE booth_id = ?1)",
                [&old.booth_id],
                |r| r.get(0),
            )
            .map_err(|_| READ_ERROR.to_string())?;
        if !exists {
            continue;
        }
        save_row(
            c,
            &Row {
                booth_id: old.booth_id,
                enabled: old.enabled,
                generation: old.generation,
                attempted_at: old.attempted_at,
                checked_at: old.checked_at,
                error: old.error,
                baseline: snapshot(old.baseline),
                latest: snapshot(old.latest),
                new_files: old.new_files,
                current_files: old.current_files,
                history_library_files: old.history_library_files,
            },
        )?;
    }
    for record in &legacy.history {
        push_history(c, record)?;
    }
    save_meta(
        c,
        &Meta {
            automatic: legacy.automatic,
            all_products: legacy.all_products,
            retry_after: legacy.retry_after,
            generation: legacy.generation,
        },
    )?;
    c.execute("DELETE FROM settings WHERE key = ?1", [LEGACY_KEY])
        .map_err(|_| WRITE_ERROR.to_string())?;
    Ok(())
}

fn active(c: &Connection, id: &str) -> Result<bool, String> {
    let exists = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM products WHERE booth_id = ?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )
        .map_err(|_| "商品を確認できません")?;
    Ok(exists && !db::product_excluded(c, id)?)
}
fn changes(row: &Row) -> Vec<String> {
    let mut changes = Vec::new();
    if !row.new_files.is_empty() {
        changes.push("新しいファイルの追加".into());
    }
    if let (Some(before), Some(after)) = (&row.baseline, &row.latest) {
        if before.description_digest != after.description_digest {
            changes.push("商品説明の変更".into());
        }
        if before.files != after.files {
            changes.push("無料ファイル一覧の変更".into());
        }
    }
    changes
}

fn candidate_key(row: &Row) -> Option<String> {
    use std::hash::{Hash, Hasher};
    if changes(row).is_empty() {
        return None;
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut files = row.new_files.clone();
    files.sort();
    files.dedup();
    files.hash(&mut hash);
    if let (Some(before), Some(after)) = (&row.baseline, &row.latest) {
        if before.description_digest != after.description_digest {
            (&before.description_digest, &after.description_digest).hash(&mut hash);
        }
        if before.files != after.files {
            (&before.files, &after.files).hash(&mut hash);
        }
    }
    Some(format!("{:016x}", hash.finish()))
}

/// New and current files of watched products, for the product cards. Every list the screens
/// load reads this, so it reads only the rows that have either.
pub(crate) fn file_watches(c: &Connection) -> Result<HashMap<String, ProductFileWatch>, String> {
    let mut q = c
        .prepare(
            "SELECT booth_id, new_files, current_files FROM asset_watch
             WHERE new_files <> '[]' OR current_files IS NOT NULL",
        )
        .map_err(|_| READ_ERROR.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|_| READ_ERROR.to_string())?;
    let mut map = HashMap::new();
    for row in rows {
        let (booth_id, new_files, current_files) = row.map_err(|_| READ_ERROR.to_string())?;
        let new_files: Vec<String> = json_column(new_files).unwrap_or_default();
        let current_files: Option<Vec<String>> = json_column(current_files);
        if new_files.is_empty() && current_files.is_none() {
            continue;
        }
        map.insert(
            booth_id,
            ProductFileWatch {
                new_files: new_files.into_iter().collect(),
                current_files: current_files.map(|files| files.into_iter().collect()),
            },
        );
    }
    Ok(map)
}

fn clean_file_key(download: &IncomingDownload) -> Option<String> {
    let id = download.downloadable_id.trim();
    if (1..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()) {
        return Some(id.to_string());
    }
    let name = download.filename.trim();
    if !name.is_empty() {
        return Some(name.to_string());
    }
    None
}

pub(crate) fn existing_file_keys(
    c: &Connection,
    booth_id: &str,
) -> Result<HashSet<String>, String> {
    let mut stmt = c
        .prepare("SELECT downloadable_id, filename FROM product_files WHERE booth_id = ?1")
        .map_err(|_| "ファイル情報を確認できません")?;
    let rows = stmt
        .query_map([booth_id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|_| "ファイル情報を確認できません")?;
    let mut keys = HashSet::new();
    for row in rows {
        let (id, name) = row.map_err(|_| "ファイル情報を確認できません")?;
        if let Some(id) = id.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            keys.insert(id);
        }
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            keys.insert(trimmed.to_string());
        }
    }
    Ok(keys)
}

/// Records newly discovered files for products that already had file rows before this import.
/// When `authoritative` is true (e.g. a completed full library walk), also updates `current_files`
/// so files no longer listed on BOOTH can be folded under "以前のファイル".
pub(crate) fn record_imported_files(
    c: &Connection,
    before_files: &HashMap<String, HashSet<String>>,
    products: &[IncomingProduct],
    authoritative: bool,
    public_files: bool,
) -> Result<(), String> {
    // One commit for the whole import: a full library read touches every product.
    in_one_commit(c, |c| {
        for product in products {
            record_imported_product(c, before_files, product, authoritative, public_files)?;
        }
        Ok(())
    })
}

fn record_imported_product(
    c: &Connection,
    before_files: &HashMap<String, HashSet<String>>,
    product: &IncomingProduct,
    authoritative: bool,
    public_files: bool,
) -> Result<(), String> {
    if booth_detail::item_json_url(&product.booth_id).is_err() || !active(c, &product.booth_id)? {
        return Ok(());
    }
    let incoming_keys: Vec<String> = product
        .downloads
        .iter()
        .filter_map(clean_file_key)
        .collect();
    if incoming_keys.is_empty() {
        return Ok(());
    }
    let had_before = before_files
        .get(&product.booth_id)
        .is_some_and(|keys| !keys.is_empty());
    let added: Vec<String> = if let Some(prev) = before_files.get(&product.booth_id) {
        if prev.is_empty() {
            Vec::new()
        } else {
            incoming_keys
                .iter()
                .filter(|key| !prev.contains(*key))
                .cloned()
                .collect()
        }
    } else {
        Vec::new()
    };
    if !had_before && !authoritative {
        return Ok(());
    }
    let mut row =
        stored_row(c, &product.booth_id)?.unwrap_or_else(|| new_row(product.booth_id.clone()));
    let mut dirty = false;
    let mut row_changed = false;
    let mut newly_marked = Vec::new();
    for key in &added {
        if !row.new_files.contains(key) {
            row.new_files.push(key.clone());
            newly_marked.push(key.clone());
            row_changed = true;
        }
    }
    if authoritative {
        let mut sorted = incoming_keys.clone();
        sorted.sort();
        sorted.dedup();
        if row.current_files.as_ref() != Some(&sorted) {
            row.current_files = Some(sorted);
            row_changed = true;
        }
    }
    if row_changed {
        row.generation += 1;
        row.checked_at = Some(crate::model::now_iso());
        dirty = true;
    }
    if !public_files {
        let refs = |only_new: bool| -> Vec<UpdateFile> {
            product
                .downloads
                .iter()
                .filter(|f| {
                    !only_new || clean_file_key(f).is_some_and(|key| newly_marked.contains(&key))
                })
                .map(|f| update_file(f.downloadable_id.trim(), &f.variation, &f.filename))
                .collect()
        };
        let previous = row.history_library_files.clone();
        let current = if authoritative {
            Some(refs(false))
        } else {
            None
        };
        if let Some(current) = &current
            && previous.as_ref() != Some(current)
        {
            row.history_library_files = Some(current.clone());
            dirty = true;
        }
        let (before, after) = match (previous, current) {
            (Some(before), Some(after)) => (before, after),
            _ => (Vec::new(), refs(true)),
        };
        record_delta(c, &product.booth_id, "library", &before, &after, false)?;
    }
    if dirty {
        save_row(c, &row)?;
    }
    Ok(())
}

pub(crate) fn clear_new_file_on_download(
    c: &Connection,
    booth_id: &str,
    downloadable_id: Option<&str>,
    filename: &str,
) -> Result<(), String> {
    let Some(mut row) = stored_row(c, booth_id)? else {
        return Ok(());
    };
    let before = row.new_files.len();
    let id = downloadable_id.map(str::trim).unwrap_or("");
    let name = filename.trim();
    row.new_files.retain(|key| key != id && key != name);
    if row.new_files.len() != before {
        row.generation += 1;
        save_row(c, &row)?;
    }
    Ok(())
}
pub fn status(c: &Connection) -> Result<Status, String> {
    let meta = load_meta(c)?;
    let titles = products(c)?;
    let mut rows = Vec::new();
    for row in target_rows(c, &meta)? {
        let Some(title) = titles.get(&row.booth_id).cloned() else {
            continue;
        };
        rows.push(RowStatus {
            changes: changes(&row),
            candidate_key: candidate_key(&row),
            revision: row.generation,
            booth_id: row.booth_id,
            title,
            enabled: row.enabled,
            checked_at: row.checked_at,
            error: row.error,
        });
    }
    let running = RUNNING.try_lock().is_err();
    let total = CHECK_TOTAL.load(Ordering::Relaxed);
    Ok(Status {
        automatic: meta.automatic,
        all_products: meta.all_products,
        total_products: titles.len(),
        running,
        progress: (running && total > 0).then(|| CheckProgress {
            done: CHECK_DONE.load(Ordering::Relaxed).min(total),
            total,
        }),
        rows,
    })
}
pub fn configure(c: &mut Connection, input: Configure) -> Result<Status, String> {
    if input.all_products.is_some()
        && (input.booth_id.is_some() || input.enabled.is_some() || input.acknowledge.is_some())
    {
        return Err("全体設定と商品ごとの設定は別々に変更してください".into());
    }
    if let Some(id) = &input.booth_id {
        booth_detail::item_json_url(id)?;
        if !active(c, id)? {
            return Err("登録済みの商品を選んでください".into());
        }
    } else if input.enabled.is_some() || input.acknowledge.is_some() {
        return Err("商品を選んでください".into());
    }
    in_one_commit(c, |c| {
        let mut meta = load_meta(c)?;
        if let Some(all) = input.all_products {
            // Turning it on enrolls every product. Turning it off writes the products that were
            // targets only through the whole-library consent, so they stay listed as revoked.
            if all || meta.all_products {
                let known: HashSet<String> =
                    stored_rows(c)?.into_iter().map(|r| r.booth_id).collect();
                for id in products(c)?.into_keys() {
                    if !known.contains(&id) {
                        save_row(c, &new_row(id))?;
                    }
                }
            }
            meta.all_products = all;
            meta.generation += 1;
            c.execute(
                "UPDATE asset_watch SET enabled = ?1, generation = generation + 1",
                params![all],
            )
            .map_err(|_| WRITE_ERROR.to_string())?;
        }
        if let Some(id) = &input.booth_id {
            if let Some(mut row) = target_row(c, &meta, id)? {
                row.generation += 1;
                if let Some(enabled) = input.enabled {
                    row.enabled = enabled;
                }
                if input.acknowledge == Some(true) {
                    row.baseline = row.latest.clone();
                    row.new_files.clear();
                }
                save_row(c, &row)?;
            } else if input.enabled == Some(true) {
                save_row(c, &new_row(id.clone()))?;
            }
        }
        if let Some(automatic) = input.automatic {
            meta.automatic = automatic;
            meta.generation += 1;
        }
        save_meta(c, &meta)
    })?;
    status(c)
}
fn interpret_observation(id: &str, body: &str) -> Result<FetchedObservation, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "商品情報を読めません")?;
    let actual_id = v.get("id").map(|id| {
        id.as_str()
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string())
    });
    if actual_id.as_deref() != Some(id)
        || !v.get("variations").is_some_and(serde_json::Value::is_array)
    {
        return Err("商品情報の形式を確認できません".into());
    }
    let description: String =
        if v.get("is_adult").and_then(serde_json::Value::as_bool) == Some(false) {
            v.get("description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(8000)
                .collect()
        } else {
            String::new()
        };
    let (mut files, free_files, is_free, is_adult) = match crate::free_files::interpret(body) {
        crate::free_files::FreeFilesOutcome::Free {
            files,
            is_free,
            is_adult,
        } => {
            let tuples = files
                .iter()
                .map(|f| {
                    (
                        f.downloadable_id.clone(),
                        f.variation.clone(),
                        f.filename.clone(),
                    )
                })
                .collect();
            (tuples, files, is_free, is_adult)
        }
        crate::free_files::FreeFilesOutcome::NotFree
        | crate::free_files::FreeFilesOutcome::NoFiles => (Vec::new(), Vec::new(), None, None),
        _ => return Err("ファイル一覧を確認できません".into()),
    };
    files.sort();
    Ok(FetchedObservation {
        snapshot: Snapshot {
            description_digest: description_digest(&description),
            files,
        },
        free_files,
        is_free,
        is_adult,
    })
}
#[cfg(test)]
fn interpret(id: &str, body: &str) -> Result<Snapshot, String> {
    interpret_observation(id, body).map(|obs| obs.snapshot)
}
async fn fetch(id: &str) -> Result<FetchedObservation, String> {
    use booth_detail::ItemDocument;
    match booth_detail::fetch_item_document(id).await?.1 {
        ItemDocument::Body {
            status,
            final_url,
            content_type,
            body,
        } => {
            if let Some(gate) = booth_detail::gate_http(status, &final_url, &content_type, &body) {
                return Err(match gate {
                    booth_detail::DetailOutcome::Missing => "商品が公開されていません",
                    booth_detail::DetailOutcome::RetryLater => {
                        "BOOTH が混み合っています。時間を空けて確認します"
                    }
                    booth_detail::DetailOutcome::AgeGate => "ログインまたは年齢確認が必要です",
                    _ => "商品情報を確認できません",
                }
                .into());
            }
            interpret_observation(id, &body)
        }
        ItemDocument::AgeGate => Err("ログインまたは年齢確認が必要です".into()),
        ItemDocument::Again => Err("商品情報を確認できません".into()),
    }
}
fn due(row: &Row, now: i64) -> bool {
    row.attempted_at.is_none_or(|last| {
        now.saturating_sub(last) >= if row.error.is_some() { 3600 } else { 86400 }
    })
}
// A consent change invalidates an in-flight result, including off -> on changes.
fn allowed(meta: &Meta, row: &Row, generation: u64, manual: bool) -> bool {
    row.enabled && (manual || meta.automatic) && meta.generation == generation
}
fn record(row: &mut Row, result: Result<Snapshot, String>) {
    match result {
        Ok(snapshot) => {
            if row.baseline.is_none() {
                row.baseline = Some(snapshot.clone());
            }
            row.latest = Some(snapshot);
            row.checked_at = Some(crate::model::now_iso());
            row.error = None;
        }
        Err(err) => row.error = Some(err), // Keep the last successful observation and candidate.
    }
}
pub async fn check(app: &AppHandle, manual: bool) -> Result<Status, String> {
    let state = app.state::<AppState>();
    if !manual {
        let c = state.db.lock().map_err(|_| "設定を読めません")?;
        let meta = load_meta(&c)?;
        let now = chrono::Utc::now().timestamp();
        if !meta.automatic
            || meta.retry_after.is_some_and(|until| now < until)
            || !target_rows(&c, &meta)?
                .iter()
                .any(|r| r.enabled && due(r, now) && active(&c, &r.booth_id).unwrap_or(false))
        {
            return status(&c);
        }
    }
    let guard = RUNNING
        .try_lock()
        .map_err(|_| "更新確認はすでに実行中です")?;
    CHECK_DONE.store(0, Ordering::Relaxed);
    CHECK_TOTAL.store(0, Ordering::Relaxed);
    let _ = app.emit_to("main", "asset-updates-changed", ());
    let result = check_inner(app, manual).await;
    drop(guard);
    // Notify after releasing the lock on every exit, including library-read errors.
    let _ = app.emit_to("main", "asset-updates-changed", ());
    result?;
    let c = state.db.lock().map_err(|_| "設定を読めません")?;
    status(&c)
}

async fn check_inner(app: &AppHandle, manual: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    if manual {
        let use_browser = {
            let c = state.db.lock().map_err(|_| "設定を読めません")?;
            let meta = load_meta(&c)?;
            db::settings(&c)?.builtin_browser && target_rows(&c, &meta)?.iter().any(|r| r.enabled)
        };
        if use_browser {
            let walk = crate::browser::import_library(app.clone()).await?;
            let _ = app.emit("library-changed", ());
            let _ = app.emit_to("main", "asset-updates-changed", ());
            if walk.cancelled || !walk.notice.is_empty() {
                if !walk.notice.is_empty() {
                    return Err(walk.notice);
                }
                return Ok(());
            }
        }
    }
    let (ids, generation) = {
        let c = state.db.lock().map_err(|_| "設定を読めません")?;
        let meta = load_meta(&c)?;
        let now = chrono::Utc::now().timestamp();
        (
            target_rows(&c, &meta)?
                .iter()
                .filter(|r| {
                    (manual || meta.retry_after.is_none_or(|until| now >= until))
                        && r.enabled
                        && (manual || (meta.automatic && due(r, now)))
                })
                .map(|r| r.booth_id.clone())
                .collect::<Vec<_>>(),
            meta.generation,
        )
    };
    CHECK_TOTAL.store(ids.len(), Ordering::Relaxed);
    let _ = app.emit_to("main", "asset-updates-changed", ());
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let revision = {
            let c = state.db.lock().map_err(|_| "設定を読めません")?;
            let meta = load_meta(&c)?;
            let Some(mut row) = target_row(&c, &meta, id)? else {
                continue;
            };
            if !allowed(&meta, &row, generation, manual) {
                if meta.generation != generation || (!manual && !meta.automatic) {
                    break;
                }
                continue;
            }
            if !active(&c, id)? || (!manual && !due(&row, chrono::Utc::now().timestamp())) {
                continue;
            }
            // Persist the attempt before I/O so restarts cannot hammer a failing endpoint.
            row.attempted_at = Some(chrono::Utc::now().timestamp());
            save_row(&c, &row)?;
            row.generation
        };
        let result = fetch(id).await;
        let stop = result.as_ref().err().is_some_and(|e| {
            e.contains("BOOTH が") || e.contains("ログイン") || e == "商品情報を確認できません"
        });
        let mut files_updated = false;
        {
            let mut c = state.db.lock().map_err(|_| "設定を読めません")?;
            let meta = load_meta(&c)?;
            let current = target_row(&c, &meta, id)?;
            if current.as_ref().is_some_and(|row| {
                allowed(&meta, row, generation, manual) && row.generation == revision
            }) && active(&c, id)?
            {
                if let Ok(obs) = &result
                    && !obs.free_files.is_empty()
                {
                    let _ = db::store_free_files(
                        &mut c,
                        id,
                        &obs.free_files,
                        obs.is_free,
                        obs.is_adult,
                    );
                    files_updated = true;
                }
                // Storing the free files can write this row (new files), so read it again.
                let mut meta = load_meta(&c)?;
                if let Some(mut row) = target_row(&c, &meta, id)? {
                    let previous = row.latest.clone();
                    record(&mut row, result.map(|obs| obs.snapshot));
                    in_one_commit(&c, |c| {
                        if row.error.is_none()
                            && let (Some(before), Some(after)) = (&previous, row.latest.as_ref())
                        {
                            let refs = |s: &Snapshot| {
                                s.files
                                    .iter()
                                    .map(|(id, v, name)| update_file(id, v, name))
                                    .collect::<Vec<_>>()
                            };
                            record_delta(
                                c,
                                id,
                                "public",
                                &refs(before),
                                &refs(after),
                                before.description_digest != after.description_digest,
                            )?;
                        }
                        if stop {
                            meta.retry_after = Some(chrono::Utc::now().timestamp() + 3600);
                            save_meta(c, &meta)?;
                        }
                        save_row(c, &row)
                    })?;
                }
            }
        }
        if files_updated {
            let _ = app.emit("library-changed", ());
        }
        CHECK_DONE.store(index + 1, Ordering::Relaxed);
        let _ = app.emit_to("main", "asset-updates-changed", ());
        // A gate or rate limit ends the batch; do not continue requesting other products.
        if stop {
            break;
        }
    }
    Ok(())
}
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let _ = check(&app, false).await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row() -> Row {
        Row {
            booth_id: "424242".into(),
            enabled: true,
            generation: 0,
            attempted_at: None,
            checked_at: None,
            error: None,
            baseline: None,
            latest: None,
            new_files: Vec::new(),
            current_files: None,
            history_library_files: None,
        }
    }
    fn snapshot(text: &str) -> Snapshot {
        Snapshot {
            description_digest: text.into(),
            files: vec![],
        }
    }
    #[test]
    fn first_observation_is_baseline_and_candidates_wait_for_acknowledgement() {
        let mut r = row();
        record(&mut r, Ok(snapshot("v1")));
        assert!(changes(&r).is_empty());
        record(&mut r, Ok(snapshot("v2")));
        assert_eq!(changes(&r), vec!["商品説明の変更"]);
        record(&mut r, Ok(snapshot("v2")));
        assert_eq!(changes(&r).len(), 1);
        record(&mut r, Err("通信失敗".into()));
        assert_eq!(changes(&r).len(), 1);
        assert_eq!(r.error.as_deref(), Some("通信失敗"));
        r.baseline = r.latest.clone();
        assert!(changes(&r).is_empty());
    }
    #[test]
    fn consent_revocation_and_reenable_reject_old_results() {
        let mut w = Meta::default();
        let mut r = row();
        assert!(!allowed(&w, &r, 0, false));
        assert!(allowed(&w, &r, 0, true));
        w.automatic = true;
        assert!(allowed(&w, &r, 0, false));
        w.generation += 2;
        assert!(!allowed(&w, &r, 0, false));
        r.enabled = false;
        assert!(!allowed(&w, &r, 2, true));
    }
    #[test]
    fn attempts_are_daily_and_errors_wait_an_hour() {
        let mut r = row();
        assert!(due(&r, 100));
        r.attempted_at = Some(100);
        assert!(!due(&r, 86499));
        assert!(due(&r, 86500));
        r.error = Some("失敗".into());
        assert!(!due(&r, 3699));
        assert!(due(&r, 3700));
        assert!(!due(&r, 0));
    }
    #[test]
    fn paid_prices_and_popularity_do_not_signal_a_file_update() {
        let a = r#"{"id":424242,"is_adult":false,"description":"v1  info","price":500,"wish_lists_count":1,"variations":[{"status":"on_sale","price":500}]}"#;
        let b = r#"{"id":424242,"is_adult":false,"description":"v1\ninfo","price":300,"wish_lists_count":5,"variations":[{"status":"on_sale","price":300}]}"#;
        assert_eq!(
            interpret("424242", a).unwrap(),
            interpret("424242", b).unwrap()
        );
        assert!(interpret("999999", a).is_err());
        assert!(interpret("424242", r#"{"id":424242}"#).is_err());
    }
    #[test]
    fn free_file_order_is_stable_and_new_names_are_candidates() {
        let a = r#"{"id":424242,"is_adult":false,"variations":[{"status":"free_download","downloadable":{"no_musics":[{"name":"v1.zip","url":"https://booth.pm/downloadables/9400001?a=1"},{"name":"readme.txt","url":"https://booth.pm/downloadables/9400002"}]}}]}"#;
        let b = a.replace("v1.zip", "v2.zip");
        let mut r = row();
        record(&mut r, Ok(interpret("424242", a).unwrap()));
        record(
            &mut r,
            Ok(interpret("424242", &a.replace("?a=1", "?a=2")).unwrap()),
        );
        assert!(changes(&r).is_empty());
        record(&mut r, Ok(interpret("424242", &b).unwrap()));
        assert_eq!(changes(&r), vec!["無料ファイル一覧の変更"]);
    }
    #[test]
    fn adult_and_unknown_age_descriptions_are_not_retained() {
        for flag in ["true", "null"] {
            let raw = format!(
                r#"{{"id":424242,"is_adult":{flag},"description":"保存しない本文","variations":[]}}"#
            );
            assert!(
                interpret("424242", &raw)
                    .unwrap()
                    .description_digest
                    .is_empty()
            );
        }
    }
    #[test]
    fn watch_rows_persist_without_touching_other_settings() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE products(booth_id TEXT PRIMARY KEY,title TEXT); CREATE TABLE product_exclusions(booth_id TEXT PRIMARY KEY,excluded_at TEXT); INSERT INTO products VALUES('424242','ダミー'); INSERT INTO settings VALUES('library_folder','saved'); PRAGMA user_version=7;").unwrap();
        c.execute_batch(TABLES).unwrap();
        assert!(!status(&c).unwrap().automatic);
        configure(
            &mut c,
            Configure {
                automatic: Some(true),
                all_products: None,
                booth_id: Some("424242".into()),
                enabled: Some(true),
                acknowledge: None,
            },
        )
        .unwrap();
        let mut row = stored_row(&c, "424242").unwrap().unwrap();
        record(&mut row, Ok(snapshot("v1")));
        record(&mut row, Ok(snapshot("v2")));
        save_row(&c, &row).unwrap();
        let saved = status(&c).unwrap();
        assert!(saved.automatic);
        assert_eq!(saved.rows[0].changes.len(), 1);
        configure(
            &mut c,
            Configure {
                automatic: None,
                all_products: None,
                booth_id: Some("424242".into()),
                enabled: None,
                acknowledge: Some(true),
            },
        )
        .unwrap();
        assert!(status(&c).unwrap().rows[0].changes.is_empty());
        assert_eq!(
            db::setting(&c, "library_folder").unwrap().as_deref(),
            Some("saved")
        );
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        c.execute("INSERT INTO product_exclusions VALUES('424242','t')", [])
            .unwrap();
        assert!(status(&c).unwrap().rows.is_empty());
        assert!(!active(&c, "424242").unwrap());
    }
    fn library() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE products(booth_id TEXT PRIMARY KEY,title TEXT); CREATE TABLE product_exclusions(booth_id TEXT PRIMARY KEY,excluded_at TEXT); INSERT INTO products VALUES('424242','既存商品'),('434343','除外商品'); INSERT INTO product_exclusions VALUES('434343','t');").unwrap();
        c.execute_batch(TABLES).unwrap();
        c
    }
    fn config(all: Option<bool>, id: Option<&str>, enabled: Option<bool>) -> Configure {
        Configure {
            all_products: all,
            automatic: None,
            booth_id: id.map(str::to_string),
            enabled,
            acknowledge: None,
        }
    }
    #[test]
    fn whole_library_includes_later_imports_preserves_exceptions_and_skips_exclusions() {
        let mut c = library();
        configure(&mut c, config(Some(true), None, None)).unwrap();
        assert_eq!(status(&c).unwrap().total_products, 1);
        c.execute("INSERT INTO products VALUES('444444','追加商品')", [])
            .unwrap();
        assert_eq!(status(&c).unwrap().rows.len(), 2);
        configure(&mut c, config(None, Some("444444"), Some(false))).unwrap();
        let meta = load_meta(&c).unwrap();
        assert!(meta.all_products);
        let saved = target_rows(&c, &meta).unwrap();
        assert!(
            !saved
                .iter()
                .find(|r| r.booth_id == "444444")
                .unwrap()
                .enabled
        );
        assert!(saved.iter().all(|r| r.booth_id != "434343"));
        assert_eq!(
            status(&c)
                .unwrap()
                .rows
                .iter()
                .filter(|r| r.enabled)
                .count(),
            1
        );
    }
    #[test]
    fn bulk_revocation_cancels_old_results_preserves_baselines_and_stops_future_enrollment() {
        let mut c = library();
        configure(&mut c, config(Some(true), None, None)).unwrap();
        let mut row = stored_row(&c, "424242").unwrap().unwrap();
        record(&mut row, Ok(snapshot("v1")));
        record(&mut row, Ok(snapshot("v2")));
        let generation = load_meta(&c).unwrap().generation;
        save_row(&c, &row).unwrap();
        configure(&mut c, config(Some(false), None, None)).unwrap();
        c.execute("INSERT INTO products VALUES('444444','追加商品')", [])
            .unwrap();
        let off_meta = load_meta(&c).unwrap();
        let off = target_rows(&c, &off_meta).unwrap();
        assert_eq!(off.len(), 1);
        assert!(!off[0].enabled);
        assert_eq!(changes(&off[0]), vec!["商品説明の変更"]);
        assert!(!allowed(&off_meta, &off[0], generation, true));
        configure(&mut c, config(Some(true), None, None)).unwrap();
        let on_meta = load_meta(&c).unwrap();
        let on = target_rows(&c, &on_meta).unwrap();
        assert_eq!(on.len(), 2);
        assert!(on.iter().all(|r| r.enabled));
        assert!(!allowed(&on_meta, &on[0], generation, true));
        assert_eq!(changes(&on[0]), vec!["商品説明の変更"]);
    }
    #[test]
    fn whole_library_has_no_five_hundred_item_limit() {
        let mut c = library();
        let tx = c.transaction().unwrap();
        for id in 100000..101000 {
            tx.execute("INSERT INTO products VALUES(?1,'商品')", [id.to_string()])
                .unwrap();
        }
        tx.execute("INSERT INTO products VALUES('bad-id','不正なID')", [])
            .unwrap();
        tx.commit().unwrap();
        let s = configure(&mut c, config(Some(true), None, None)).unwrap();
        assert_eq!(s.total_products, 1001);
        assert_eq!(s.rows.len(), 1001);
        assert!(s.rows.iter().all(|r| r.enabled));
        assert_eq!(stored_rows(&c).unwrap().len(), 1001);
    }
    #[test]
    fn ambiguous_bulk_changes_are_rejected_and_change_nothing() {
        let mut c = library();
        configure(&mut c, config(None, Some("424242"), Some(true))).unwrap();
        let before = db::setting(&c, STATE_KEY).unwrap();
        assert!(configure(&mut c, config(Some(true), Some("424242"), Some(false))).is_err());
        assert_eq!(db::setting(&c, STATE_KEY).unwrap(), before);
        let rows = stored_rows(&c).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].enabled);
    }

    #[test]
    fn the_single_value_of_older_versions_moves_into_rows_without_widening_consent() {
        let c = library();
        // No `allProducts`: written before whole-library consent existed.
        let legacy = serde_json::json!({
            "automatic": true,
            "retryAfter": 1_800_000_000i64,
            "generation": 7,
            "rows": [
                {
                    "boothId": "424242", "enabled": true, "generation": 3,
                    "attemptedAt": 1_794_000_000i64, "checkedAt": "2026-11-01T00:00:00Z", "error": null,
                    "baseline": { "description": "説明 v1", "files": [["9400001", "", "v1.zip"]] },
                    "latest": { "description": "説明 v2", "files": [["9400001", "", "v1.zip"]] },
                    "newFiles": ["9400002"], "currentFiles": ["9400001", "9400002"],
                },
                { "boothId": "999999", "enabled": true, "generation": 0, "baseline": null, "latest": null },
            ],
            "history": [{
                "boothId": "424242", "detectedAt": "2026-11-01T00:00:00Z", "source": "public",
                "descriptionChanged": true, "removed": [], "added": [],
            }],
        });
        db::set_setting(&c, LEGACY_KEY, &legacy.to_string()).unwrap();
        move_legacy_watch(&c).unwrap();

        assert_eq!(db::setting(&c, LEGACY_KEY).unwrap(), None);
        let meta = load_meta(&c).unwrap();
        assert!(meta.automatic);
        assert!(!meta.all_products);
        assert_eq!(meta.retry_after, Some(1_800_000_000));
        assert_eq!(meta.generation, 7);
        let rows = stored_rows(&c).unwrap();
        assert_eq!(rows.len(), 1, "a row of a product that is gone is dropped");
        let row = &rows[0];
        assert_eq!(row.generation, 3);
        assert_eq!(row.attempted_at, Some(1_794_000_000));
        assert_eq!(row.new_files, vec!["9400002"]);
        assert_eq!(changes(row), vec!["新しいファイルの追加", "商品説明の変更"]);
        // The stored text became the digest a later check of the same page produces.
        let page = r#"{"id":424242,"is_adult":false,"description":"説明  v1","variations":[]}"#;
        assert_eq!(
            row.baseline.as_ref().unwrap().description_digest,
            interpret("424242", page).unwrap().description_digest
        );
        assert!(!to_json(&row.baseline).unwrap().contains("説明"));
        assert_eq!(history(&c).unwrap().len(), 1);
        assert_eq!(status(&c).unwrap().rows.len(), 1);

        // A value that cannot be read stays, and the library still opens with nothing watched.
        let broken = library();
        db::set_setting(&broken, LEGACY_KEY, "{not json").unwrap();
        move_legacy_watch(&broken).unwrap();
        assert_eq!(
            db::setting(&broken, LEGACY_KEY).unwrap().as_deref(),
            Some("{not json")
        );
        assert!(status(&broken).unwrap().rows.is_empty());
    }

    #[test]
    fn opening_a_version_fourteen_library_moves_the_watch_and_keeps_products() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite");
        {
            let c = db::open(&path).unwrap();
            c.execute_batch(
                "DROP TABLE asset_watch; DROP TABLE asset_watch_history; PRAGMA user_version = 14;
                 INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('424242', '既存商品', 't', 't');",
            )
            .unwrap();
            let legacy = serde_json::json!({
                "automatic": false, "allProducts": true, "generation": 2,
                "rows": [{
                    "boothId": "424242", "enabled": false, "generation": 5,
                    "baseline": { "description": "長い説明", "files": [] },
                    "latest": { "description": "長い説明", "files": [] },
                }],
            });
            db::set_setting(&c, LEGACY_KEY, &legacy.to_string()).unwrap();
        }
        let c = db::open(&path).unwrap();
        let version: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, db::SCHEMA_VERSION);
        assert_eq!(db::setting(&c, LEGACY_KEY).unwrap(), None);
        let s = status(&c).unwrap();
        assert!(s.all_products);
        assert_eq!(s.rows.len(), 1);
        assert!(
            !s.rows[0].enabled,
            "a product the user turned off stays off"
        );
        assert!(s.rows[0].changes.is_empty());
        assert_eq!(s.rows[0].revision, 5);
        let title: String = c
            .query_row(
                "SELECT title FROM products WHERE booth_id = '424242'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "既存商品");
    }

    #[test]
    fn the_description_digest_never_changes_between_versions() {
        assert_eq!(description_digest(""), "");
        assert_eq!(description_digest("a"), "af63dc4c8601ec8c");
        assert_eq!(description_digest("abc"), "e71fa2190541574b");
        assert_ne!(description_digest("説明 v1"), description_digest("説明 v2"));
    }

    #[test]
    fn history_keeps_the_newest_two_hundred_records() {
        let c = library();
        for index in 0..HISTORY_LIMIT + 5 {
            push_history(
                &c,
                &UpdateRecord {
                    booth_id: "424242".into(),
                    detected_at: format!("t{index}"),
                    source: "public".into(),
                    description_changed: true,
                    removed: Vec::new(),
                    added: Vec::new(),
                },
            )
            .unwrap();
        }
        let records = history(&c).unwrap();
        assert_eq!(records.len(), HISTORY_LIMIT);
        assert_eq!(records[0].detected_at, format!("t{}", HISTORY_LIMIT + 4));
        assert_eq!(records.last().unwrap().detected_at, "t5");
    }

    #[test]
    fn checking_one_product_writes_only_its_row() {
        let mut c = library();
        c.execute("INSERT INTO products VALUES('444444','追加商品')", [])
            .unwrap();
        configure(&mut c, config(Some(true), None, None)).unwrap();
        let before: Vec<(i64, String)> = c
            .prepare("SELECT rowid, booth_id FROM asset_watch ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut row = stored_row(&c, "444444").unwrap().unwrap();
        record(&mut row, Ok(snapshot("v1")));
        let changed_before = c.total_changes();
        save_row(&c, &row).unwrap();
        assert_eq!(c.total_changes() - changed_before, 1);
        let after: Vec<(i64, String)> = c
            .prepare("SELECT rowid, booth_id FROM asset_watch ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(before, after, "updating a row keeps the order of the list");
        assert!(stored_row(&c, "424242").unwrap().unwrap().latest.is_none());
    }

    #[test]
    fn imported_files_record_new_files_and_authoritative_current_set() {
        let mut c = library();
        let mut before = HashMap::new();
        before.insert(
            "424242".to_string(),
            HashSet::from(["9871769".to_string(), "MochiFitter73r.zip".to_string()]),
        );
        let mut incoming = vec![IncomingProduct {
            booth_id: "424242".into(),
            title: "既存商品".into(),
            shop: String::new(),
            description_snippet: String::new(),
            url: String::new(),
            thumbnail_url: None,
            variations: Vec::new(),
            downloads: vec![IncomingDownload {
                filename: "MochiFitter75r.zip".into(),
                size_bytes: None,
                variation: String::new(),
                downloadable_id: "10000100".into(),
            }],
            is_free: None,
            is_adult: None,
            unpublished: None,
            received_gift: None,
            booth_category: None,
            booth_tags: None,
        }];
        record_imported_files(&c, &before, &incoming, false, false).unwrap();
        let partial = file_watches(&c).unwrap();
        let watch = partial.get("424242").unwrap();
        assert!(watch.new_files.contains("10000100"));
        assert!(watch.current_files.is_none());
        assert_eq!(
            status(&c).unwrap().rows[0].changes,
            vec!["新しいファイルの追加"]
        );

        record_imported_files(&c, &before, &incoming, true, false).unwrap();
        let full = file_watches(&c).unwrap();
        let watch = full.get("424242").unwrap();
        assert_eq!(
            watch.current_files.as_ref().unwrap(),
            &HashSet::from(["10000100".to_string()])
        );
        assert_eq!(
            history(&c).unwrap().len(),
            1,
            "the partial and full import do not repeat the same discovery"
        );
        let before_row = stored_rows(&c).unwrap().remove(0);
        let key = candidate_key(&before_row);
        let mut revised = before_row;
        revised.generation += 1;
        assert_eq!(
            key,
            candidate_key(&revised),
            "revisions alone do not change the notification"
        );

        clear_new_file_on_download(&c, "424242", Some("10000100"), "MochiFitter75r.zip").unwrap();
        assert!(status(&c).unwrap().rows[0].changes.is_empty());

        incoming[0].downloads[0].downloadable_id = "10000101".into();
        incoming[0].downloads[0].filename = "MochiFitter76r.zip".into();
        record_imported_files(&c, &before, &incoming, true, false).unwrap();
        record_imported_files(&c, &before, &incoming, true, false).unwrap();
        let records = history(&c).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].removed[0].downloadable_id.as_deref(),
            Some("10000100")
        );
        assert_eq!(
            records[0].added[0].downloadable_id.as_deref(),
            Some("10000101")
        );
        assert!(chrono::DateTime::parse_from_rfc3339(&records[0].detected_at).is_ok());
        let mut ack = config(None, Some("424242"), None);
        ack.acknowledge = Some(true);
        configure(&mut c, ack).unwrap();
        assert!(status(&c).unwrap().rows[0].changes.is_empty());
        assert_eq!(
            history(&c).unwrap().len(),
            2,
            "acknowledging the candidate keeps the evidence"
        );
    }
}
