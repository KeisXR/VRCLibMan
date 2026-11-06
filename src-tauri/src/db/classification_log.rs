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

use super::{Sql, parse_list, to_json};
use crate::model::now_iso;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

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
/// `extract_products`: the model and the BOOTH category rules, plus the batch-end re-resolve.
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
        assert_eq!(version, 10);
        assert_eq!(run_count(&connection), 0);
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
        assert_eq!(version, 10);
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
}
