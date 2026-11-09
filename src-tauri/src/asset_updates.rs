//! Opt-in public metadata checks. Never downloads files or uses a BOOTH session.
use crate::{AppState, booth_detail, db};
use crate::model::{IncomingDownload, IncomingProduct};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const KEY: &str = "asset_update_watch_v1";
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CHECK_DONE: AtomicUsize = AtomicUsize::new(0);
static CHECK_TOTAL: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Watch {
    automatic: bool,
    #[serde(default)]
    all_products: bool,
    #[serde(default)]
    retry_after: Option<i64>,
    generation: u64,
    rows: Vec<Row>,
    #[serde(default)]
    history: Vec<UpdateRecord>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    booth_id: String,
    enabled: bool,
    generation: u64,
    attempted_at: Option<i64>,
    checked_at: Option<String>,
    error: Option<String>,
    baseline: Option<Snapshot>,
    latest: Option<Snapshot>,
    #[serde(default)]
    new_files: Vec<String>,
    #[serde(default)]
    current_files: Option<Vec<String>>,
    #[serde(default)]
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
        downloadable_id: ((1..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())).then(|| id.to_owned()),
        variation: variation.to_owned(),
        filename: filename.to_owned(),
    }
}

fn record_delta(w: &mut Watch, booth_id: &str, source: &str, before: &[UpdateFile], after: &[UpdateFile], description_changed: bool) -> bool {
    let mut removed: Vec<_> = before.iter().filter(|f| !after.contains(f)).cloned().collect();
    let mut added: Vec<_> = after.iter().filter(|f| !before.contains(f)).cloned().collect();
    if removed.is_empty() && added.is_empty() && !description_changed { return false; }
    removed.sort(); removed.dedup();
    added.sort(); added.dedup();
    w.history.push(UpdateRecord { booth_id: booth_id.to_owned(), detected_at: crate::model::now_iso(), source: source.to_owned(), description_changed, removed, added });
    if w.history.len() > 200 { w.history.drain(..w.history.len() - 200); }
    true
}

pub(crate) fn history(c: &Connection) -> Result<Vec<UpdateRecord>, String> {
    let w = load(c)?;
    let titles = products(c)?;
    Ok(w.history.into_iter().rev().filter(|r| titles.contains_key(&r.booth_id)).collect())
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
struct Snapshot {
    // No adult description or tags, no URL, cookies, prices, popularity, or signed links.
    description: String,
    files: Vec<(String, String, String)>,
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
struct CheckProgress { done: usize, total: usize }
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
fn include_products(w: &mut Watch, ids: impl Iterator<Item = String>) {
    let mut known: std::collections::HashSet<_> =
        w.rows.iter().map(|r| r.booth_id.clone()).collect();
    for id in ids {
        if known.insert(id.clone()) {
            w.rows.push(new_row(id));
        }
    }
}
fn load(c: &Connection) -> Result<Watch, String> {
    let mut w: Watch = match db::setting(c, KEY)? {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "更新確認の設定を読めません")?,
        None => Watch::default(),
    };
    // Whole-library consent includes later imports. Existing disabled rows remain exceptions.
    if w.all_products {
        include_products(&mut w, products(c)?.into_keys());
    }
    Ok(w)
}
fn save(c: &mut Connection, w: &Watch) -> Result<(), String> {
    db::set_setting(
        c,
        KEY,
        &serde_json::to_string(w).map_err(|_| "更新確認の設定を保存できません")?,
    )
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
        if before.description != after.description {
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
    if changes(row).is_empty() { return None; }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut files = row.new_files.clone();
    files.sort(); files.dedup(); files.hash(&mut hash);
    if let (Some(before), Some(after)) = (&row.baseline, &row.latest) {
        if before.description != after.description { (&before.description, &after.description).hash(&mut hash); }
        if before.files != after.files { (&before.files, &after.files).hash(&mut hash); }
    }
    Some(format!("{:016x}", hash.finish()))
}

pub(crate) fn file_watches(c: &Connection) -> Result<HashMap<String, ProductFileWatch>, String> {
    let w = load(c)?;
    let mut map = HashMap::new();
    for row in w.rows {
        if row.new_files.is_empty() && row.current_files.is_none() {
            continue;
        }
        map.insert(
            row.booth_id,
            ProductFileWatch {
                new_files: row.new_files.into_iter().collect(),
                current_files: row.current_files.map(|files| files.into_iter().collect()),
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
    let mut w = load(c)?;
    let mut dirty = false;
    for product in products {
        if !booth_detail::item_json_url(&product.booth_id).is_ok()
            || !active(c, &product.booth_id)?
        {
            continue;
        }
        let incoming_keys: Vec<String> = product
            .downloads
            .iter()
            .filter_map(clean_file_key)
            .collect();
        if incoming_keys.is_empty() {
            continue;
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
            continue;
        }
        let row = if let Some(row) = w.rows.iter_mut().find(|r| r.booth_id == product.booth_id) {
            row
        } else {
            w.rows.push(new_row(product.booth_id.clone()));
            w.rows.last_mut().unwrap()
        };
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
                product.downloads.iter().filter(|f| !only_new || clean_file_key(f).is_some_and(|key| newly_marked.contains(&key)))
                    .map(|f| update_file(f.downloadable_id.trim(), &f.variation, &f.filename)).collect()
            };
            let previous = row.history_library_files.clone();
            let current = if authoritative { Some(refs(false)) } else { None };
            if let Some(current) = &current {
                if previous.as_ref() != Some(current) {
                    row.history_library_files = Some(current.clone());
                    dirty = true;
                }
            }
            dirty |= match (previous, current) {
                (Some(before), Some(after)) => record_delta(&mut w, &product.booth_id, "library", &before, &after, false),
                _ => record_delta(&mut w, &product.booth_id, "library", &[], &refs(true), false),
            };
        }
    }
    if dirty {
        db::set_setting(
            c,
            KEY,
            &serde_json::to_string(&w).map_err(|_| "更新確認の設定を保存できません")?,
        )?;
    }
    Ok(())
}

pub(crate) fn clear_new_file_on_download(
    c: &Connection,
    booth_id: &str,
    downloadable_id: Option<&str>,
    filename: &str,
) -> Result<(), String> {
    let mut w = load(c)?;
    let Some(row) = w.rows.iter_mut().find(|r| r.booth_id == booth_id) else {
        return Ok(());
    };
    let before = row.new_files.len();
    let id = downloadable_id.map(str::trim).unwrap_or("");
    let name = filename.trim();
    row.new_files
        .retain(|key| key != id && key != name);
    if row.new_files.len() != before {
        row.generation += 1;
        db::set_setting(
            c,
            KEY,
            &serde_json::to_string(&w).map_err(|_| "更新確認の設定を保存できません")?,
        )?;
    }
    Ok(())
}
pub fn status(c: &Connection) -> Result<Status, String> {
    let w = load(c)?;
    let titles = products(c)?;
    let mut rows = Vec::new();
    for row in w.rows {
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
        automatic: w.automatic,
        all_products: w.all_products,
        total_products: titles.len(),
        running,
        progress: (running && total > 0).then(|| CheckProgress { done: CHECK_DONE.load(Ordering::Relaxed).min(total), total }),
        rows,
    })
}
pub fn configure(c: &mut Connection, input: Configure) -> Result<Status, String> {
    let mut w = load(c)?;
    if input.all_products.is_some()
        && (input.booth_id.is_some() || input.enabled.is_some() || input.acknowledge.is_some())
    {
        return Err("全体設定と商品ごとの設定は別々に変更してください".into());
    }
    if let Some(all) = input.all_products {
        if all {
            include_products(&mut w, products(c)?.into_keys());
        }
        w.all_products = all;
        w.generation += 1;
        for row in &mut w.rows {
            row.enabled = all;
            row.generation += 1;
        }
    }
    if let Some(id) = input.booth_id {
        booth_detail::item_json_url(&id)?;
        if !active(c, &id)? {
            return Err("登録済みの商品を選んでください".into());
        }
        if let Some(row) = w.rows.iter_mut().find(|r| r.booth_id == id) {
            row.generation += 1;
            if let Some(enabled) = input.enabled {
                row.enabled = enabled;
            }
            if input.acknowledge == Some(true) {
                row.baseline = row.latest.clone();
                row.new_files.clear();
            }
        } else if input.enabled == Some(true) {
            w.rows.push(new_row(id));
        }
    } else if input.enabled.is_some() || input.acknowledge.is_some() {
        return Err("商品を選んでください".into());
    }
    if let Some(automatic) = input.automatic {
        w.automatic = automatic;
        w.generation += 1;
    }
    save(c, &w)?;
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
    let description = if v.get("is_adult").and_then(serde_json::Value::as_bool) == Some(false) {
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
                .map(|f| (f.downloadable_id.clone(), f.variation.clone(), f.filename.clone()))
                .collect();
            (tuples, files, is_free, is_adult)
        }
        crate::free_files::FreeFilesOutcome::NotFree
        | crate::free_files::FreeFilesOutcome::NoFiles => (Vec::new(), Vec::new(), None, None),
        _ => return Err("ファイル一覧を確認できません".into()),
    };
    files.sort();
    Ok(FetchedObservation {
        snapshot: Snapshot { description, files },
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
fn allowed(w: &Watch, row: &Row, generation: u64, manual: bool) -> bool {
    row.enabled && (manual || w.automatic) && w.generation == generation
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
        let w = load(&c)?;
        let now = chrono::Utc::now().timestamp();
        if !w.automatic || w.retry_after.is_some_and(|until| now < until)
            || !w.rows.iter().any(|r| r.enabled && due(r, now) && active(&c, &r.booth_id).unwrap_or(false)) {
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
            let w = load(&c)?;
            db::settings(&c)?.builtin_browser && w.rows.iter().any(|r| r.enabled)
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
        let w = load(&c)?;
        (
            w.rows
                .iter()
                .filter(|r| {
                    (manual
                        || w.retry_after
                            .is_none_or(|until| chrono::Utc::now().timestamp() >= until))
                        && r.enabled
                        && (manual || (w.automatic && due(r, chrono::Utc::now().timestamp())))
                })
                .map(|r| r.booth_id.clone())
                .collect::<Vec<_>>(),
            w.generation,
        )
    };
    CHECK_TOTAL.store(ids.len(), Ordering::Relaxed);
    let _ = app.emit_to("main", "asset-updates-changed", ());
    for (index, id) in ids.iter().enumerate() {
        if index > 0 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let revision = {
            let mut c = state.db.lock().map_err(|_| "設定を読めません")?;
            let mut w = load(&c)?;
            let Some(row) = w.rows.iter().find(|r| &r.booth_id == id) else {
                continue;
            };
            if !allowed(&w, row, generation, manual) {
                if w.generation != generation || (!manual && !w.automatic) {
                    break;
                }
                continue;
            }
            if !active(&c, id)? || (!manual && !due(row, chrono::Utc::now().timestamp())) {
                continue;
            }
            let row = w.rows.iter_mut().find(|r| &r.booth_id == id).unwrap();
            // Persist the attempt before I/O so restarts cannot hammer a failing endpoint.
            row.attempted_at = Some(chrono::Utc::now().timestamp());
            let revision = row.generation;
            save(&mut c, &w)?;
            revision
        };
        let result = fetch(id).await;
        let stop = result.as_ref().err().is_some_and(|e| {
            e.contains("BOOTH が") || e.contains("ログイン") || e == "商品情報を確認できません"
        });
        let mut files_updated = false;
        {
            let mut c = state.db.lock().map_err(|_| "設定を読めません")?;
            let mut w = load(&c)?;
            if let Some(row) = w.rows.iter().find(|r| &r.booth_id == id) {
                if allowed(&w, row, generation, manual)
                    && row.generation == revision
                    && active(&c, id)?
                {
                    if let Ok(obs) = &result {
                        if !obs.free_files.is_empty() {
                            let _ = db::store_free_files(
                                &mut c,
                                id,
                                &obs.free_files,
                                obs.is_free,
                                obs.is_adult,
                            );
                            files_updated = true;
                            w = load(&c)?;
                        }
                    }
                    let row = w.rows.iter_mut().find(|r| &r.booth_id == id).unwrap();
                    let previous = row.latest.clone();
                    record(row, result.map(|obs| obs.snapshot));
                    if row.error.is_none() {
                        if let (Some(before), Some(after)) = (previous, row.latest.as_ref()) {
                            let refs = |s: &Snapshot| s.files.iter().map(|(id, v, name)| update_file(id, v, name)).collect::<Vec<_>>();
                            let (old, new, desc) = (refs(&before), refs(after), before.description != after.description);
                            record_delta(&mut w, id, "public", &old, &new, desc);
                        }
                    }
                    if stop {
                        w.retry_after = Some(chrono::Utc::now().timestamp() + 3600);
                    }
                    save(&mut c, &w)?;
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
            description: text.into(),
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
        let mut w = Watch::default();
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
            assert!(interpret("424242", &raw).unwrap().description.is_empty());
        }
    }
    #[test]
    fn watch_settings_persist_without_changing_existing_schema_or_settings() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE products(booth_id TEXT PRIMARY KEY,title TEXT); CREATE TABLE product_exclusions(booth_id TEXT PRIMARY KEY,excluded_at TEXT); INSERT INTO products VALUES('424242','ダミー'); INSERT INTO settings VALUES('library_folder','saved'); PRAGMA user_version=7;").unwrap();
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
        let mut w = load(&c).unwrap();
        record(&mut w.rows[0], Ok(snapshot("v1")));
        record(&mut w.rows[0], Ok(snapshot("v2")));
        save(&mut c, &w).unwrap();
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
        let saved = load(&c).unwrap();
        assert!(saved.all_products);
        assert!(
            !saved
                .rows
                .iter()
                .find(|r| r.booth_id == "444444")
                .unwrap()
                .enabled
        );
        assert!(saved.rows.iter().all(|r| r.booth_id != "434343"));
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
        let mut w = load(&c).unwrap();
        record(&mut w.rows[0], Ok(snapshot("v1")));
        record(&mut w.rows[0], Ok(snapshot("v2")));
        let generation = w.generation;
        save(&mut c, &w).unwrap();
        configure(&mut c, config(Some(false), None, None)).unwrap();
        c.execute("INSERT INTO products VALUES('444444','追加商品')", [])
            .unwrap();
        let off = load(&c).unwrap();
        assert_eq!(off.rows.len(), 1);
        assert!(!off.rows[0].enabled);
        assert_eq!(changes(&off.rows[0]), vec!["商品説明の変更"]);
        assert!(!allowed(&off, &off.rows[0], generation, true));
        configure(&mut c, config(Some(true), None, None)).unwrap();
        let on = load(&c).unwrap();
        assert_eq!(on.rows.len(), 2);
        assert!(on.rows.iter().all(|r| r.enabled));
        assert!(!allowed(&on, &on.rows[0], generation, true));
        assert_eq!(changes(&on.rows[0]), vec!["商品説明の変更"]);
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
        assert_eq!(load(&c).unwrap().rows.len(), 1001);
    }
    #[test]
    fn legacy_consent_is_not_widened_and_ambiguous_bulk_changes_are_rejected() {
        let mut c = library();
        configure(&mut c, config(None, Some("424242"), Some(true))).unwrap();
        let raw = db::setting(&c, KEY)
            .unwrap()
            .unwrap()
            .replace("\"allProducts\":false,", "");
        db::set_setting(&c, KEY, &raw).unwrap();
        let legacy = status(&c).unwrap();
        assert!(!legacy.all_products);
        assert!(!legacy.automatic);
        assert_eq!(legacy.rows.len(), 1);
        assert!(configure(&mut c, config(Some(true), Some("424242"), Some(false))).is_err());
        assert_eq!(db::setting(&c, KEY).unwrap().unwrap(), raw);
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
        assert_eq!(history(&c).unwrap().len(), 1, "the partial and full import do not repeat the same discovery");
        let before_row = load(&c).unwrap().rows.remove(0);
        let key = candidate_key(&before_row);
        let mut revised = before_row;
        revised.generation += 1;
        assert_eq!(key, candidate_key(&revised), "revisions alone do not change the notification");

        clear_new_file_on_download(&c, "424242", Some("10000100"), "MochiFitter75r.zip").unwrap();
        assert!(status(&c).unwrap().rows[0].changes.is_empty());

        incoming[0].downloads[0].downloadable_id = "10000101".into();
        incoming[0].downloads[0].filename = "MochiFitter76r.zip".into();
        record_imported_files(&c, &before, &incoming, true, false).unwrap();
        record_imported_files(&c, &before, &incoming, true, false).unwrap();
        let records = history(&c).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].removed[0].downloadable_id.as_deref(), Some("10000100"));
        assert_eq!(records[0].added[0].downloadable_id.as_deref(), Some("10000101"));
        assert!(chrono::DateTime::parse_from_rfc3339(&records[0].detected_at).is_ok());
        let mut ack = config(None, Some("424242"), None);
        ack.acknowledge = Some(true);
        configure(&mut c, ack).unwrap();
        assert!(status(&c).unwrap().rows[0].changes.is_empty());
        assert_eq!(history(&c).unwrap().len(), 2, "acknowledging the candidate keeps the evidence");
    }
}
