use crate::import_data::ParsedExport;
use crate::matching::{choose_match, score_label, LabelScore};
use crate::model::{
    now_iso, Avatar, AvatarInput, BaseBody, BaseBodyInput, Category, Extraction, ImportReport,
    IncomingProduct, Knowledge, Link, LocalFile, Product, ProductUpdate, Settings, SettingsInput,
};
use crate::resolve::{resolve_product, ResolveInput};
use crate::scan::ScanOutcome;
use crate::view::Snapshot;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE products (
    booth_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    shop TEXT NOT NULL DEFAULT '',
    description_snippet TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL DEFAULT '',
    thumbnail_url TEXT,
    thumbnail_path TEXT,
    category TEXT,
    category_locked INTEGER NOT NULL DEFAULT 0,
    compatible_avatars_json TEXT NOT NULL DEFAULT '[]',
    unresolved_names_json TEXT NOT NULL DEFAULT '[]',
    is_generic INTEGER NOT NULL DEFAULT 0,
    confidence REAL,
    links_locked INTEGER NOT NULL DEFAULT 0,
    user_confirmed INTEGER NOT NULL DEFAULT 0,
    needs_review INTEGER NOT NULL DEFAULT 1,
    imported_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    extracted_at TEXT
);
CREATE TABLE local_files (
    path TEXT PRIMARY KEY,
    filename TEXT NOT NULL,
    is_dir INTEGER NOT NULL DEFAULT 0,
    size INTEGER NOT NULL DEFAULT 0,
    linked_product_id TEXT,
    candidate_product_id TEXT,
    match_score REAL NOT NULL DEFAULT 0,
    link_source TEXT,
    scanned_at TEXT NOT NULL,
    FOREIGN KEY (linked_product_id) REFERENCES products(booth_id) ON DELETE SET NULL
);
CREATE TABLE base_bodies (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT ''
);
CREATE TABLE avatars (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    base_body TEXT,
    aliases_json TEXT NOT NULL DEFAULT '[]',
    is_builtin INTEGER NOT NULL DEFAULT 0,
    notes TEXT NOT NULL DEFAULT ''
);
CREATE TABLE product_avatars (
    product_id TEXT NOT NULL,
    avatar_id TEXT NOT NULL,
    source TEXT NOT NULL,
    PRIMARY KEY (product_id, avatar_id),
    FOREIGN KEY (product_id) REFERENCES products(booth_id) ON DELETE CASCADE,
    FOREIGN KEY (avatar_id) REFERENCES avatars(id) ON DELETE CASCADE
);
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE INDEX idx_links_avatar ON product_avatars(avatar_id);
CREATE INDEX idx_files_product ON local_files(linked_product_id);
CREATE INDEX idx_products_review ON products(needs_review);
";

pub(crate) struct ExtractJob {
    pub booth_id: String,
    pub title: String,
    pub description: String,
}

pub(crate) struct ThumbJob {
    pub booth_id: String,
    pub thumbnail_url: String,
    pub thumbnail_path: Option<String>,
}

trait Sql<T> {
    fn sql(self) -> Result<T, String>;
}

impl<T> Sql<T> for Result<T, rusqlite::Error> {
    fn sql(self) -> Result<T, String> {
        self.map_err(|err| err.to_string())
    }
}

pub(crate) fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("保存先を作れません: {err}"))?;
    }
    let connection = Connection::open(path)
        .map_err(|err| format!("データベースを開けません: {err}"))?;
    finish_open(connection, true)
}

#[cfg(test)]
fn open_memory() -> Result<Connection, String> {
    finish_open(Connection::open_in_memory().sql()?, false)
}

fn finish_open(mut connection: Connection, wal: bool) -> Result<Connection, String> {
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
        .sql()?;
    if wal {
        let _ = connection.execute_batch("PRAGMA journal_mode = WAL;");
    }
    migrate(&mut connection)?;
    ensure_ready(&mut connection)?;
    Ok(connection)
}

fn migrate(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .sql()?;
    if version == 0 {
        connection.execute_batch(SCHEMA).sql()?;
        connection
            .execute_batch("PRAGMA user_version = 1;")
            .sql()?;
    }
    Ok(())
}

fn ensure_ready(connection: &Connection) -> Result<(), String> {
    for (key, value) in [
        ("ollama_base_url", "http://127.0.0.1:11434"),
        ("ollama_model", "qwen3:4b"),
        ("confidence_threshold", "0.6"),
        ("library_folder", ""),
    ] {
        connection
            .execute(
                "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .sql()?;
    }
    if setting(connection, "initialized")?.as_deref() != Some("1") {
        merge_seed(connection)?;
        set_setting(connection, "initialized", "1")?;
    }
    Ok(())
}

pub(crate) fn settings(connection: &Connection) -> Result<Settings, String> {
    let threshold = setting(connection, "confidence_threshold")?
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.6)
        .clamp(0.0, 1.0);
    Ok(Settings {
        ollama_base_url: setting(connection, "ollama_base_url")?
            .unwrap_or_else(|| "http://127.0.0.1:11434".into()),
        ollama_model: setting(connection, "ollama_model")?.unwrap_or_else(|| "qwen3:4b".into()),
        confidence_threshold: threshold,
        library_folder: setting(connection, "library_folder")?.unwrap_or_default(),
    })
}

pub(crate) fn save_settings(connection: &Connection, input: &SettingsInput) -> Result<(), String> {
    set_setting(connection, "ollama_base_url", input.ollama_base_url.trim())?;
    set_setting(connection, "ollama_model", input.ollama_model.trim())?;
    set_setting(
        connection,
        "confidence_threshold",
        &input.confidence_threshold.clamp(0.0, 1.0).to_string(),
    )?;
    set_setting(connection, "library_folder", input.library_folder.trim())?;
    Ok(())
}

pub(crate) fn set_library_folder(connection: &Connection, path: &str) -> Result<(), String> {
    set_setting(connection, "library_folder", path)
}

fn setting(connection: &Connection, key: &str) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .sql()
}

fn set_setting(connection: &Connection, key: &str, value: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .sql()?;
    Ok(())
}

pub(crate) fn import_products(
    connection: &mut Connection,
    parsed: &ParsedExport,
) -> Result<ImportReport, String> {
    let tx = connection.transaction().sql()?;
    let mut inserted = 0;
    let mut updated = 0;
    let mut unchanged = 0;
    for product in &parsed.products {
        match upsert(&tx, product)? {
            Upsert::Inserted => inserted += 1,
            Upsert::Updated => updated += 1,
            Upsert::Unchanged => unchanged += 1,
        }
    }
    tx.commit().sql()?;
    Ok(ImportReport {
        inserted,
        updated,
        unchanged,
        skipped: parsed.skipped,
        warnings: parsed.warnings.clone(),
    })
}

enum Upsert {
    Inserted,
    Updated,
    Unchanged,
}

fn upsert(connection: &Connection, incoming: &IncomingProduct) -> Result<Upsert, String> {
    let now = now_iso();
    if let Some(existing) = product_by_id(connection, &incoming.booth_id)? {
        let description = if incoming.description_snippet.is_empty() {
            existing.description_snippet.clone()
        } else {
            incoming.description_snippet.clone()
        };
        let description_changed = description != existing.description_snippet;
        let title = if incoming.title.is_empty() {
            existing.title.clone()
        } else {
            incoming.title.clone()
        };
        let shop = if incoming.shop.is_empty() {
            existing.shop.clone()
        } else {
            incoming.shop.clone()
        };
        let url = if incoming.url.is_empty() {
            existing.url.clone()
        } else {
            incoming.url.clone()
        };
        let thumbnail_changed =
            incoming.thumbnail_url.is_some() && incoming.thumbnail_url != existing.thumbnail_url;
        let thumbnail = if incoming.thumbnail_url.is_some() {
            incoming.thumbnail_url.clone()
        } else {
            existing.thumbnail_url.clone()
        };
        let thumbnail_path = if thumbnail_changed {
            None
        } else {
            existing.thumbnail_path.clone()
        };
        if !description_changed
            && title == existing.title
            && shop == existing.shop
            && url == existing.url
            && thumbnail == existing.thumbnail_url
        {
            return Ok(Upsert::Unchanged);
        }
        if description_changed {
            let category = if existing.category_locked {
                existing.category.map(|category| category.as_str().to_string())
            } else {
                None
            };
            connection
                .execute(
                    "UPDATE products SET title = ?1, shop = ?2, description_snippet = ?3, url = ?4,
                     thumbnail_url = ?5, thumbnail_path = ?6, category = ?7,
                     compatible_avatars_json = '[]', unresolved_names_json = '[]',
                     is_generic = 0, confidence = NULL, user_confirmed = 0, needs_review = 1,
                     extracted_at = NULL, updated_at = ?8 WHERE booth_id = ?9",
                    params![
                        title,
                        shop,
                        description,
                        url,
                        thumbnail,
                        thumbnail_path,
                        category,
                        now,
                        existing.booth_id
                    ],
                )
                .sql()?;
            if !existing.links_locked {
                delete_links(connection, &existing.booth_id)?;
            }
        } else {
            connection
                .execute(
                    "UPDATE products SET title = ?1, shop = ?2, url = ?3, thumbnail_url = ?4,
                     thumbnail_path = ?5, updated_at = ?6 WHERE booth_id = ?7",
                    params![
                        title,
                        shop,
                        url,
                        thumbnail,
                        thumbnail_path,
                        now,
                        existing.booth_id
                    ],
                )
                .sql()?;
        }
        return Ok(Upsert::Updated);
    }

    connection
        .execute(
            "INSERT INTO products (
                booth_id, title, shop, description_snippet, url, thumbnail_url, thumbnail_path,
                category, category_locked, compatible_avatars_json, unresolved_names_json,
                is_generic, confidence, links_locked, user_confirmed, needs_review,
                imported_at, updated_at, extracted_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, 0, '[]', '[]', 0, NULL, 0, 0, 1, ?7, ?7, NULL)",
            params![
                incoming.booth_id,
                incoming.title,
                incoming.shop,
                incoming.description_snippet,
                incoming.url,
                incoming.thumbnail_url,
                now
            ],
        )
        .sql()?;
    Ok(Upsert::Inserted)
}

pub(crate) fn apply_extraction(
    connection: &mut Connection,
    booth_id: &str,
    extraction: &Extraction,
    threshold: f64,
) -> Result<(), String> {
    let existing = require_product(connection, booth_id)?;
    let avatars = load_avatars(connection)?;
    let category = if existing.category_locked {
        existing.category
    } else {
        Some(extraction.category)
    };
    let output = resolve_product(
        &ResolveInput {
            booth_id: existing.booth_id.as_str(),
            title: existing.title.as_str(),
            category,
            raw_names: &extraction.compatible_avatars,
            is_generic: extraction.is_generic,
            confidence: Some(extraction.confidence),
            user_confirmed: false,
            threshold,
            allow_create: !existing.links_locked,
        },
        &avatars,
    );
    let needs_review = if existing.links_locked {
        category.is_none()
            || extraction.confidence < threshold
            || (category != Some(Category::Avatar) && !output.unresolved.is_empty())
    } else {
        output.needs_review
    };
    let unresolved = if existing.links_locked && category == Some(Category::Avatar) {
        Vec::new()
    } else {
        output.unresolved.clone()
    };
    let now = now_iso();
    let category_text = category.map(|item| item.as_str().to_string());
    let tx = connection.transaction().sql()?;
    if !existing.links_locked {
        for draft in &output.created_avatars {
            insert_placeholder(&tx, &draft.id, &draft.display_name)?;
        }
    }
    tx.execute(
        "UPDATE products SET category = ?1, compatible_avatars_json = ?2, unresolved_names_json = ?3,
         is_generic = ?4, confidence = ?5, needs_review = ?6, user_confirmed = 0,
         extracted_at = ?7, updated_at = ?8 WHERE booth_id = ?9",
        params![
            category_text,
            to_json(&extraction.compatible_avatars)?,
            to_json(&unresolved)?,
            extraction.is_generic,
            extraction.confidence,
            needs_review,
            now,
            now,
            booth_id
        ],
    )
    .sql()?;
    if !existing.links_locked {
        replace_links(&tx, booth_id, &output.avatar_ids, "extracted")?;
    }
    tx.commit().sql()?;
    Ok(())
}

pub(crate) fn reresolve_all(connection: &mut Connection) -> Result<u32, String> {
    let threshold = settings(connection)?.confidence_threshold;
    let products = load_products(connection)?;
    let tx = connection.transaction().sql()?;
    let mut processed = 0;
    for product in products {
        if product.links_locked || (product.category.is_none() && product.extracted_at.is_none()) {
            continue;
        }
        let avatars = load_avatars(&tx)?;
        let mut output = resolve_product(
            &ResolveInput {
                booth_id: product.booth_id.as_str(),
                title: product.title.as_str(),
                category: product.category,
                raw_names: &product.compatible_avatars,
                is_generic: product.is_generic,
                confidence: product.confidence,
                user_confirmed: product.user_confirmed,
                threshold,
                allow_create: true,
            },
            &avatars,
        );
        let reopen = output.unresolved != product.unresolved_names && !output.unresolved.is_empty();
        if reopen {
            output.needs_review = true;
        }
        for draft in &output.created_avatars {
            insert_placeholder(&tx, &draft.id, &draft.display_name)?;
        }
        let confirmed = if reopen { false } else { product.user_confirmed };
        tx.execute(
            "UPDATE products SET unresolved_names_json = ?1, needs_review = ?2, user_confirmed = ?3, updated_at = ?4 WHERE booth_id = ?5",
            params![
                to_json(&output.unresolved)?,
                output.needs_review,
                confirmed,
                now_iso(),
                product.booth_id
            ],
        )
        .sql()?;
        replace_links(&tx, &product.booth_id, &output.avatar_ids, "extracted")?;
        processed += 1;
    }
    tx.execute(
        "DELETE FROM avatars WHERE id LIKE 'owned-%' AND id NOT IN (SELECT avatar_id FROM product_avatars)",
        [],
    )
    .sql()?;
    tx.commit().sql()?;
    Ok(processed)
}

pub(crate) fn update_product(connection: &mut Connection, update: &ProductUpdate) -> Result<(), String> {
    let mut product = require_product(connection, &update.booth_id)?;
    let threshold = settings(connection)?.confidence_threshold;
    if let Some(category) = &update.category {
        product.category = Some(Category::parse(category).ok_or("カテゴリが不正です")?);
        product.category_locked = true;
    }
    if let Some(generic) = update.is_generic {
        product.is_generic = generic;
    }
    let mut replacement = None;
    if let Some(ids) = &update.avatar_ids {
        for id in ids {
            if !avatar_exists(connection, id)? {
                return Err(format!("アバター {id} が対応表にありません"));
            }
        }
        product.links_locked = true;
        product.unresolved_names.clear();
        replacement = Some(ids.clone());
    }
    if let Some(confirm) = update.confirm {
        product.user_confirmed = confirm;
    }
    let link_count = if let Some(ids) = &replacement {
        ids.len()
    } else {
        count_links(connection, &product.booth_id)?
    };
    product.needs_review = needs_review_for(&product, link_count, threshold);
    let category = product.category.map(|category| category.as_str().to_string());
    let tx = connection.transaction().sql()?;
    tx.execute(
        "UPDATE products SET category = ?1, category_locked = ?2, is_generic = ?3,
         unresolved_names_json = ?4, links_locked = ?5, user_confirmed = ?6, needs_review = ?7,
         updated_at = ?8 WHERE booth_id = ?9",
        params![
            category,
            product.category_locked,
            product.is_generic,
            to_json(&product.unresolved_names)?,
            product.links_locked,
            product.user_confirmed,
            product.needs_review,
            now_iso(),
            product.booth_id
        ],
    )
    .sql()?;
    if let Some(ids) = replacement {
        replace_links(&tx, &product.booth_id, &ids, "manual")?;
    }
    tx.commit().sql()?;
    Ok(())
}

pub(crate) fn link_file(
    connection: &Connection,
    path: &str,
    product_id: Option<&str>,
) -> Result<(), String> {
    let found: Option<String> = connection
        .query_row(
            "SELECT path FROM local_files WHERE path = ?1",
            params![path],
            |row| row.get(0),
        )
        .optional()
        .sql()?;
    if found.is_none() {
        return Err("ファイルがスキャン結果にありません".into());
    }
    if let Some(product_id) = product_id {
        let product = require_product(connection, product_id)?;
        connection
            .execute(
                "UPDATE local_files SET linked_product_id = ?1, candidate_product_id = ?1,
                 match_score = 1, link_source = 'manual' WHERE path = ?2",
                params![product.booth_id, path],
            )
            .sql()?;
    } else {
        connection
            .execute(
                "UPDATE local_files SET linked_product_id = NULL, candidate_product_id = NULL,
                 match_score = 0, link_source = NULL WHERE path = ?1",
                params![path],
            )
            .sql()?;
    }
    Ok(())
}

pub(crate) fn apply_scan(
    connection: &mut Connection,
    root: &str,
    outcome: &ScanOutcome,
) -> Result<crate::model::ScanReport, String> {
    let products = load_products(connection)?;
    let tx = connection.transaction().sql()?;
    tx.execute(
        "DELETE FROM local_files WHERE link_source IS NULL OR link_source != 'manual'",
        [],
    )
    .sql()?;
    let now = now_iso();
    let mut linked = 0u32;
    let mut seen = HashSet::new();
    for entry in &outcome.entries {
        seen.insert(normalize_path_key(&entry.path));
        let source: Option<String> = tx
            .query_row(
                "SELECT link_source FROM local_files WHERE path = ?1",
                params![entry.path],
                |row| row.get(0),
            )
            .optional()
            .sql()?;
        if source.as_deref() == Some("manual") {
            tx.execute(
                "UPDATE local_files SET filename = ?1, is_dir = ?2, size = ?3, scanned_at = ?4 WHERE path = ?5",
                params![entry.filename, entry.is_dir, entry.size as i64, now, entry.path],
            )
            .sql()?;
            continue;
        }
        let mut scores = Vec::new();
        for product in &products {
            let mut best = 0.0;
            let mut id_hit = false;
            for label in &entry.labels {
                let (score, hit) = score_label(label, &product.title, &product.booth_id);
                if score > best {
                    best = score;
                    id_hit = hit;
                }
            }
            if best > 0.0 {
                scores.push(LabelScore {
                    product_id: product.booth_id.clone(),
                    score: best,
                    id_hit,
                });
            }
        }
        let chosen = choose_match(&scores);
        let (linked_id, candidate, score, link_source) = if let Some((id, score)) = &chosen {
            (Some(id.clone()), Some(id.clone()), *score, Some("auto".to_string()))
        } else if let Some(top) = scores
            .iter()
            .max_by(|left, right| left.score.partial_cmp(&right.score).unwrap_or(std::cmp::Ordering::Equal))
        {
            if top.score >= 0.5 {
                (None, Some(top.product_id.clone()), top.score, None)
            } else {
                (None, None, top.score, None)
            }
        } else {
            (None, None, 0.0, None)
        };
        if linked_id.is_some() {
            linked += 1;
        }
        tx.execute(
            "INSERT INTO local_files (
                path, filename, is_dir, size, linked_product_id, candidate_product_id,
                match_score, link_source, scanned_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.path,
                entry.filename,
                entry.is_dir,
                entry.size as i64,
                linked_id,
                candidate,
                score,
                link_source,
                now
            ],
        )
        .sql()?;
    }
    if !outcome.truncated {
        let mut statement = tx.prepare("SELECT path FROM local_files").sql()?;
        let paths: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .sql()?
            .collect::<Result<Vec<_>, _>>()
            .sql()?;
        drop(statement);
        for path in paths {
            if is_under(&path, root) && !seen.contains(&normalize_path_key(&path)) {
                tx.execute("DELETE FROM local_files WHERE path = ?1", params![path])
                    .sql()?;
            }
        }
    }
    tx.commit().sql()?;
    Ok(crate::model::ScanReport {
        indexed: outcome.entries.len() as u32,
        linked,
        truncated: outcome.truncated,
        root: root.to_string(),
    })
}

pub(crate) fn products_for_extract(
    connection: &Connection,
    force: bool,
) -> Result<Vec<ExtractJob>, String> {
    let sql = if force {
        "SELECT booth_id, title, description_snippet FROM products ORDER BY updated_at DESC"
    } else {
        "SELECT booth_id, title, description_snippet FROM products WHERE extracted_at IS NULL ORDER BY imported_at ASC"
    };
    let mut statement = connection.prepare(sql).sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok(ExtractJob {
                booth_id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(crate) fn thumbnail_jobs(connection: &Connection) -> Result<Vec<ThumbJob>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id, thumbnail_url, thumbnail_path FROM products
             WHERE thumbnail_url IS NOT NULL AND thumbnail_url != ''",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok(ThumbJob {
                booth_id: row.get(0)?,
                thumbnail_url: row.get(1)?,
                thumbnail_path: row.get(2)?,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(crate) fn set_thumbnail_path(
    connection: &Connection,
    booth_id: &str,
    path: &Path,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE products SET thumbnail_path = ?1 WHERE booth_id = ?2",
            params![path.display().to_string(), booth_id],
        )
        .sql()?;
    Ok(())
}

pub(crate) fn load_snapshot(connection: &Connection) -> Result<Snapshot, String> {
    Ok(Snapshot {
        products: load_products(connection)?,
        files: load_files(connection)?,
        avatars: load_avatars(connection)?,
        base_bodies: load_bases(connection)?,
        links: load_links(connection)?,
    })
}

pub(crate) fn knowledge(connection: &Connection) -> Result<Knowledge, String> {
    Ok(Knowledge {
        avatars: load_avatars(connection)?,
        base_bodies: load_bases(connection)?,
    })
}

pub(crate) fn save_avatar(connection: &mut Connection, input: &AvatarInput) -> Result<String, String> {
    let display = input.display_name.trim();
    if display.is_empty() || display.chars().count() > 80 {
        return Err("表示名は1〜80文字にしてください".into());
    }
    let aliases = clean_aliases(&input.aliases, display);
    let notes: String = input.notes.chars().take(400).collect();
    let base = normalize_base(connection, input.base_body.as_deref())?;
    let requested = input.id.as_deref().map(str::trim).filter(|id| !id.is_empty());
    let id = if let Some(id) = requested {
        if !valid_id(id) {
            return Err("IDは英小文字、数字、_、- で64文字以内にしてください".into());
        }
        if id.starts_with("owned-") && !avatar_exists(connection, id)? {
            return Err("このIDは自動作成用に予約されています".into());
        }
        id.to_string()
    } else {
        generate_id(connection, display)?
    };
    let builtin = avatar_by_id(connection, &id)?
        .map(|avatar| avatar.is_builtin)
        .unwrap_or(false);
    connection
        .execute(
            "INSERT INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
               display_name = excluded.display_name,
               base_body = excluded.base_body,
               aliases_json = excluded.aliases_json,
               notes = excluded.notes",
            params![id, display, base, to_json(&aliases)?, builtin, notes],
        )
        .sql()?;
    reresolve_all(connection)?;
    Ok(id)
}

pub(crate) fn delete_avatar(connection: &mut Connection, id: &str) -> Result<(), String> {
    let avatar = avatar_by_id(connection, id)?.ok_or("アバターが見つかりません")?;
    if avatar.is_builtin {
        return Err("初期データのアバターは削除できません".into());
    }
    connection
        .execute(
            "DELETE FROM product_avatars WHERE avatar_id = ?1",
            params![id],
        )
        .sql()?;
    connection
        .execute("DELETE FROM avatars WHERE id = ?1", params![id])
        .sql()?;
    reresolve_all(connection)?;
    Ok(())
}

pub(crate) fn save_base_body(connection: &Connection, input: &BaseBodyInput) -> Result<String, String> {
    let display = input.display_name.trim();
    if display.is_empty() || display.chars().count() > 80 {
        return Err("素体名は1〜80文字にしてください".into());
    }
    let note: String = input.note.chars().take(400).collect();
    let requested = input.id.as_deref().map(str::trim).filter(|id| !id.is_empty());
    let id = if let Some(id) = requested {
        if !valid_id(id) {
            return Err("IDは英小文字、数字、_、- で64文字以内にしてください".into());
        }
        id.to_string()
    } else {
        generate_id(connection, display)?
    };
    connection
        .execute(
            "INSERT INTO base_bodies (id, display_name, note) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name, note = excluded.note",
            params![id, display, note],
        )
        .sql()?;
    Ok(id)
}

pub(crate) fn delete_base_body(connection: &Connection, id: &str) -> Result<(), String> {
    connection
        .execute(
            "UPDATE avatars SET base_body = NULL WHERE base_body = ?1",
            params![id],
        )
        .sql()?;
    let rows = connection
        .execute("DELETE FROM base_bodies WHERE id = ?1", params![id])
        .sql()?;
    if rows == 0 {
        return Err("素体が見つかりません".into());
    }
    Ok(())
}

pub(crate) fn merge_avatars(connection: &mut Connection, from_id: &str, into_id: &str) -> Result<(), String> {
    if from_id == into_id {
        return Err("同じアバターには統合できません".into());
    }
    let source = avatar_by_id(connection, from_id)?.ok_or("統合元のアバターが見つかりません")?;
    let mut target = avatar_by_id(connection, into_id)?.ok_or("統合先のアバターが見つかりません")?;
    if source.is_builtin {
        return Err("初期データのアバターは統合で消せません。別名を編集してください".into());
    }
    let mut aliases = target.aliases.clone();
    for alias in std::iter::once(source.display_name.clone()).chain(source.aliases) {
        if alias != target.display_name && !aliases.iter().any(|existing| existing == &alias) {
            aliases.push(alias);
        }
    }
    aliases.truncate(32);
    target.aliases = aliases;
    let tx = connection.transaction().sql()?;
    tx.execute(
        "UPDATE avatars SET aliases_json = ?1 WHERE id = ?2",
        params![to_json(&target.aliases)?, into_id],
    )
    .sql()?;
    let mut statement = tx
        .prepare(
            "SELECT l.product_id FROM product_avatars l
             JOIN products p ON p.booth_id = l.product_id
             WHERE l.avatar_id = ?1 AND p.links_locked = 1",
        )
        .sql()?;
    let locked: Vec<String> = statement
        .query_map(params![from_id], |row| row.get(0))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    drop(statement);
    tx.execute(
        "DELETE FROM product_avatars WHERE avatar_id = ?1",
        params![from_id],
    )
    .sql()?;
    for product_id in locked {
        tx.execute(
            "INSERT OR IGNORE INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, 'manual')",
            params![product_id, into_id],
        )
        .sql()?;
    }
    tx.execute("DELETE FROM avatars WHERE id = ?1", params![from_id])
        .sql()?;
    tx.commit().sql()?;
    reresolve_all(connection)?;
    Ok(())
}

pub(crate) fn restore_seed(connection: &mut Connection) -> Result<u32, String> {
    let added = merge_seed(connection)?;
    reresolve_all(connection)?;
    Ok(added)
}

fn merge_seed(connection: &Connection) -> Result<u32, String> {
    let seed: SeedFile = serde_json::from_str(include_str!("../knowledge/seed.json"))
        .map_err(|err| format!("初期データを読めません: {err}"))?;
    for body in seed.base_bodies {
        connection
            .execute(
                "INSERT OR IGNORE INTO base_bodies (id, display_name, note) VALUES (?1, ?2, ?3)",
                params![body.id, body.display_name, body.note],
            )
            .sql()?;
    }
    let mut added = 0;
    for avatar in seed.avatars {
        let rows = connection
            .execute(
                "INSERT OR IGNORE INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
                 VALUES (?1, ?2, ?3, ?4, 1, '')",
                params![
                    avatar.id,
                    avatar.display_name,
                    avatar.base_body,
                    to_json(&avatar.aliases)?
                ],
            )
            .sql()?;
        added += rows as u32;
    }
    Ok(added)
}

fn needs_review_for(product: &Product, link_count: usize, threshold: f64) -> bool {
    if product.user_confirmed || product.category.is_none() {
        return product.category.is_none() && !product.user_confirmed;
    }
    if product.extracted_at.is_some() && product.confidence.unwrap_or(0.0) < threshold {
        return true;
    }
    if product.category != Some(Category::Avatar) && !product.unresolved_names.is_empty() {
        return true;
    }
    product.category != Some(Category::Avatar) && !product.is_generic && link_count == 0
}

fn insert_placeholder(connection: &Connection, id: &str, display_name: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
             VALUES (?1, ?2, NULL, '[]', 0, '')",
            params![id, display_name],
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

fn count_links(connection: &Connection, product_id: &str) -> Result<usize, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_avatars WHERE product_id = ?1",
            params![product_id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(count as usize)
}

fn require_product(connection: &Connection, booth_id: &str) -> Result<Product, String> {
    product_by_id(connection, booth_id)?.ok_or_else(|| "商品が見つかりません".into())
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
    let mut statement = connection.prepare(PRODUCT_SELECT).sql()?;
    let rows = statement.query_map([], map_product).sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn load_avatars(connection: &Connection) -> Result<Vec<Avatar>, String> {
    let mut statement = connection
        .prepare("SELECT id, display_name, base_body, aliases_json, is_builtin, notes FROM avatars")
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
                notes: row.get(5)?,
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
       category, category_locked, compatible_avatars_json, unresolved_names_json,
       is_generic, confidence, links_locked, user_confirmed, needs_review,
       imported_at, updated_at, extracted_at
FROM products
";

fn map_product(row: &rusqlite::Row<'_>) -> rusqlite::Result<Product> {
    let category: Option<String> = row.get(7)?;
    let compatible: String = row.get(9)?;
    let unresolved: String = row.get(10)?;
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
        compatible_avatars: parse_list(&compatible),
        unresolved_names: parse_list(&unresolved),
        is_generic: row.get::<_, i64>(11)? != 0,
        confidence: row.get(12)?,
        links_locked: row.get::<_, i64>(13)? != 0,
        user_confirmed: row.get::<_, i64>(14)? != 0,
        needs_review: row.get::<_, i64>(15)? != 0,
        imported_at: row.get(16)?,
        updated_at: row.get(17)?,
        extracted_at: row.get(18)?,
    })
}

fn avatar_by_id(connection: &Connection, id: &str) -> Result<Option<Avatar>, String> {
    let avatars = load_avatars(connection)?;
    Ok(avatars.into_iter().find(|avatar| avatar.id == id))
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
    if !base.starts_with("owned-") && valid_id(&base) && !avatar_exists(connection, &base)? {
        if !base_exists(connection, &base)? {
            return Ok(base);
        }
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

fn fnv(text: &str) -> u32 {
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
    path.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn is_under(path: &str, root: &str) -> bool {
    let path = normalize_path_key(path);
    let root = normalize_path_key(root);
    path.starts_with(&(root + "\\"))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedFile {
    base_bodies: Vec<SeedBody>,
    avatars: Vec<SeedAvatar>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedBody {
    id: String,
    display_name: String,
    note: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeedAvatar {
    id: String,
    display_name: String,
    base_body: Option<String>,
    aliases: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import_data::parse_export;
    use crate::view::{avatar_cards, avatar_detail};

    const SAMPLE: &str = r#"{
        "products": [
            {"boothId":"1001","title":"ライム -Lime- / オリジナル3Dモデル","shop":"あまとうさぎ","description":"共通素体"},
            {"boothId":"1002","title":"プラム -Plum- / オリジナル3Dモデル","shop":"あまとうさぎ","description":"共通素体"},
            {"boothId":"1003","title":"オリジナル3Dモデル「マヌカ」ver1.02","shop":"STUDIO JINGO","description":"マヌカ"},
            {"boothId":"1004","title":"ライム対応ワンピース","shop":"sample","description":"ライム対応"},
            {"boothId":"1005","title":"汎用ペンライト","shop":"sample","description":"汎用ギミック"},
            {"boothId":"1006","title":"しなの専用ヘア","shop":"sample","description":"しなの専用"}
        ]
    }"#;

    fn extraction(
        category: Category,
        names: &[&str],
        generic: bool,
        confidence: f64,
    ) -> Extraction {
        Extraction {
            category,
            compatible_avatars: names.iter().map(|name| (*name).to_string()).collect(),
            is_generic: generic,
            confidence,
        }
    }

    #[test]
    fn shared_body_outfit_does_not_mark_siblings_owned() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "1001",
            &extraction(Category::Avatar, &["ライム", "プラム", "シフォン", "ショコラ"], false, 0.93),
            0.6,
        )
        .unwrap();
        apply_extraction(&mut connection, "1002", &extraction(Category::Avatar, &["プラム"], false, 0.9), 0.6).unwrap();
        apply_extraction(&mut connection, "1003", &extraction(Category::Avatar, &["マヌカ"], false, 0.9), 0.6).unwrap();
        apply_extraction(&mut connection, "1004", &extraction(Category::Outfit, &["ライム"], false, 0.88), 0.6).unwrap();
        apply_extraction(&mut connection, "1005", &extraction(Category::Gimmick, &[], true, 0.84), 0.6).unwrap();
        apply_extraction(&mut connection, "1006", &extraction(Category::Hair, &["しなの"], false, 0.86), 0.6).unwrap();

        let snapshot = load_snapshot(&mut connection).unwrap();
        let lime_links: Vec<_> = snapshot
            .links
            .iter()
            .filter(|link| link.product_id == "1001")
            .map(|link| link.avatar_id.as_str())
            .collect();
        assert_eq!(lime_links, vec!["lime"]);

        let cards = avatar_cards(&snapshot);
        assert!(cards.iter().any(|card| card.id == "lime"));
        assert!(cards.iter().any(|card| card.id == "plum"));
        assert!(!cards.iter().any(|card| card.id == "shinano"));

        let plum = avatar_detail(&snapshot, "plum").unwrap();
        assert!(plum.sections.iter().flat_map(|section| &section.items).any(|item| {
            item.booth_id == "1004" && item.via_base_body
        }));
        let manuka = avatar_detail(&snapshot, "manuka").unwrap();
        assert!(!manuka
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .any(|item| item.booth_id == "1004"));
        assert!(manuka.generic_items.iter().any(|item| item.booth_id == "1005"));
    }

    #[test]
    fn manual_link_survives_reresolve_and_description_change_clears_extraction() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        apply_extraction(&mut connection, "1004", &extraction(Category::Outfit, &["ライム"], false, 0.9), 0.6).unwrap();
        update_product(
            &mut connection,
            &ProductUpdate {
                booth_id: "1004".into(),
                category: Some("outfit".into()),
                is_generic: Some(false),
                avatar_ids: Some(vec!["manuka".into()]),
                confirm: Some(true),
            },
        )
        .unwrap();
        reresolve_all(&mut connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let links: Vec<_> = snapshot
            .links
            .iter()
            .filter(|link| link.product_id == "1004")
            .map(|link| link.avatar_id.as_str())
            .collect();
        assert_eq!(links, vec!["manuka"]);

        let changed = parse_export(
            r#"[{"boothId":"1004","title":"ライム対応ワンピース","description":"説明を更新した"}]"#,
        )
        .unwrap();
        import_products(&mut connection, &changed).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let product = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "1004")
            .unwrap();
        assert!(product.category.is_none() || product.category_locked);
        assert!(product.extracted_at.is_none());
        assert!(product.description_snippet.contains("更新"));
    }
}
