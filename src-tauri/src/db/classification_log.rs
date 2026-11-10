//! The classification log: which run moved which product, from what to what.
//!
//! This is local display history. It is not part of the sync export, and nothing reads it to
//! decide a classification. Writing it must never fail the classification it describes, so
//! `observe` and `observe_all` report a log failure and carry on.
//!
//! Recording compares the product before and after a step instead of hooking each writer. The
//! catalog, owned-body, ended-listing, and re-resolve paths all change the same columns, and a
//! comparison covers them with one rule. A run holds one entry per product: a later step in the
//! same run updates that entry, and a step that restores the original state removes it.
//!
//! Undo returns one entry to its "before". It refuses when the product no longer matches the
//! entry's "after", so an old entry cannot overwrite a newer change.

use super::{
    Sql, apply_owned_restore, avatar_exists, delete_links, load_avatars, parse_list,
    product_excluded, require_product, to_json,
};
use crate::model::{OwnedAvatarRestore, now_iso};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) const TABLES: &str = "
CREATE TABLE IF NOT EXISTS classification_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL,
    kind TEXT NOT NULL,
    classifier TEXT NOT NULL DEFAULT '',
    note TEXT
);
CREATE TABLE IF NOT EXISTS classification_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL REFERENCES classification_runs(id) ON DELETE CASCADE,
    booth_id TEXT NOT NULL,
    before_category TEXT,
    after_category TEXT,
    before_links TEXT NOT NULL DEFAULT '[]',
    after_links TEXT NOT NULL DEFAULT '[]',
    confidence REAL,
    unresolved_names TEXT NOT NULL DEFAULT '[]',
    before_state TEXT NOT NULL DEFAULT '{}',
    undone INTEGER NOT NULL DEFAULT 0,
    UNIQUE (run_id, booth_id)
);
CREATE INDEX IF NOT EXISTS idx_classification_entries_booth ON classification_entries(booth_id);
";

/// BOOTH import, including the listed-compatibility pass and dictionary settling it runs.
pub(crate) const KIND_IMPORT: &str = "import";
/// `extract_products`: the BOOTH category and title rules, plus the batch-end re-resolve.
pub(crate) const KIND_RECLASSIFY: &str = "reclassify";
/// The catalog applied again: the maintenance button, a catalog refresh, or restoring seed rows.
pub(crate) const KIND_CATALOG: &str = "catalog";
/// Re-resolve that follows a manual avatar edit.
pub(crate) const KIND_EDIT: &str = "edit";
/// Dictionary settling when the database opens.
pub(crate) const KIND_STARTUP: &str = "startup";
/// A public product read that changed the description, which clears the classification.
pub(crate) const KIND_BOOTH_DETAIL: &str = "booth_detail";
/// A call with no entry point of its own.
#[cfg(test)]
pub(crate) const KIND_IMPLICIT: &str = "implicit";

pub(crate) const CLASSIFIER_CATALOG: &str = "catalog";
/// The BOOTH category and tags, then the title and description rules. Older runs keep the
/// model name they used (`ollama`, `apple`, `cloud`).
pub(crate) const CLASSIFIER_RULES: &str = "rules";
/// The run decided nothing by itself, such as a reset after a description change.
pub(crate) const CLASSIFIER_NONE: &str = "";

/// One run of the log. The row is written on the first change, so a run that changed nothing
/// leaves no trace.
#[derive(Debug)]
pub(crate) struct ClassificationRun {
    kind: &'static str,
    classifier: String,
    note: Option<String>,
    started_at: String,
    id: Option<i64>,
}

impl ClassificationRun {
    pub(crate) fn new(kind: &'static str, classifier: &str) -> Self {
        Self {
            kind,
            classifier: classifier.to_string(),
            note: None,
            started_at: now_iso(),
            id: None,
        }
    }

    pub(crate) fn with_note(mut self, note: &str) -> Self {
        self.note = Some(note.to_string());
        self
    }

    fn ensure_id(&mut self, connection: &Connection) -> Result<i64, String> {
        if let Some(id) = self.id {
            return Ok(id);
        }
        connection
            .execute(
                "INSERT INTO classification_runs (started_at, kind, classifier, note)
                 VALUES (?1, ?2, ?3, ?4)",
                params![self.started_at, self.kind, self.classifier, self.note],
            )
            .sql()?;
        let id = connection.last_insert_rowid();
        self.id = Some(id);
        Ok(id)
    }
}

/// What a restore needs beyond the category and the link set, stored as `before_state`.
/// Names must travel with the links: re-resolve rebuilds links from `compatible_avatars`, so
/// restoring links alone would be undone by the next pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateDetail {
    genre: Option<String>,
    is_generic: bool,
    compatible_avatars: Vec<String>,
    unresolved_names: Vec<String>,
    confidence: Option<f64>,
    extracted_at: Option<String>,
    classifier_version: i64,
    user_confirmed: bool,
    needs_review: bool,
    link_sources: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
struct ProductState {
    category: Option<String>,
    /// Sorted avatar ids.
    links: Vec<String>,
    detail: StateDetail,
}

impl ProductState {
    /// A product the import has not created yet, with the defaults a new row gets.
    fn absent() -> Self {
        Self {
            category: None,
            links: Vec::new(),
            detail: StateDetail {
                genre: None,
                is_generic: false,
                compatible_avatars: Vec::new(),
                unresolved_names: Vec::new(),
                confidence: None,
                extracted_at: None,
                classifier_version: 1,
                user_confirmed: false,
                needs_review: true,
                link_sources: BTreeMap::new(),
            },
        }
    }
}

/// An entry exists when the category, the link set, or the unresolved names moved.
fn differs(left: &ProductState, right: &ProductState) -> bool {
    left.category != right.category
        || left.links != right.links
        || sorted(&left.detail.unresolved_names) != sorted(&right.detail.unresolved_names)
}

fn sorted(names: &[String]) -> Vec<&String> {
    let mut list: Vec<&String> = names.iter().collect();
    list.sort();
    list
}

const STATE_SELECT: &str = "SELECT booth_id, category, unresolved_names_json, confidence, genre, is_generic,
       compatible_avatars_json, extracted_at, classifier_version, user_confirmed, needs_review
     FROM products";

fn map_state(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, ProductState)> {
    let unresolved: String = row.get(2)?;
    let compatible: String = row.get(6)?;
    Ok((
        row.get(0)?,
        ProductState {
            category: row.get(1)?,
            links: Vec::new(),
            detail: StateDetail {
                genre: row.get(4)?,
                is_generic: row.get::<_, i64>(5)? != 0,
                compatible_avatars: parse_list(&compatible),
                unresolved_names: parse_list(&unresolved),
                confidence: row.get(3)?,
                extracted_at: row.get(7)?,
                classifier_version: row.get(8)?,
                user_confirmed: row.get::<_, i64>(9)? != 0,
                needs_review: row.get::<_, i64>(10)? != 0,
                link_sources: BTreeMap::new(),
            },
        },
    ))
}

fn product_state(connection: &Connection, booth_id: &str) -> Result<Option<ProductState>, String> {
    let found = connection
        .query_row(
            &format!("{STATE_SELECT} WHERE booth_id = ?1"),
            params![booth_id],
            map_state,
        )
        .optional()
        .sql()?;
    let Some((_, mut state)) = found else {
        return Ok(None);
    };
    let mut statement = connection
        .prepare(
            "SELECT avatar_id, source FROM product_avatars WHERE product_id = ?1 ORDER BY avatar_id",
        )
        .sql()?;
    let rows = statement
        .query_map(params![booth_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .sql()?;
    for row in rows {
        let (avatar_id, source) = row.sql()?;
        state.links.push(avatar_id.clone());
        state.detail.link_sources.insert(avatar_id, source);
    }
    Ok(Some(state))
}

fn all_states(connection: &Connection) -> Result<HashMap<String, ProductState>, String> {
    let mut statement = connection.prepare(STATE_SELECT).sql()?;
    let rows = statement.query_map([], map_state).sql()?;
    let mut states = rows.collect::<Result<HashMap<_, _>, _>>().sql()?;
    let mut statement = connection
        .prepare("SELECT product_id, avatar_id, source FROM product_avatars ORDER BY product_id, avatar_id")
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .sql()?;
    for row in rows {
        let (product_id, avatar_id, source) = row.sql()?;
        if let Some(state) = states.get_mut(&product_id) {
            state.links.push(avatar_id.clone());
            state.detail.link_sources.insert(avatar_id, source);
        }
    }
    Ok(states)
}

fn encode_detail(detail: &StateDetail) -> Result<String, String> {
    serde_json::to_string(detail).map_err(|err| err.to_string())
}

fn decode_before(
    category: Option<String>,
    links: &str,
    detail: &str,
) -> Result<ProductState, String> {
    Ok(ProductState {
        category,
        links: parse_list(links),
        detail: serde_json::from_str(detail).map_err(|err| err.to_string())?,
    })
}

fn record(
    connection: &Connection,
    run: &mut ClassificationRun,
    booth_id: &str,
    before: &ProductState,
    after: &ProductState,
) -> Result<(), String> {
    if let Some(run_id) = run.id {
        let existing: Option<(i64, Option<String>, String, String)> = connection
            .query_row(
                "SELECT id, before_category, before_links, before_state
                 FROM classification_entries WHERE run_id = ?1 AND booth_id = ?2",
                params![run_id, booth_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .sql()?;
        if let Some((entry_id, category, links, detail)) = existing {
            // The run's first state stays the "before", so Undo returns to where the run began.
            let first = decode_before(category, &links, &detail)?;
            if differs(&first, after) {
                connection
                    .execute(
                        "UPDATE classification_entries
                         SET after_category = ?1, after_links = ?2, confidence = ?3, unresolved_names = ?4
                         WHERE id = ?5",
                        params![
                            after.category,
                            to_json(&after.links)?,
                            after.detail.confidence,
                            to_json(&after.detail.unresolved_names)?,
                            entry_id
                        ],
                    )
                    .sql()?;
            } else {
                connection
                    .execute(
                        "DELETE FROM classification_entries WHERE id = ?1",
                        params![entry_id],
                    )
                    .sql()?;
            }
            return Ok(());
        }
    }
    if !differs(before, after) {
        return Ok(());
    }
    let run_id = run.ensure_id(connection)?;
    connection
        .execute(
            "INSERT INTO classification_entries (
                run_id, booth_id, before_category, after_category, before_links, after_links,
                confidence, unresolved_names, before_state
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                run_id,
                booth_id,
                before.category,
                after.category,
                to_json(&before.links)?,
                to_json(&after.links)?,
                after.detail.confidence,
                to_json(&after.detail.unresolved_names)?,
                encode_detail(&before.detail)?
            ],
        )
        .sql()?;
    Ok(())
}

fn report_failure(err: &str) {
    eprintln!("VRCLibMan: 分類記録を書けませんでした: {err}");
}

fn record_product(
    connection: &Connection,
    run: &mut ClassificationRun,
    booth_id: &str,
    before: &ProductState,
) -> Result<(), String> {
    match product_state(connection, booth_id)? {
        Some(after) => record(connection, run, booth_id, before, &after),
        None => Ok(()),
    }
}

/// Runs one step on one product and logs what it changed. A log failure is reported and dropped.
pub(crate) fn observe<T>(
    connection: &mut Connection,
    run: &mut ClassificationRun,
    booth_id: &str,
    step: impl FnOnce(&mut Connection) -> Result<T, String>,
) -> Result<T, String> {
    let before = match product_state(connection, booth_id) {
        Ok(state) => Some(state.unwrap_or_else(ProductState::absent)),
        Err(err) => {
            report_failure(&err);
            None
        }
    };
    let result = step(connection);
    if let Some(before) = before {
        if let Err(err) = record_product(connection, run, booth_id, &before) {
            report_failure(&err);
        }
    }
    result
}

fn record_all(
    connection: &mut Connection,
    run: &mut ClassificationRun,
    before: &HashMap<String, ProductState>,
) -> Result<(), String> {
    let after = all_states(connection)?;
    let mut ids: Vec<&String> = after.keys().collect();
    ids.sort();
    let run_id = run.id;
    let tx = connection.transaction().sql()?;
    let mut write = || -> Result<(), String> {
        let absent = ProductState::absent();
        for id in &ids {
            let previous = before.get(*id).unwrap_or(&absent);
            record(&tx, run, id, previous, &after[*id])?;
        }
        Ok(())
    };
    let written = write();
    let saved = written.and_then(|_| tx.commit().sql());
    if saved.is_err() {
        // A rolled-back run row must not be reused by a later step.
        run.id = run_id;
    }
    saved
}

/// `observe` for a step that may touch any product. All of it runs under one database lock, so
/// nothing else lands between the two reads.
pub(crate) fn observe_all<T>(
    connection: &mut Connection,
    run: &mut ClassificationRun,
    step: impl FnOnce(&mut Connection) -> Result<T, String>,
) -> Result<T, String> {
    let before = match all_states(connection) {
        Ok(states) => Some(states),
        Err(err) => {
            report_failure(&err);
            None
        }
    };
    let result = step(connection);
    if let Some(before) = before {
        if let Err(err) = record_all(connection, run, &before) {
            report_failure(&err);
        }
    }
    result
}

pub(crate) const KIND_UNDO: &str = "undo";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationRunView {
    pub id: i64,
    pub started_at: String,
    pub kind: String,
    pub classifier: String,
    pub note: Option<String>,
    pub changed_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationRunPage {
    pub total: u32,
    pub runs: Vec<ClassificationRunView>,
}

/// An avatar in an entry. A deleted avatar keeps its id as the name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationLink {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationEntryView {
    pub id: i64,
    pub run_id: i64,
    pub started_at: String,
    pub kind: String,
    pub classifier: String,
    pub booth_id: String,
    pub title: Option<String>,
    pub before_category: Option<String>,
    pub after_category: Option<String>,
    pub before_links: Vec<ClassificationLink>,
    pub after_links: Vec<ClassificationLink>,
    pub confidence: Option<f64>,
    pub unresolved_names: Vec<String>,
    pub undone: bool,
    /// Why Undo is not available now. `None` means it is.
    pub undo_blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedNameCount {
    pub name: String,
    pub product_count: u32,
}

pub(crate) fn run_page(
    connection: &Connection,
    offset: u32,
    limit: u32,
) -> Result<ClassificationRunPage, String> {
    let total: u32 = connection
        .query_row(
            "SELECT COUNT(DISTINCT run_id) FROM classification_entries",
            [],
            |row| row.get(0),
        )
        .sql()?;
    let mut statement = connection
        .prepare(
            "SELECT r.id, r.started_at, r.kind, r.classifier, r.note, COUNT(e.id)
             FROM classification_runs r
             JOIN classification_entries e ON e.run_id = r.id
             GROUP BY r.id
             ORDER BY r.id DESC
             LIMIT ?1 OFFSET ?2",
        )
        .sql()?;
    let rows = statement
        .query_map(params![limit, offset], |row| {
            Ok(ClassificationRunView {
                id: row.get(0)?,
                started_at: row.get(1)?,
                kind: row.get(2)?,
                classifier: row.get(3)?,
                note: row.get(4)?,
                changed_count: row.get(5)?,
            })
        })
        .sql()?;
    let runs = rows.collect::<Result<Vec<_>, _>>().sql()?;
    Ok(ClassificationRunPage { total, runs })
}

struct EntryRow {
    id: i64,
    run_id: i64,
    started_at: String,
    kind: String,
    classifier: String,
    booth_id: String,
    title: Option<String>,
    before_category: Option<String>,
    after_category: Option<String>,
    before_links: String,
    after_links: String,
    confidence: Option<f64>,
    unresolved_names: String,
    before_state: String,
    undone: bool,
}

const ENTRY_SELECT: &str = "SELECT e.id, e.run_id, r.started_at, r.kind, r.classifier, e.booth_id, p.title,
       e.before_category, e.after_category, e.before_links, e.after_links, e.confidence,
       e.unresolved_names, e.before_state, e.undone
     FROM classification_entries e
     JOIN classification_runs r ON r.id = e.run_id
     LEFT JOIN products p ON p.booth_id = e.booth_id";

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntryRow> {
    Ok(EntryRow {
        id: row.get(0)?,
        run_id: row.get(1)?,
        started_at: row.get(2)?,
        kind: row.get(3)?,
        classifier: row.get(4)?,
        booth_id: row.get(5)?,
        title: row.get(6)?,
        before_category: row.get(7)?,
        after_category: row.get(8)?,
        before_links: row.get(9)?,
        after_links: row.get(10)?,
        confidence: row.get(11)?,
        unresolved_names: row.get(12)?,
        before_state: row.get(13)?,
        undone: row.get::<_, i64>(14)? != 0,
    })
}

fn entry_views(
    connection: &Connection,
    rows: Vec<EntryRow>,
) -> Result<Vec<ClassificationEntryView>, String> {
    let names: HashMap<String, String> = load_avatars(connection)?
        .into_iter()
        .map(|avatar| (avatar.id, avatar.display_name))
        .collect();
    let named = |json: &str| -> Vec<ClassificationLink> {
        parse_list(json)
            .into_iter()
            .map(|id| ClassificationLink {
                name: names.get(&id).cloned().unwrap_or_else(|| id.clone()),
                id,
            })
            .collect()
    };
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let undo_blocked = undo_blocker(connection, &row)?;
        views.push(ClassificationEntryView {
            before_links: named(&row.before_links),
            after_links: named(&row.after_links),
            unresolved_names: parse_list(&row.unresolved_names),
            undo_blocked,
            id: row.id,
            run_id: row.run_id,
            started_at: row.started_at,
            kind: row.kind,
            classifier: row.classifier,
            booth_id: row.booth_id,
            title: row.title,
            before_category: row.before_category,
            after_category: row.after_category,
            confidence: row.confidence,
            undone: row.undone,
        });
    }
    Ok(views)
}

pub(crate) fn run_entries(
    connection: &Connection,
    run_id: i64,
    offset: u32,
    limit: u32,
) -> Result<Vec<ClassificationEntryView>, String> {
    let mut statement = connection
        .prepare(&format!(
            "{ENTRY_SELECT} WHERE e.run_id = ?1 ORDER BY e.id LIMIT ?2 OFFSET ?3"
        ))
        .sql()?;
    let rows = statement
        .query_map(params![run_id, limit, offset], map_entry)
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    entry_views(connection, rows)
}

/// One product's history, by exact id or part of the title, newest first.
pub(crate) fn search_entries(
    connection: &Connection,
    query: &str,
    limit: u32,
) -> Result<Vec<ClassificationEntryView>, String> {
    let needle = query.trim();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(&format!(
            "{ENTRY_SELECT}
             WHERE e.booth_id = ?1 OR instr(lower(IFNULL(p.title, '')), lower(?1)) > 0
             ORDER BY e.id DESC LIMIT ?2"
        ))
        .sql()?;
    let rows = statement
        .query_map(params![needle, limit], map_entry)
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    entry_views(connection, rows)
}

/// Names the dictionary still lacks, by how many products mentioned them. An undone entry and a
/// name that resolves now are left out, so the list stays usable as a list of gaps to report.
pub(crate) fn unresolved_name_counts(
    connection: &Connection,
) -> Result<Vec<UnresolvedNameCount>, String> {
    let avatars = load_avatars(connection)?;
    let mut statement = connection
        .prepare(
            "SELECT e.booth_id, e.unresolved_names
             FROM classification_entries e
             JOIN classification_runs r ON r.id = e.run_id
             WHERE e.undone = 0 AND r.kind <> ?1
               AND e.booth_id NOT IN (SELECT booth_id FROM product_exclusions)",
        )
        .sql()?;
    let rows = statement
        .query_map(params![KIND_UNDO], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .sql()?;
    let mut products: HashMap<String, HashSet<String>> = HashMap::new();
    for row in rows {
        let (booth_id, names) = row.sql()?;
        for name in parse_list(&names) {
            if !name.trim().is_empty() {
                products.entry(name).or_default().insert(booth_id.clone());
            }
        }
    }
    let mut counts: Vec<UnresolvedNameCount> = products
        .into_iter()
        .filter(|(name, _)| crate::resolve::resolve_name(name, &avatars).is_none())
        .map(|(name, ids)| UnresolvedNameCount {
            name,
            product_count: ids.len() as u32,
        })
        .collect();
    counts.sort_by(|left, right| {
        right
            .product_count
            .cmp(&left.product_count)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(counts)
}

/// The reason Undo must refuse this entry. Every check protects something newer than the entry:
/// a later run, or a choice the person made by hand.
fn undo_blocker(connection: &Connection, row: &EntryRow) -> Result<Option<String>, String> {
    if row.undone {
        return Ok(Some("すでに元に戻しています".into()));
    }
    if row.kind == KIND_UNDO {
        return Ok(Some("Undo の記録は元に戻せません".into()));
    }
    let Some(current) = product_state(connection, &row.booth_id)? else {
        return Ok(Some("商品が見つかりません".into()));
    };
    if product_excluded(connection, &row.booth_id)? {
        return Ok(Some("この商品は除外されています".into()));
    }
    let product = require_product(connection, &row.booth_id)?;
    if product.links_locked {
        return Ok(Some("対応アバターが手動で固定されているため戻せません".into()));
    }
    if current.category != row.after_category || current.links != parse_list(&row.after_links) {
        return Ok(Some("この記録のあとに分類が変わったため戻せません".into()));
    }
    let before = decode_before(row.before_category.clone(), &row.before_links, &row.before_state)?;
    if product.category_locked && current.category != before.category {
        return Ok(Some("カテゴリが手動で固定されているため戻せません".into()));
    }
    if product.genre_locked && current.detail.genre != before.detail.genre {
        return Ok(Some("ジャンルが手動で固定されているため戻せません".into()));
    }
    for avatar_id in &before.links {
        if !avatar_exists(connection, avatar_id)? {
            return Ok(Some("元の対応アバターが対応表から消えているため戻せません".into()));
        }
    }
    Ok(None)
}

/// Restores the entry's "before" and records the restore as its own run.
/// This is not a pin: a later run may classify the product the same way again.
pub(crate) fn undo_entry(connection: &mut Connection, entry_id: i64) -> Result<(), String> {
    let row = connection
        .query_row(
            &format!("{ENTRY_SELECT} WHERE e.id = ?1"),
            params![entry_id],
            map_entry,
        )
        .optional()
        .sql()?
        .ok_or("記録が見つかりません")?;
    if let Some(reason) = undo_blocker(connection, &row)? {
        return Err(reason);
    }
    let before = decode_before(row.before_category.clone(), &row.before_links, &row.before_state)?;
    let current = product_state(connection, &row.booth_id)?.ok_or("商品が見つかりません")?;
    // A link this run added may be the only thing holding a body or a placeholder.
    let released: Vec<String> = current
        .links
        .iter()
        .filter(|id| !before.links.contains(*id))
        .filter(|id| {
            let source = current.detail.link_sources.get(*id).map(String::as_str);
            id.starts_with("owned-") || matches!(source, Some("owned" | "catalog"))
        })
        .cloned()
        .collect();
    let tx = connection.transaction().sql()?;
    tx.execute(
        "UPDATE products SET category = ?1, genre = ?2, compatible_avatars_json = ?3,
         unresolved_names_json = ?4, is_generic = ?5, confidence = ?6, extracted_at = ?7,
         classifier_version = ?8, user_confirmed = ?9, needs_review = ?10, updated_at = ?11
         WHERE booth_id = ?12",
        params![
            before.category,
            before.detail.genre,
            to_json(&before.detail.compatible_avatars)?,
            to_json(&before.detail.unresolved_names)?,
            before.detail.is_generic,
            before.detail.confidence,
            before.detail.extracted_at,
            before.detail.classifier_version,
            before.detail.user_confirmed,
            before.detail.needs_review,
            now_iso(),
            row.booth_id
        ],
    )
    .sql()?;
    delete_links(&tx, &row.booth_id)?;
    for avatar_id in &before.links {
        let source = before
            .detail
            .link_sources
            .get(avatar_id)
            .map(String::as_str)
            .unwrap_or("extracted");
        tx.execute(
            "INSERT OR IGNORE INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, ?3)",
            params![row.booth_id, avatar_id, source],
        )
        .sql()?;
    }
    for avatar_id in &released {
        apply_owned_restore(
            &tx,
            &OwnedAvatarRestore {
                id: avatar_id.clone(),
                display_name: String::new(),
                aliases: Vec::new(),
                owned: false,
                remove_if_unused: true,
            },
        )?;
    }
    tx.execute(
        "UPDATE classification_entries SET undone = 1 WHERE id = ?1",
        params![entry_id],
    )
    .sql()?;
    let after = product_state(&tx, &row.booth_id)?.ok_or("商品が見つかりません")?;
    let mut run = ClassificationRun::new(KIND_UNDO, CLASSIFIER_NONE);
    record(&tx, &mut run, &row.booth_id, &current, &after)?;
    crate::sync_meta::touch_product(&tx, &row.booth_id)?;
    for avatar_id in &released {
        crate::sync_meta::touch_avatar(&tx, avatar_id)?;
    }
    tx.commit().sql()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;

    const LISTED: &str = "対応アバター\nライム_ver1.00 https://booth.pm/ja/items/1001\n基本仕様\n布です。";

    struct Entry {
        run_id: i64,
        before_category: Option<String>,
        after_category: Option<String>,
        before_links: Vec<String>,
        after_links: Vec<String>,
        confidence: Option<f64>,
        unresolved: Vec<String>,
    }

    fn product(connection: &mut Connection, booth_id: &str) {
        import_export_json(
            connection,
            &serde_json::json!({"boothId": booth_id, "title": "見本の服", "description": "布です。"})
                .to_string(),
        )
        .unwrap();
    }

    fn outfit_for(names: &[&str], confidence: f64) -> Extraction {
        Extraction {
            category: Category::Outfit,
            genre: None,
            compatible_avatars: names.iter().map(|name| name.to_string()).collect(),
            is_generic: false,
            confidence,
        }
    }

    fn entries(connection: &Connection, booth_id: &str) -> Vec<Entry> {
        let mut statement = connection
            .prepare(
                "SELECT run_id, before_category, after_category, before_links, after_links,
                        confidence, unresolved_names
                 FROM classification_entries WHERE booth_id = ?1 ORDER BY id",
            )
            .unwrap();
        let rows = statement
            .query_map(params![booth_id], |row| {
                let before_links: String = row.get(3)?;
                let after_links: String = row.get(4)?;
                let unresolved: String = row.get(6)?;
                Ok(Entry {
                    run_id: row.get(0)?,
                    before_category: row.get(1)?,
                    after_category: row.get(2)?,
                    before_links: parse_list(&before_links),
                    after_links: parse_list(&after_links),
                    confidence: row.get(5)?,
                    unresolved: parse_list(&unresolved),
                })
            })
            .unwrap();
        rows.collect::<Result<Vec<_>, _>>().unwrap()
    }

    fn run_of(connection: &Connection, run_id: i64) -> (String, String, Option<String>) {
        connection
            .query_row(
                "SELECT kind, classifier, note FROM classification_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    fn run_count(connection: &Connection) -> i64 {
        connection
            .query_row("SELECT COUNT(*) FROM classification_runs", [], |row| row.get(0))
            .unwrap()
    }

    fn lime_id(connection: &Connection) -> String {
        let avatars = load_avatars(connection).unwrap();
        crate::resolve::resolve_name("ライム", &avatars).unwrap().id.clone()
    }

    #[test]
    fn a_new_database_has_the_log_tables() {
        let connection = open_memory().unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(run_count(&connection), 0);
        assert_eq!(version, 12);
    }

    #[test]
    fn a_version_nine_database_gains_the_tables_and_keeps_its_products() {
        let connection = open_memory().unwrap();
        connection
            .execute_batch(
                "DROP TABLE classification_entries;
                 DROP TABLE classification_runs;
                 INSERT INTO products (booth_id, title, category, needs_review, imported_at, updated_at)
                   VALUES ('9001', '既存の服', 'outfit', 1, 't', 't');
                 PRAGMA user_version = 9;",
            )
            .unwrap();
        migrate(&connection).unwrap();
        migrate(&connection).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 12);
        assert_eq!(run_count(&connection), 0, "the migration writes no pseudo history");
        let product = require_product(&connection, "9001").unwrap();
        assert_eq!(product.category, Some(Category::Outfit));
        assert!(product.needs_review, "the migration leaves product rows alone");
    }

    #[test]
    fn an_apply_records_the_change_with_its_before_and_after() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7001");
        let lime = lime_id(&connection);
        let mut run = ClassificationRun::new(KIND_RECLASSIFY, "ollama").with_note("unclassified");
        apply_extraction_in_run(&mut connection, &mut run, "7001", &outfit_for(&["ライム"], 0.8), 0.6)
            .unwrap();
        let logged = entries(&connection, "7001");
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].before_category, None);
        assert_eq!(logged[0].after_category.as_deref(), Some("outfit"));
        assert!(logged[0].before_links.is_empty());
        assert_eq!(logged[0].after_links, vec![lime]);
        assert_eq!(logged[0].confidence, Some(0.8));
        assert!(logged[0].unresolved.is_empty());
        assert_eq!(
            run_of(&connection, logged[0].run_id),
            ("reclassify".to_string(), "ollama".to_string(), Some("unclassified".to_string()))
        );
    }

    #[test]
    fn an_unchanged_product_records_nothing_and_leaves_no_empty_run() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7002");
        apply_extraction(&mut connection, "7002", &outfit_for(&["ライム"], 0.8), 0.6).unwrap();
        let runs = run_count(&connection);
        let mut run = ClassificationRun::new(KIND_RECLASSIFY, "apple");
        apply_extraction_in_run(&mut connection, &mut run, "7002", &outfit_for(&["ライム"], 0.8), 0.6)
            .unwrap();
        assert_eq!(entries(&connection, "7002").len(), 1);
        assert_eq!(run_count(&connection), runs);
    }

    #[test]
    fn unresolved_names_land_in_the_entry() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7003");
        apply_extraction(&mut connection, "7003", &outfit_for(&["架空ちゃん"], 0.8), 0.6).unwrap();
        let logged = entries(&connection, "7003");
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].unresolved, vec!["架空ちゃん".to_string()]);
        assert_eq!(
            run_of(&connection, logged[0].run_id).0,
            KIND_IMPLICIT,
            "a call with no entry point says so"
        );
    }

    #[test]
    fn a_change_to_unresolved_names_alone_is_recorded() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7004");
        apply_extraction(&mut connection, "7004", &outfit_for(&["ライム"], 0.8), 0.6).unwrap();
        apply_extraction(&mut connection, "7004", &outfit_for(&["ライム", "架空ちゃん"], 0.8), 0.6)
            .unwrap();
        let logged = entries(&connection, "7004");
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[1].before_category, logged[1].after_category);
        assert_eq!(logged[1].before_links, logged[1].after_links);
        assert_eq!(logged[1].unresolved, vec!["架空ちゃん".to_string()]);
    }

    #[test]
    fn one_run_keeps_one_entry_per_product() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7005");
        let mut run = ClassificationRun::new(KIND_RECLASSIFY, "ollama");
        apply_extraction_in_run(&mut connection, &mut run, "7005", &outfit_for(&["架空ちゃん"], 0.7), 0.6)
            .unwrap();
        apply_extraction_in_run(&mut connection, &mut run, "7005", &outfit_for(&["ライム"], 0.9), 0.6)
            .unwrap();
        let logged = entries(&connection, "7005");
        assert_eq!(logged.len(), 1, "the later step updates the same entry");
        assert_eq!(logged[0].before_category, None);
        assert_eq!(logged[0].after_links, vec![lime_id(&connection)]);
        assert_eq!(logged[0].confidence, Some(0.9));
        assert!(logged[0].unresolved.is_empty());
    }

    #[test]
    fn a_step_that_restores_the_first_state_removes_the_entry() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7006");
        apply_extraction(&mut connection, "7006", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let mut run = ClassificationRun::new(KIND_RECLASSIFY, "ollama");
        apply_extraction_in_run(&mut connection, &mut run, "7006", &outfit_for(&["架空ちゃん"], 0.9), 0.6)
            .unwrap();
        assert_eq!(entries(&connection, "7006").len(), 2);
        apply_extraction_in_run(&mut connection, &mut run, "7006", &outfit_for(&["ライム"], 0.9), 0.6)
            .unwrap();
        assert_eq!(entries(&connection, "7006").len(), 1);
    }

    #[test]
    fn import_records_listed_compatibility_under_one_run() {
        let mut connection = open_memory().unwrap();
        let body = serde_json::json!({"boothId": "7007", "title": "見本の小物", "description": LISTED})
            .to_string();
        import_export_json(&mut connection, &body).unwrap();
        let logged = entries(&connection, "7007");
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].before_category, None);
        assert!(logged[0].after_category.is_some());
        assert_eq!(logged[0].after_links, vec![lime_id(&connection)]);
        assert_eq!(run_of(&connection, logged[0].run_id).0, KIND_IMPORT);
        // Sending the same page again changes nothing.
        import_export_json(&mut connection, &body).unwrap();
        assert_eq!(entries(&connection, "7007").len(), 1);
    }

    #[test]
    fn a_description_change_on_import_logs_the_reset_from_the_real_starting_point() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7008");
        apply_extraction(&mut connection, "7008", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        import_export_json(
            &mut connection,
            &serde_json::json!({"boothId": "7008", "title": "見本の服", "description": "別の説明です。"})
                .to_string(),
        )
        .unwrap();
        let logged = entries(&connection, "7008");
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[1].before_category.as_deref(), Some("outfit"));
        assert_eq!(logged[1].before_links, vec![lime_id(&connection)]);
        assert_eq!(logged[1].after_category, None);
        assert!(logged[1].after_links.is_empty());
    }

    #[test]
    fn catalog_reapply_is_its_own_run() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7009");
        apply_extraction(&mut connection, "7009", &outfit_for(&["架空ちゃん"], 0.9), 0.6).unwrap();
        // Adding the avatar re-resolves under the edit run. The catalog button then finds nothing.
        let avatar = save_avatar(
            &mut connection,
            &AvatarInput {
                id: None,
                display_name: "架空ちゃん".into(),
                base_body: None,
                aliases: Vec::new(),
                notes: String::new(),
            },
        )
        .unwrap();
        let logged = entries(&connection, "7009");
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[1].after_links, vec![avatar.clone()]);
        assert!(logged[1].unresolved.is_empty());
        assert_eq!(run_of(&connection, logged[1].run_id).0, KIND_EDIT);
        reapply_catalog(&mut connection).unwrap();
        assert_eq!(entries(&connection, "7009").len(), 2);
        connection
            .execute("DELETE FROM product_avatars WHERE product_id = '7009'", [])
            .unwrap();
        reapply_catalog(&mut connection).unwrap();
        let logged = entries(&connection, "7009");
        assert_eq!(logged.len(), 3);
        assert!(logged[2].before_links.is_empty());
        assert_eq!(logged[2].after_links, vec![avatar]);
        assert_eq!(run_of(&connection, logged[2].run_id).0, KIND_CATALOG);
    }

    #[test]
    fn deleting_an_avatar_logs_the_links_it_removed() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7010");
        let avatar = save_avatar(
            &mut connection,
            &AvatarInput {
                id: None,
                display_name: "架空ちゃん".into(),
                base_body: None,
                aliases: Vec::new(),
                notes: String::new(),
            },
        )
        .unwrap();
        apply_extraction(&mut connection, "7010", &outfit_for(&["架空ちゃん"], 0.9), 0.6).unwrap();
        delete_avatar(&mut connection, &avatar).unwrap();
        let logged = entries(&connection, "7010");
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[1].before_links, vec![avatar]);
        assert!(logged[1].after_links.is_empty());
        assert_eq!(logged[1].unresolved, vec!["架空ちゃん".to_string()]);
    }

    #[test]
    fn a_public_detail_that_resets_a_product_is_logged() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7011");
        apply_extraction(&mut connection, "7011", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let mut run = ClassificationRun::new(KIND_BOOTH_DETAIL, CLASSIFIER_NONE);
        observe(&mut connection, &mut run, "7011", |connection| {
            connection
                .execute("UPDATE products SET category = NULL WHERE booth_id = '7011'", [])
                .sql()?;
            Ok(())
        })
        .unwrap();
        let logged = entries(&connection, "7011");
        assert_eq!(logged.len(), 2);
        assert_eq!(run_of(&connection, logged[1].run_id).1, "");
    }

    #[test]
    fn the_before_state_keeps_what_a_restore_needs() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7012");
        apply_extraction(&mut connection, "7012", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        apply_extraction(&mut connection, "7012", &outfit_for(&["架空ちゃん"], 0.7), 0.6).unwrap();
        let (category, links, detail): (Option<String>, String, String) = connection
            .query_row(
                "SELECT before_category, before_links, before_state FROM classification_entries
                 WHERE booth_id = '7012' ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let before = decode_before(category, &links, &detail).unwrap();
        let lime = lime_id(&connection);
        assert_eq!(before.category.as_deref(), Some("outfit"));
        assert_eq!(before.links, vec![lime.clone()]);
        assert_eq!(before.detail.compatible_avatars, vec!["ライム".to_string()]);
        assert_eq!(before.detail.confidence, Some(0.9));
        assert_eq!(before.detail.link_sources[&lime], "extracted");
        assert!(before.detail.extracted_at.is_some());
    }

    #[test]
    fn a_log_failure_does_not_fail_the_classification() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7013");
        connection
            .execute_batch("DROP TABLE classification_entries; DROP TABLE classification_runs;")
            .unwrap();
        let mut run = ClassificationRun::new(KIND_RECLASSIFY, "ollama");
        apply_extraction_in_run(&mut connection, &mut run, "7013", &outfit_for(&["ライム"], 0.9), 0.6)
            .unwrap();
        assert_eq!(
            require_product(&connection, "7013").unwrap().category,
            Some(Category::Outfit)
        );
        reapply_catalog(&mut connection).unwrap();
    }

    #[test]
    fn a_failed_step_still_returns_its_error_and_logs_what_it_changed() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7014");
        let mut run = ClassificationRun::new(KIND_RECLASSIFY, "ollama");
        let result: Result<(), String> = observe(&mut connection, &mut run, "7014", |connection| {
            connection
                .execute("UPDATE products SET category = 'hair' WHERE booth_id = '7014'", [])
                .sql()?;
            Err("途中で失敗".into())
        });
        assert_eq!(result.unwrap_err(), "途中で失敗");
        assert_eq!(entries(&connection, "7014").len(), 1);
    }

    fn latest_entry(connection: &Connection, booth_id: &str) -> i64 {
        connection
            .query_row(
                "SELECT id FROM classification_entries WHERE booth_id = ?1 ORDER BY id DESC LIMIT 1",
                params![booth_id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn blocked(connection: &Connection, booth_id: &str, entry_id: i64) -> Option<String> {
        search_entries(connection, booth_id, 50)
            .unwrap()
            .into_iter()
            .find(|entry| entry.id == entry_id)
            .unwrap()
            .undo_blocked
    }

    #[test]
    fn undo_restores_the_before_state_and_records_itself() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7101");
        apply_extraction(&mut connection, "7101", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let lime = lime_id(&connection);
        let original = require_product(&connection, "7101").unwrap();
        apply_extraction(
            &mut connection,
            "7101",
            &Extraction {
                category: Category::Hair,
                genre: None,
                compatible_avatars: vec!["架空ちゃん".to_string()],
                is_generic: false,
                confidence: 0.7,
            },
            0.6,
        )
        .unwrap();
        let entry_id = latest_entry(&connection, "7101");
        assert_eq!(blocked(&connection, "7101", entry_id), None);
        undo_entry(&mut connection, entry_id).unwrap();

        let restored = require_product(&connection, "7101").unwrap();
        assert_eq!(restored.category, original.category);
        assert_eq!(restored.genre, original.genre);
        assert_eq!(restored.compatible_avatars, original.compatible_avatars);
        assert!(restored.unresolved_names.is_empty());
        assert_eq!(restored.confidence, original.confidence);
        assert_eq!(restored.extracted_at, original.extracted_at);
        assert_eq!(link_ids(&connection, "7101").unwrap(), vec![lime.clone()]);

        let logged = entries(&connection, "7101");
        assert_eq!(logged.len(), 3);
        assert_eq!(logged[2].before_category.as_deref(), Some("hair"));
        assert_eq!(logged[2].after_category.as_deref(), Some("outfit"));
        assert_eq!(logged[2].after_links, vec![lime.clone()]);
        assert_eq!(run_of(&connection, logged[2].run_id).0, KIND_UNDO);

        // Re-resolve rebuilds links from the stored names, so they have to come back too.
        reresolve_all(&mut connection).unwrap();
        assert_eq!(link_ids(&connection, "7101").unwrap(), vec![lime]);
        assert_eq!(entries(&connection, "7101").len(), 3);
    }

    #[test]
    fn undo_refuses_an_entry_a_later_run_has_replaced() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7102");
        apply_extraction(&mut connection, "7102", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let older = latest_entry(&connection, "7102");
        apply_extraction(&mut connection, "7102", &outfit_for(&["架空ちゃん"], 0.9), 0.6).unwrap();
        let reason = blocked(&connection, "7102", older).unwrap();
        assert!(reason.contains("あとに分類が変わった"));
        let category = require_product(&connection, "7102").unwrap().category;
        assert_eq!(undo_entry(&mut connection, older).unwrap_err(), reason);
        assert_eq!(require_product(&connection, "7102").unwrap().category, category);
        assert!(entries(&connection, "7102").len() == 2, "a refused undo writes nothing");
    }

    #[test]
    fn undo_refuses_a_product_with_locked_links() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7103");
        apply_extraction(&mut connection, "7103", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let entry_id = latest_entry(&connection, "7103");
        let lime = lime_id(&connection);
        update_product(
            &mut connection,
            &ProductUpdate {
                booth_id: "7103".into(),
                category: None,
                genre: None,
                is_generic: None,
                avatar_ids: Some(vec![lime]),
            },
        )
        .unwrap();
        let reason = undo_entry(&mut connection, entry_id).unwrap_err();
        assert!(reason.contains("固定"));
        assert_eq!(blocked(&connection, "7103", entry_id), Some(reason));
    }

    #[test]
    fn undo_refuses_to_overwrite_a_manual_category() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7104");
        apply_extraction(&mut connection, "7104", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let entry_id = latest_entry(&connection, "7104");
        connection
            .execute("UPDATE products SET category_locked = 1 WHERE booth_id = '7104'", [])
            .unwrap();
        assert!(undo_entry(&mut connection, entry_id).unwrap_err().contains("カテゴリ"));
    }

    #[test]
    fn undo_runs_once_and_an_undo_entry_cannot_be_undone() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7105");
        apply_extraction(&mut connection, "7105", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let entry_id = latest_entry(&connection, "7105");
        undo_entry(&mut connection, entry_id).unwrap();
        assert!(undo_entry(&mut connection, entry_id).unwrap_err().contains("すでに"));
        let undo_id = latest_entry(&connection, "7105");
        assert_ne!(undo_id, entry_id);
        assert!(undo_entry(&mut connection, undo_id).unwrap_err().contains("Undo"));
        assert!(undo_entry(&mut connection, 999_999).is_err());
    }

    #[test]
    fn undo_refuses_when_the_original_avatar_is_gone() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7106");
        let avatar = save_avatar(
            &mut connection,
            &AvatarInput {
                id: None,
                display_name: "架空ちゃん".into(),
                base_body: None,
                aliases: Vec::new(),
                notes: String::new(),
            },
        )
        .unwrap();
        apply_extraction(&mut connection, "7106", &outfit_for(&["架空ちゃん"], 0.9), 0.6).unwrap();
        apply_extraction(&mut connection, "7106", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        let entry_id = latest_entry(&connection, "7106");
        connection
            .execute("DELETE FROM avatars WHERE id = ?1", params![avatar])
            .unwrap();
        assert!(undo_entry(&mut connection, entry_id).unwrap_err().contains("消えている"));
    }

    #[test]
    fn undoing_a_registered_body_releases_ownership_and_the_placeholder() {
        let mut connection = open_memory().unwrap();
        import_export_json(
            &mut connection,
            r#"{"boothId":"7107","title":"九尾オリジナル3Dモデル「輝夜」-kaguya-","description":"見本です。"}"#,
        )
        .unwrap();
        let avatar = link_ids(&connection, "7107").unwrap().remove(0);
        assert!(avatar_is_owned(&connection, &avatar).unwrap());
        let entry_id = latest_entry(&connection, "7107");
        undo_entry(&mut connection, entry_id).unwrap();
        let product = require_product(&connection, "7107").unwrap();
        assert_eq!(product.category, None);
        assert!(!product.user_confirmed);
        assert!(link_ids(&connection, "7107").unwrap().is_empty());
        assert!(!avatar_exists(&connection, &avatar).unwrap());
    }

    #[test]
    fn views_name_avatars_and_page_runs_newest_first() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7108");
        apply_extraction(&mut connection, "7108", &outfit_for(&["ライム"], 0.9), 0.6).unwrap();
        apply_extraction(&mut connection, "7108", &outfit_for(&["架空ちゃん"], 0.9), 0.6).unwrap();
        let page = run_page(&connection, 0, 1).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.runs.len(), 1);
        assert_eq!(page.runs[0].changed_count, 1);
        let newest = run_entries(&connection, page.runs[0].id, 0, 50).unwrap();
        assert_eq!(newest.len(), 1);
        assert_eq!(newest[0].title.as_deref(), Some("見本の服"));
        assert_eq!(newest[0].before_links[0].name, "ライム");
        assert!(newest[0].after_links.is_empty());
        assert_eq!(newest[0].unresolved_names, vec!["架空ちゃん".to_string()]);
        assert_eq!(search_entries(&connection, "見本", 50).unwrap().len(), 2);
        assert_eq!(search_entries(&connection, "7108", 50).unwrap().len(), 2);
        assert!(search_entries(&connection, "71", 50).unwrap().is_empty());
        assert!(search_entries(&connection, "  ", 50).unwrap().is_empty());
    }

    #[test]
    fn unresolved_names_count_products_and_drop_what_resolves_now() {
        let mut connection = open_memory().unwrap();
        product(&mut connection, "7109");
        product(&mut connection, "7110");
        product(&mut connection, "7111");
        apply_extraction(&mut connection, "7109", &outfit_for(&["架空ちゃん"], 0.9), 0.6).unwrap();
        // A second entry for the same product is still one product.
        apply_extraction(&mut connection, "7109", &outfit_for(&["架空ちゃん", "別人"], 0.9), 0.6)
            .unwrap();
        apply_extraction(&mut connection, "7110", &outfit_for(&["架空ちゃん"], 0.9), 0.6).unwrap();
        apply_extraction(&mut connection, "7111", &outfit_for(&["取り消す名"], 0.9), 0.6).unwrap();
        let entry_id = latest_entry(&connection, "7111");
        undo_entry(&mut connection, entry_id).unwrap();
        assert_eq!(
            unresolved_name_counts(&connection).unwrap(),
            vec![
                UnresolvedNameCount { name: "架空ちゃん".into(), product_count: 2 },
                UnresolvedNameCount { name: "別人".into(), product_count: 1 },
            ]
        );
        save_avatar(
            &mut connection,
            &AvatarInput {
                id: None,
                display_name: "別人".into(),
                base_body: None,
                aliases: Vec::new(),
                notes: String::new(),
            },
        )
        .unwrap();
        let names: Vec<String> = unresolved_name_counts(&connection)
            .unwrap()
            .into_iter()
            .map(|item| item.name)
            .collect();
        assert_eq!(names, vec!["架空ちゃん".to_string()]);
    }
}
