use crate::import_data::ParsedExport;
use crate::matching::{LabelScore, choose_match, contains_booth_id, score_label};
use crate::model::{
    Avatar, AvatarCandidate, AvatarInput, BaseBody, BaseBodyInput, CatalogInfo, CatalogRefresh,
    Category, ClassificationSnapshot, DownloadFact, DownloadFinished, Extraction, ImportReport,
    IncomingDownload, IncomingProduct, IncomingVariation, Knowledge, Link, LocalFile, Product,
    ProductFlag, ProductUpdate, ManagedProduct, Settings, SettingsInput, SettingsPatch, SetupStatus,
    VariationFact, now_iso,
};
use crate::resolve::{ResolveInput, resolve_product};
use crate::scan::ScanOutcome;
use crate::view::Snapshot;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{HashMap, HashSet};
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
    genre TEXT,
    genre_locked INTEGER NOT NULL DEFAULT 0,
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
    owned INTEGER NOT NULL DEFAULT 0,
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
CREATE TABLE avatar_sources (
    booth_id TEXT PRIMARY KEY,
    avatar_id TEXT NOT NULL,
    FOREIGN KEY (avatar_id) REFERENCES avatars(id) ON DELETE CASCADE
);
CREATE INDEX idx_links_avatar ON product_avatars(avatar_id);
CREATE INDEX idx_avatar_sources_avatar ON avatar_sources(avatar_id);
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
    let connection =
        Connection::open(path).map_err(|err| format!("データベースを開けません: {err}"))?;
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
    ensure_ready(&connection)?;
    let added = merge_catalog(&connection)?;
    settle_dictionary(&mut connection)?;
    if added > 0 {
        reresolve_all(&mut connection)?;
    }
    Ok(connection)
}

fn migrate(connection: &Connection) -> Result<(), String> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .sql()?;
    if version == 0 {
        connection.execute_batch(SCHEMA).sql()?;
        connection.execute_batch("PRAGMA user_version = 1;").sql()?;
    }
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS avatar_sources (
                booth_id TEXT PRIMARY KEY,
                avatar_id TEXT NOT NULL,
                FOREIGN KEY (avatar_id) REFERENCES avatars(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_avatar_sources_avatar ON avatar_sources(avatar_id);",
        )
        .sql()?;
    ensure_owned_column(connection)?;
    ensure_product_flag_columns(connection)?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS product_variations (
                booth_id TEXT NOT NULL,
                name TEXT NOT NULL,
                owned INTEGER NOT NULL,
                PRIMARY KEY (booth_id, name),
                FOREIGN KEY (booth_id) REFERENCES products(booth_id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS product_files (
                booth_id TEXT NOT NULL,
                variation TEXT NOT NULL DEFAULT '',
                filename TEXT NOT NULL,
                size_bytes INTEGER,
                downloaded INTEGER NOT NULL DEFAULT 0,
                local_path TEXT,
                PRIMARY KEY (booth_id, variation, filename),
                FOREIGN KEY (booth_id) REFERENCES products(booth_id) ON DELETE CASCADE
            );",
        )
        .sql()?;
    ensure_column(
        connection,
        "product_files",
        "downloadable_id",
        "ALTER TABLE product_files ADD COLUMN downloadable_id TEXT",
    )?;
    if version < 2 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS product_exclusions (
                booth_id TEXT PRIMARY KEY REFERENCES products(booth_id) ON DELETE CASCADE,
                excluded_at TEXT NOT NULL
            ); PRAGMA user_version = 2;",
        ).sql()?;
        tx.commit().sql()?;
    }
    if version < 3 {
        let tx = connection.unchecked_transaction().sql()?;
        ensure_column(&tx, "products", "genre", "ALTER TABLE products ADD COLUMN genre TEXT")?;
        ensure_column(&tx, "products", "genre_locked", "ALTER TABLE products ADD COLUMN genre_locked INTEGER NOT NULL DEFAULT 0")?;
        let products = load_products(&tx)?;
        for product in products {
            let category = product.category.unwrap_or(Category::Other);
            let genre = crate::genre::classify_text(category, &product.title, &product.description_snippet)
                .unwrap_or_else(|| crate::genre::validate(category, None));
            tx.execute("UPDATE products SET genre = ?1 WHERE booth_id = ?2", params![genre, product.booth_id]).sql()?;
        }
        tx.execute_batch("PRAGMA user_version = 3;").sql()?;
        tx.commit().sql()?;
    }
    if version < 4 {
        ensure_column(
            connection,
            "products",
            "classifier_version",
            "ALTER TABLE products ADD COLUMN classifier_version INTEGER NOT NULL DEFAULT 1",
        )?;
        connection
            .execute_batch("PRAGMA user_version = 4;")
            .sql()?;
    }
    // v5 is only `products.unpublished` (library badge「非公開」).
    // Keep this block self-contained so a parallel migration can take the next number.
    if version < 5 {
        ensure_column(
            connection,
            "products",
            "unpublished",
            "ALTER TABLE products ADD COLUMN unpublished INTEGER",
        )?;
        connection
            .execute_batch("PRAGMA user_version = 5;")
            .sql()?;
    }
    Ok(())
}

fn ensure_product_flag_columns(connection: &Connection) -> Result<(), String> {
    ensure_column(
        connection,
        "products",
        "is_free",
        "ALTER TABLE products ADD COLUMN is_free INTEGER",
    )?;
    ensure_column(
        connection,
        "products",
        "is_adult",
        "ALTER TABLE products ADD COLUMN is_adult INTEGER",
    )
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    statement: &str,
) -> Result<(), String> {
    let mut info = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .sql()?;
    let names: Vec<String> = info
        .query_map([], |row| row.get(1))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    if !names.iter().any(|name| name == column) {
        connection.execute(statement, []).sql()?;
    }
    Ok(())
}

fn ensure_owned_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection.prepare("PRAGMA table_info(avatars)").sql()?;
    let names: Vec<String> = statement
        .query_map([], |row| row.get(1))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    if !names.iter().any(|name| name == "owned") {
        connection
            .execute(
                "ALTER TABLE avatars ADD COLUMN owned INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .sql()?;
    }
    Ok(())
}

fn ensure_ready(connection: &Connection) -> Result<(), String> {
    for (key, value) in [
        ("ollama_base_url", "http://127.0.0.1:11434"),
        ("ollama_model", crate::model::DEFAULT_OLLAMA_MODEL),
        ("confidence_threshold", "0.6"),
        ("library_folder", ""),
        ("builtin_browser", "0"),
        ("booth_page_interval_ms", "1000"),
        ("remote_library_folder", ""),
        ("remote_auto_sync", "0"),
        ("route_downloads", "0"),
        ("llm_backend", "ollama"),
        ("background_mode", "0"),
        ("launch_at_login", "0"),
        ("unload_ollama_after_classification", "1"),
        ("classification_dismissed_version", "0"),
        ("ui_rail_width", "232"),
        ("ui_genre_width", "200"),
        ("check_update_on_startup", "0"),
        ("ui_update_dismissed_version", ""),
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
    close_unavailable_products(connection)?;
    skip_setup_for_existing_use(connection)?;
    Ok(())
}

fn skip_setup_for_existing_use(connection: &Connection) -> Result<(), String> {
    if setting(connection, "setup_complete")?.is_some() {
        return Ok(());
    }
    let products: i64 = connection
        .query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))
        .sql()?;
    let paired = extension_origin(connection)?.is_some();
    if products > 0 || paired {
        set_setting(connection, "setup_complete", "1")?;
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
        ollama_model: setting(connection, "ollama_model")?
            .unwrap_or_else(|| crate::model::DEFAULT_OLLAMA_MODEL.into()),
        confidence_threshold: threshold,
        library_folder: setting(connection, "library_folder")?.unwrap_or_default(),
        builtin_browser: setting(connection, "builtin_browser")?.as_deref() == Some("1"),
        remote_library_folder: setting(connection, "remote_library_folder")?.unwrap_or_default(),
        remote_auto_sync: setting(connection, "remote_auto_sync")?.as_deref() == Some("1"),
        route_downloads: setting(connection, "route_downloads")?.as_deref() == Some("1"),
        llm_backend: normalize_backend(
            &setting(connection, "llm_backend")?.unwrap_or_else(|| "ollama".into()),
        ),
        background_mode: setting(connection, "background_mode")?.as_deref() == Some("1"),
        launch_at_login: setting(connection, "launch_at_login")?.as_deref() == Some("1"),
        unload_ollama_after_classification: setting(connection, "unload_ollama_after_classification")?
            .as_deref()
            != Some("0"),
        classification_dismissed_version: setting(connection, "classification_dismissed_version")?
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(0),
        ui_rail_width: crate::model::clamp_ui_rail_width(
            setting(connection, "ui_rail_width")?
                .and_then(|value| value.parse().ok())
                .unwrap_or(232),
        ),
        ui_genre_width: crate::model::clamp_ui_genre_width(
            setting(connection, "ui_genre_width")?
                .and_then(|value| value.parse().ok())
                .unwrap_or(200),
        ),
        check_update_on_startup: setting(connection, "check_update_on_startup")?.as_deref() == Some("1"),
        ui_update_dismissed_version: setting(connection, "ui_update_dismissed_version")?
            .unwrap_or_default(),
    })
}

fn normalize_backend(value: &str) -> String {
    if value.trim() == "apple" {
        "apple".into()
    } else {
        "ollama".into()
    }
}

pub(crate) fn save_settings(connection: &Connection, input: &SettingsInput) -> Result<(), String> {
    save_settings_values(connection, input, true)
}

pub(crate) fn update_settings(
    connection: &Connection,
    patch: &SettingsPatch,
) -> Result<Settings, String> {
    let input = patch.apply(settings(connection)?);
    save_settings_values(
        connection,
        &input,
        patch.library_folder.is_some() || patch.route_downloads == Some(true),
    )?;
    settings(connection)
}

fn save_settings_values(
    connection: &Connection,
    input: &SettingsInput,
    validate_library: bool,
) -> Result<(), String> {
    let base = crate::ollama::validate_base_url(&input.ollama_base_url)?;
    let model = crate::ollama::validate_model_name(&input.ollama_model)?;
    let (background_mode, launch_at_login) =
        crate::presence::normalize_presence(input.background_mode, input.launch_at_login);
    let library = input.library_folder.trim();
    let backend = normalize_backend(&input.llm_backend);
    if backend == "apple" && !crate::apple_fm::can_select(None) {
        return Err("Apple のオンデバイスモデルは、今の Mac では使えません".into());
    }
    if input.remote_auto_sync {
        crate::remote_sync::validate_root(input.remote_library_folder.trim())?;
    }
    if input.route_downloads && library.is_empty() {
        return Err("自動整理をオンにする前に、ライブラリのフォルダを選んでください".into());
    }
    if validate_library && !library.is_empty() {
        let folder = std::path::Path::new(library);
        if !folder.is_absolute()
            || folder
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(
                "ライブラリのフォルダは、絶対パスで親ディレクトリを含まないものにしてください"
                    .into(),
            );
        }
        match crate::library_files::probe_path(folder, crate::library_files::PROBE_TIMEOUT) {
            crate::library_files::Probe::Present => {}
            crate::library_files::Probe::Absent => {
                return Err("ライブラリのフォルダが見つかりません".into());
            }
            crate::library_files::Probe::Unreachable => {
                return Err("ライブラリのフォルダに届きません。接続を確認してからもう一度".into());
            }
        }
        if !std::fs::metadata(folder)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
        {
            return Err("ライブラリのフォルダが見つかりません".into());
        }
        crate::scan::scan_block_reason(folder)?;
    }
    let tx = connection.unchecked_transaction().sql()?;
    let connection = &tx;
    set_setting(connection, "ollama_base_url", &base)?;
    set_setting(connection, "ollama_model", &model)?;
    set_setting(
        connection,
        "confidence_threshold",
        &input.confidence_threshold.clamp(0.0, 1.0).to_string(),
    )?;
    set_setting(connection, "library_folder", input.library_folder.trim())?;
    set_setting(
        connection,
        "builtin_browser",
        if input.builtin_browser { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "remote_library_folder",
        input.remote_library_folder.trim(),
    )?;
    set_setting(
        connection,
        "remote_auto_sync",
        if input.remote_auto_sync { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "route_downloads",
        if input.route_downloads { "1" } else { "0" },
    )?;
    set_setting(connection, "llm_backend", &backend)?;
    set_setting(
        connection,
        "background_mode",
        if background_mode { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "launch_at_login",
        if launch_at_login { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "unload_ollama_after_classification",
        if input.unload_ollama_after_classification { "1" } else { "0" },
    )?;
    set_setting(
        connection,
        "classification_dismissed_version",
        &input.classification_dismissed_version.to_string(),
    )?;
    set_setting(
        connection,
        "ui_rail_width",
        &crate::model::clamp_ui_rail_width(input.ui_rail_width).to_string(),
    )?;
    set_setting(
        connection,
        "ui_genre_width",
        &crate::model::clamp_ui_genre_width(input.ui_genre_width).to_string(),
    )?;
    set_setting(
        connection,
        "check_update_on_startup",
        if input.check_update_on_startup { "1" } else { "0" },
    )?;
    set_setting(connection, "ui_update_dismissed_version", &input.ui_update_dismissed_version)?;
    tx.commit().sql()
}

pub(crate) fn browser_bookmarks(
    connection: &Connection,
) -> Result<Vec<crate::browser_tabs::Bookmark>, String> {
    match setting(connection, "browser_bookmarks")? {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "ブックマークを読めません".into()),
        None => Ok(vec![
            crate::browser_tabs::Bookmark::new("BOOTH", "https://booth.pm/")?,
            crate::browser_tabs::Bookmark::new("購入ライブラリ", crate::browser::LIBRARY_URL)?,
        ]),
    }
}

pub(crate) fn save_browser_bookmark(
    connection: &Connection,
    original: Option<&str>,
    title: &str,
    url: &str,
) -> Result<(), String> {
    let mut bookmarks = browser_bookmarks(connection)?;
    crate::browser_tabs::save_bookmark(&mut bookmarks, original, title, url)?;
    set_setting(
        connection,
        "browser_bookmarks",
        &serde_json::to_string(&bookmarks)
            .map_err(|_| "ブックマークを保存できません".to_string())?,
    )
}

pub(crate) fn delete_browser_bookmark(connection: &Connection, url: &str) -> Result<(), String> {
    let mut bookmarks = browser_bookmarks(connection)?;
    bookmarks.retain(|bookmark| bookmark.url != url);
    set_setting(
        connection,
        "browser_bookmarks",
        &serde_json::to_string(&bookmarks)
            .map_err(|_| "ブックマークを保存できません".to_string())?,
    )
}

pub(crate) fn booth_page_interval_ms(connection: &Connection) -> Result<u64, String> {
    let stored = setting(connection, "booth_page_interval_ms")?
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(crate::library_walk::PAGE_DELAY_MS);
    Ok(crate::library_walk::normalize_page_delay_ms(stored))
}

pub(crate) fn set_booth_page_interval_ms(
    connection: &Connection,
    interval_ms: u64,
) -> Result<(), String> {
    if !crate::library_walk::allowed_page_delay_ms(interval_ms) {
        return Err("BOOTH のアクセス間隔は 250、500、1000 ミリ秒から選んでください".into());
    }
    set_setting(connection, "booth_page_interval_ms", &interval_ms.to_string())
}

pub(crate) fn import_export_json(
    connection: &mut Connection,
    json: &str,
) -> Result<ImportReport, String> {
    let parsed = crate::import_data::parse_export(json)?;
    if parsed.products.is_empty() {
        return Err(parsed
            .warnings
            .first()
            .cloned()
            .unwrap_or_else(|| "商品が1件もありません".into()));
    }
    import_products(connection, &parsed)
}

pub(crate) fn extension_token(connection: &Connection) -> Result<String, String> {
    if let Some(existing) = setting(connection, "extension_token")? {
        if crate::local_import::stored_token_ok(&existing) {
            return Ok(existing);
        }
    }
    let token = crate::local_import::generate_token();
    set_setting(connection, "extension_token", &token)?;
    Ok(token)
}

pub(crate) fn extension_origin(connection: &Connection) -> Result<Option<String>, String> {
    let Some(value) = setting(connection, "extension_origin")? else {
        return Ok(None);
    };
    if crate::local_import::is_extension_origin(&value) {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

pub(crate) fn set_extension_origin(connection: &Connection, origin: &str) -> Result<(), String> {
    set_setting(connection, "extension_origin", origin)
}

pub(crate) fn regenerate_extension_token(connection: &Connection) -> Result<String, String> {
    let token = crate::local_import::generate_token();
    set_setting(connection, "extension_token", &token)?;
    connection
        .execute("DELETE FROM settings WHERE key = 'extension_origin'", [])
        .sql()?;
    Ok(token)
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
    let mut touched = Vec::new();
    for product in &parsed.products {
        match upsert(&tx, product)? {
            Upsert::Inserted => {
                inserted += 1;
                touched.push(product.booth_id.clone());
            }
            Upsert::Updated => {
                updated += 1;
                touched.push(product.booth_id.clone());
            }
            Upsert::Unchanged => {
                unchanged += 1;
                touched.push(product.booth_id.clone());
            }
        }
    }
    tx.commit().sql()?;
    for booth_id in touched {
        apply_listed_compatibility(connection, &booth_id)?;
    }
    settle_dictionary(connection)?;
    let variations = parsed
        .products
        .iter()
        .map(|product| product.variations.len() as u32)
        .sum();
    let downloads = parsed
        .products
        .iter()
        .map(|product| product.downloads.len() as u32)
        .sum();
    let unnamed_downloads = parsed
        .products
        .iter()
        .flat_map(|product| &product.downloads)
        .filter(|download| {
            filename_is_placeholder(
                &download.filename,
                &clean_downloadable_id(&download.downloadable_id),
            )
        })
        .count() as u32;
    let mut warnings = parsed.warnings.clone();
    if !parsed.products.is_empty() && variations == 0 && downloads == 0 {
        warnings.push(
            "バリエーションもダウンロードも 0 件です。ファイルの有無は分かりません。拡張の「解析結果をコピー」で構造を確認するか、ライブラリを再インポートするか、フォルダを調べてください"
                .into(),
        );
    }
    Ok(ImportReport {
        inserted,
        updated,
        unchanged,
        skipped: parsed.skipped,
        variations,
        downloads,
        unnamed_downloads,
        warnings,
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
            return finish_upsert(connection, incoming, Upsert::Unchanged);
        }
        // Both locks mean a person placed this product. Keep genre, kind, confirmation,
        // and links so a later extension resend cannot queue it again.
        if description_changed && classification_pinned(&existing) {
            connection
                .execute(
                    "UPDATE products SET title = ?1, shop = ?2, description_snippet = ?3, url = ?4,
                     thumbnail_url = ?5, thumbnail_path = ?6, updated_at = ?7 WHERE booth_id = ?8",
                    params![
                        title,
                        shop,
                        description,
                        url,
                        thumbnail,
                        thumbnail_path,
                        now,
                        existing.booth_id
                    ],
                )
                .sql()?;
            return finish_upsert(connection, incoming, Upsert::Updated);
        }
        if description_changed {
            let category = if existing.category_locked {
                existing
                    .category
                    .map(|category| category.as_str().to_string())
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
        return finish_upsert(connection, incoming, Upsert::Updated);
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
    finish_upsert(connection, incoming, Upsert::Inserted)
}

fn finish_upsert(
    connection: &Connection,
    incoming: &IncomingProduct,
    status: Upsert,
) -> Result<Upsert, String> {
    let facts_changed = merge_library_facts(connection, incoming)?;
    if matches!(status, Upsert::Unchanged) && facts_changed {
        Ok(Upsert::Updated)
    } else {
        Ok(status)
    }
}

fn merge_library_facts(
    connection: &Connection,
    incoming: &IncomingProduct,
) -> Result<bool, String> {
    let mut changed = false;
    changed |= merge_flag(connection, &incoming.booth_id, "is_free", incoming.is_free)?;
    changed |= merge_flag(
        connection,
        &incoming.booth_id,
        "is_adult",
        incoming.is_adult,
    )?;
    changed |= merge_flag(
        connection,
        &incoming.booth_id,
        "unpublished",
        incoming.unpublished,
    )?;
    for variation in &incoming.variations {
        changed |= merge_variation(connection, &incoming.booth_id, variation)?;
    }
    for download in &incoming.downloads {
        if !download.variation.is_empty() {
            changed |= merge_variation(
                connection,
                &incoming.booth_id,
                &IncomingVariation {
                    name: download.variation.clone(),
                    owned: true,
                },
            )?;
        }
        changed |= merge_download(connection, &incoming.booth_id, download)?;
    }
    Ok(changed)
}

fn merge_flag(
    connection: &Connection,
    booth_id: &str,
    column: &str,
    value: Option<bool>,
) -> Result<bool, String> {
    let Some(value) = value else {
        return Ok(false);
    };
    let sql = match column {
        "is_free" => "UPDATE products SET is_free = ?1 WHERE booth_id = ?2 AND (is_free IS NOT ?1)",
        "is_adult" => {
            "UPDATE products SET is_adult = ?1 WHERE booth_id = ?2 AND (is_adult IS NOT ?1)"
        }
        "unpublished" => {
            "UPDATE products SET unpublished = ?1 WHERE booth_id = ?2 AND (unpublished IS NOT ?1)"
        }
        _ => return Err("不明な項目です".into()),
    };
    let changed = connection
        .execute(sql, params![if value { 1 } else { 0 }, booth_id])
        .sql()?;
    Ok(changed > 0)
}

fn merge_variation(
    connection: &Connection,
    booth_id: &str,
    variation: &IncomingVariation,
) -> Result<bool, String> {
    let Some(name) = crate::library_files::clean_variation(&variation.name) else {
        return Ok(false);
    };
    let owned = if variation.owned { 1 } else { 0 };
    let changed = connection
        .execute(
            "INSERT INTO product_variations (booth_id, name, owned) VALUES (?1, ?2, ?3)
             ON CONFLICT(booth_id, name) DO UPDATE SET
               owned = CASE WHEN product_variations.owned = 1 THEN 1 ELSE excluded.owned END
             WHERE NOT (product_variations.owned = 1 OR product_variations.owned = excluded.owned)",
            params![booth_id, name, owned],
        )
        .sql()?;
    Ok(changed > 0)
}

fn clean_downloadable_id(value: &str) -> String {
    let id = value.trim();
    if (1..=12).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit()) {
        id.to_string()
    } else {
        String::new()
    }
}

fn filename_is_placeholder(filename: &str, downloadable_id: &str) -> bool {
    let name = filename.trim();
    if name.is_empty() || (!downloadable_id.is_empty() && name == downloadable_id) {
        return true;
    }
    let bytes = name.as_bytes();
    (4..=12).contains(&bytes.len()) && bytes.iter().all(|byte| byte.is_ascii_digit())
}

fn merge_download(
    connection: &Connection,
    booth_id: &str,
    download: &IncomingDownload,
) -> Result<bool, String> {
    let downloadable_id = clean_downloadable_id(&download.downloadable_id);
    let Some(filename) = crate::library_files::safe_filename(&download.filename)
        .or_else(|| crate::library_files::safe_filename(&downloadable_id))
    else {
        return Ok(false);
    };
    let variation = crate::library_files::clean_variation(&download.variation).unwrap_or_default();
    if !downloadable_id.is_empty() {
        let existing: Option<(String, String)> = connection
            .query_row(
                "SELECT variation, filename FROM product_files
                 WHERE booth_id = ?1 AND downloadable_id = ?2",
                params![booth_id, downloadable_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .sql()?;
        let placeholder = if existing.is_none() && !downloadable_id.is_empty() {
            connection
                .query_row(
                    "SELECT variation, filename FROM product_files
                     WHERE booth_id = ?1
                       AND (downloadable_id IS NULL OR downloadable_id = '' OR downloadable_id = ?2)
                       AND (filename = ?2 OR filename = '')
                     LIMIT 1",
                    params![booth_id, downloadable_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .sql()?
        } else {
            None
        };
        if let Some((old_variation, old_filename)) = existing.or(placeholder) {
            let new_filename = if filename_is_placeholder(&filename, &downloadable_id) {
                old_filename.clone()
            } else {
                filename.clone()
            };
            let new_variation = if variation.is_empty() {
                old_variation.clone()
            } else {
                variation
            };
            let mut changed = false;
            if new_filename != old_filename || new_variation != old_variation {
                let clash: i64 = connection
                    .query_row(
                        "SELECT COUNT(*) FROM product_files
                         WHERE booth_id = ?1 AND variation = ?2 AND filename = ?3
                           AND NOT (variation = ?4 AND filename = ?5)",
                        params![
                            booth_id,
                            new_variation,
                            new_filename,
                            old_variation,
                            old_filename
                        ],
                        |row| row.get(0),
                    )
                    .sql()?;
                if clash == 0 {
                    let updated = connection
                        .execute(
                            "UPDATE product_files
                             SET variation = ?1, filename = ?2, downloadable_id = ?3
                             WHERE booth_id = ?4 AND variation = ?5 AND filename = ?6",
                            params![
                                new_variation,
                                new_filename,
                                downloadable_id,
                                booth_id,
                                old_variation,
                                old_filename
                            ],
                        )
                        .sql()?;
                    changed |= updated > 0;
                }
            }
            if !changed {
                let updated = connection
                    .execute(
                        "UPDATE product_files SET downloadable_id = ?1
                         WHERE booth_id = ?2 AND variation = ?3 AND filename = ?4
                           AND (downloadable_id IS NULL OR downloadable_id = '')",
                        params![downloadable_id, booth_id, old_variation, old_filename],
                    )
                    .sql()?;
                changed |= updated > 0;
            }
            if let Some(size) = download.size_bytes {
                let updated = connection
                    .execute(
                        "UPDATE product_files SET size_bytes = ?1
                         WHERE booth_id = ?2 AND downloadable_id = ?3 AND size_bytes IS NOT ?1",
                        params![size as i64, booth_id, downloadable_id],
                    )
                    .sql()?;
                changed |= updated > 0;
            }
            return Ok(changed);
        }
    }
    let id_value = if downloadable_id.is_empty() {
        None
    } else {
        Some(downloadable_id)
    };
    let changed = connection
        .execute(
            "INSERT INTO product_files (
                booth_id, variation, filename, size_bytes, downloaded, local_path, downloadable_id
             ) VALUES (?1, ?2, ?3, ?4, 0, NULL, ?5)
             ON CONFLICT(booth_id, variation, filename) DO UPDATE SET
               size_bytes = COALESCE(excluded.size_bytes, product_files.size_bytes),
               downloadable_id = COALESCE(product_files.downloadable_id, excluded.downloadable_id)
             WHERE (excluded.size_bytes IS NOT NULL AND product_files.size_bytes IS NOT excluded.size_bytes)
                OR (product_files.downloadable_id IS NULL AND excluded.downloadable_id IS NOT NULL)",
            params![
                booth_id,
                variation,
                filename,
                download.size_bytes.map(|size| size as i64),
                id_value
            ],
        )
        .sql()?;
    Ok(changed > 0)
}

fn apply_listed_compatibility(connection: &mut Connection, booth_id: &str) -> Result<(), String> {
    if product_excluded(connection, booth_id)? {
        return Ok(());
    }
    let existing = require_product(connection, booth_id)?;
    if classification_pinned(&existing) {
        return Ok(());
    }
    if existing.description_snippet == crate::model::UNAVAILABLE_DESCRIPTION {
        return note_unavailable(connection, booth_id);
    }
    if existing.links_locked {
        return Ok(());
    }
    if settle_named_original_body(connection, &existing)? {
        return Ok(());
    }
    let avatars = load_avatars(connection)?;
    let listed = crate::compat::listed_avatar_names(&existing.description_snippet, &avatars);
    if listed.is_empty() {
        return Ok(());
    }
    let category = if existing.category_locked {
        existing.category.unwrap_or(Category::Outfit)
    } else {
        crate::compat::category_for_listed_product(&existing.title, &existing.description_snippet)
    };
    let threshold = settings(connection)?.confidence_threshold;
    apply_extraction(
        connection,
        booth_id,
        &Extraction {
            category,
            genre: None,
            compatible_avatars: listed,
            is_generic: false,
            confidence: 0.9,
        },
        threshold,
    )
}

pub(crate) fn apply_extraction(
    connection: &mut Connection,
    booth_id: &str,
    extraction: &Extraction,
    threshold: f64,
) -> Result<(), String> {
    if product_excluded(connection, booth_id)? {
        return Ok(());
    }
    let existing = require_product(connection, booth_id)?;
    if classification_pinned(&existing) {
        return Ok(());
    }
    let avatars = load_avatars(connection)?;
    let names = crate::compat::names_for_extraction(
        &existing.title,
        &existing.description_snippet,
        &extraction.compatible_avatars,
        &avatars,
    );
    let category = decide_category(&existing, extraction.category, extraction.confidence);
    let genre = if existing.genre_locked {
        existing.genre.clone()
    } else {
        category.map(|category| {
            let selected = crate::genre::classify_text(category, &existing.title, &existing.description_snippet)
                .or_else(|| extraction.genre.clone());
            crate::genre::validate(category, selected.as_deref())
        })
    };
    let model_generic = if category == Some(Category::World) {
        false
    } else {
        extraction.is_generic
    };
    let is_generic = generic_without_avatars(category, &names, model_generic);
    let output = resolve_product(
        &ResolveInput {
            booth_id: existing.booth_id.as_str(),
            title: existing.title.as_str(),
            description: existing.description_snippet.as_str(),
            category,
            raw_names: &names,
            is_generic,
            confidence: Some(extraction.confidence),
            user_confirmed: false,
            threshold,
            allow_create: !existing.links_locked,
        },
        &avatars,
    );
    let linked_ids = if existing.links_locked {
        link_ids(connection, booth_id)?
    } else {
        output.avatar_ids.clone()
    };
    let established = established_avatar_sources(connection, threshold)?;
    let needs_review = if existing.links_locked {
        review_from_product(
            connection,
            &existing,
            category,
            &output.unresolved,
            &linked_ids,
            threshold,
        )?
    } else {
        review_with_established_placeholders(
            &output,
            &existing.booth_id,
            category,
            is_generic,
            extraction.confidence,
            threshold,
            false,
            &existing.title,
            &existing.description_snippet,
            &avatars,
            &established,
        )
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
            insert_placeholder(&tx, &draft.id, &draft.display_name, &draft.aliases)?;
        }
        remember_readings(&tx, &output.readings)?;
    }
    tx.execute(
        "UPDATE products SET category = ?1, genre = ?2, compatible_avatars_json = ?3, unresolved_names_json = ?4,
         is_generic = ?5, confidence = ?6, needs_review = ?7, user_confirmed = 0,
         extracted_at = ?8, updated_at = ?9, classifier_version = ?10 WHERE booth_id = ?11",
        params![
            category_text,
            genre,
            to_json(&names)?,
            to_json(&unresolved)?,
            is_generic,
            extraction.confidence,
            needs_review,
            now,
            now,
            crate::classifier::CLASSIFIER_VERSION,
            booth_id
        ],
    )
    .sql()?;
    if !existing.links_locked {
        replace_links(&tx, booth_id, &output.avatar_ids, "extracted")?;
    }
    tx.commit().sql()?;
    // The model records an owned-<booth id> placeholder and source "extracted"
    // without setting avatars.owned. Settle afterwards so a later reclassify
    // does not leave that body unowned. Guards in settle_named_original_body
    // still reject locked, confirmed, excluded, and multi-quote titles.
    let placeholder = format!("owned-{booth_id}");
    if category == Some(Category::Avatar) && linked_ids.iter().any(|id| id == &placeholder) {
        let updated = require_product(connection, booth_id)?;
        settle_named_original_body(connection, &updated)?;
    }
    Ok(())
}

fn generic_without_avatars(
    category: Option<Category>,
    names: &[String],
    model_generic: bool,
) -> bool {
    if category == Some(Category::Accessory)
        && names
            .iter()
            .all(|name| crate::normalize::is_blocked_avatar_label(name))
    {
        return true;
    }
    // Texture and gimmick with no avatar, and none listed for one, are generic.
    // The model's own flag does not decide this.
    if matches!(category, Some(Category::Texture | Category::Gimmick)) {
        return names.is_empty();
    }
    model_generic
}

fn jp_readings(products: &[Product]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for product in products {
        for name in &product.compatible_avatars {
            let pieces = crate::normalize::paren_pieces(name);
            if pieces.len() != 2 {
                continue;
            }
            let Some(jp) = pieces
                .iter()
                .find(|piece| piece.chars().any(crate::normalize::is_cjk))
            else {
                continue;
            };
            let Some(latin) = pieces.iter().find(|piece| {
                piece.chars().any(|ch| ch.is_ascii_alphabetic())
                    && !piece.chars().any(crate::normalize::is_cjk)
            }) else {
                continue;
            };
            map.insert(crate::normalize::normalize_label(latin), jp.clone());
        }
    }
    map
}

fn fold_romaji_names(names: &[String], readings: &HashMap<String, String>) -> Vec<String> {
    let mut folded = Vec::new();
    for name in names {
        let next = if name.chars().any(crate::normalize::is_cjk) {
            name.clone()
        } else {
            readings
                .get(&crate::normalize::normalize_label(name))
                .cloned()
                .unwrap_or_else(|| name.clone())
        };
        if !folded.iter().any(|existing| existing == &next) {
            folded.push(next);
        }
    }
    folded
}

fn established_avatar_sources(
    connection: &Connection,
    threshold: f64,
) -> Result<HashSet<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id, confidence, user_confirmed FROM products WHERE category = 'avatar' AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            let confidence: Option<f64> = row.get(1)?;
            let confirmed: i64 = row.get(2)?;
            Ok((row.get::<_, String>(0)?, confidence, confirmed != 0))
        })
        .sql()?;
    let mut ids = HashSet::new();
    for row in rows {
        let (id, confidence, confirmed) = row.sql()?;
        if confirmed || confidence.unwrap_or(0.0) >= threshold {
            ids.insert(id);
        }
    }
    Ok(ids)
}

fn review_with_established_placeholders(
    output: &crate::resolve::ResolveOutput,
    booth_id: &str,
    category: Option<Category>,
    is_generic: bool,
    confidence: f64,
    threshold: f64,
    user_confirmed: bool,
    title: &str,
    description: &str,
    avatars: &[crate::model::Avatar],
    established: &HashSet<String>,
) -> bool {
    let blocking = output.avatar_ids.iter().any(|id| {
        let Some(source) = id.strip_prefix("owned-") else {
            return false;
        };
        let self_ok =
            source == booth_id && category == Some(Category::Avatar) && confidence >= threshold;
        !(self_ok || established.contains(source))
    });
    crate::resolve::product_needs_review(&crate::resolve::ReviewFacts {
        category,
        placeholder: blocking,
        link_count: output.avatar_ids.len(),
        unresolved: &output.unresolved,
        is_generic,
        grounded: created_names_are_grounded(output, avatars, title, description),
        user_confirmed,
        threshold,
    })
}

fn created_names_are_grounded(
    output: &crate::resolve::ResolveOutput,
    avatars: &[crate::model::Avatar],
    title: &str,
    description: &str,
) -> bool {
    if output.avatar_ids.is_empty() {
        return false;
    }
    let haystack = crate::normalize::nfkc(&format!("{title}\n{description}")).to_lowercase();
    output.avatar_ids.iter().all(|id| {
        if let Some(draft) = output.created_avatars.iter().find(|draft| &draft.id == id) {
            return std::iter::once(&draft.display_name)
                .chain(draft.aliases.iter())
                .any(|form| {
                    let needle = crate::normalize::nfkc(form).to_lowercase();
                    needle.chars().count() >= 1 && haystack.contains(&needle)
                });
        }
        crate::resolve::links_grounded(std::slice::from_ref(id), avatars, title, description)
    })
}

pub(crate) fn reresolve_all(connection: &mut Connection) -> Result<u32, String> {
    let threshold = settings(connection)?.confidence_threshold;
    let tx = connection.transaction().sql()?;
    let mut processed = 0;
    // A second pass picks up readings learned from 名前(読み) pairs in the first pass.
    for _pass in 0..2 {
        let products = load_products(&tx)?;
        let readings = jp_readings(&products);
        processed = 0;
        for product in products {
            if product.links_locked
                || (product.category.is_none() && product.extracted_at.is_none())
            {
                continue;
            }
            let avatars = load_avatars(&tx)?;
            let mut names = fold_romaji_names(&product.compatible_avatars, &readings);
            let mut hinted = false;
            for hint in crate::compat::title_avatar_hints(&product.title, &avatars) {
                let label = crate::resolve::resolve_name(&hint, &avatars)
                    .map(|avatar| avatar.display_name.clone())
                    .unwrap_or(hint);
                hinted = true;
                if !names.iter().any(|name| name == &label) {
                    names.push(label);
                }
            }
            let is_generic = if hinted {
                false
            } else {
                generic_without_avatars(product.category, &names, product.is_generic)
            };
            let mut output = resolve_product(
                &ResolveInput {
                    booth_id: product.booth_id.as_str(),
                    title: product.title.as_str(),
                    description: product.description_snippet.as_str(),
                    category: product.category,
                    raw_names: &names,
                    is_generic,
                    confidence: product.confidence,
                    user_confirmed: product.user_confirmed,
                    threshold,
                    allow_create: true,
                },
                &avatars,
            );
            let established = established_avatar_sources(&tx, threshold)?;
            output.needs_review = review_with_established_placeholders(
                &output,
                &product.booth_id,
                product.category,
                is_generic,
                product.confidence.unwrap_or(0.0),
                threshold,
                product.user_confirmed,
                &product.title,
                &product.description_snippet,
                &avatars,
                &established,
            );
            let reopen =
                output.unresolved != product.unresolved_names && !output.unresolved.is_empty();
            if reopen {
                output.needs_review = true;
            }
            for draft in &output.created_avatars {
                insert_placeholder(&tx, &draft.id, &draft.display_name, &draft.aliases)?;
            }
            remember_readings(&tx, &output.readings)?;
            let confirmed = if reopen {
                false
            } else {
                product.user_confirmed
            };
            tx.execute(
                "UPDATE products SET unresolved_names_json = ?1, needs_review = ?2, user_confirmed = ?3,
                 is_generic = ?4, compatible_avatars_json = ?5, updated_at = ?6 WHERE booth_id = ?7",
                params![
                    to_json(&output.unresolved)?,
                    output.needs_review,
                    confirmed,
                    is_generic,
                    to_json(&names)?,
                    now_iso(),
                    product.booth_id
                ],
            )
            .sql()?;
            replace_links(&tx, &product.booth_id, &output.avatar_ids, "extracted")?;
            processed += 1;
        }
    }
    tx.execute(
        "DELETE FROM avatars WHERE id LIKE 'owned-%' AND id NOT IN (SELECT avatar_id FROM product_avatars)",
        [],
    )
    .sql()?;
    tx.commit().sql()?;
    Ok(processed)
}

fn classification_pinned(product: &Product) -> bool {
    product.category_locked && product.genre_locked
}

fn classification_snapshot(product: &Product) -> ClassificationSnapshot {
    ClassificationSnapshot {
        booth_id: product.booth_id.clone(),
        category: product.category.map(|category| category.as_str().to_string()),
        genre: product.genre.clone(),
        category_locked: product.category_locked,
        genre_locked: product.genre_locked,
        user_confirmed: product.user_confirmed,
        needs_review: product.needs_review,
    }
}

/// Move one product onto an asset or world genre and pin it by locking category and genre together.
/// Returns the classification from before the move.
pub(crate) fn move_product_genre(
    connection: &mut Connection,
    booth_id: &str,
    kind: &str,
    genre: &str,
) -> Result<ClassificationSnapshot, String> {
    if product_excluded(connection, booth_id)? {
        return Err("この商品は除外されています".into());
    }
    let product = require_product(connection, booth_id)?;
    let allowed = crate::genre::kind_genres(kind).ok_or("ジャンルの種類が不正です")?;
    if !allowed.iter().any(|(key, _)| *key == genre) {
        return Err("ジャンルが不正です".into());
    }
    let category = crate::genre::category_for_genre(genre).ok_or("ジャンルが不正です")?;
    if (category == Category::World) != (kind == "world") {
        return Err("ジャンルが不正です".into());
    }
    let validated = crate::genre::validate(category, Some(genre));
    if validated != genre {
        return Err("ジャンルが不正です".into());
    }
    let snapshot = classification_snapshot(&product);
    let already = product.category == Some(category)
        && product.genre.as_deref() == Some(genre)
        && classification_pinned(&product);
    if !already {
        update_product(
            connection,
            &ProductUpdate {
                booth_id: booth_id.to_string(),
                category: Some(category.as_str().to_string()),
                genre: Some(genre.to_string()),
                is_generic: None,
                avatar_ids: None,
                confirm: None,
            },
        )?;
    }
    Ok(snapshot)
}

pub(crate) fn restore_product_classification(
    connection: &Connection,
    snapshot: &ClassificationSnapshot,
) -> Result<(), String> {
    let _product = require_product(connection, &snapshot.booth_id)?;
    if let Some(category) = snapshot.category.as_deref() {
        if Category::parse(category).is_none() {
            return Err("カテゴリが不正です".into());
        }
    }
    connection
        .execute(
            "UPDATE products SET category = ?1, category_locked = ?2, genre = ?3, genre_locked = ?4,
             user_confirmed = ?5, needs_review = ?6, updated_at = ?7 WHERE booth_id = ?8",
            params![
                snapshot.category,
                snapshot.category_locked,
                snapshot.genre,
                snapshot.genre_locked,
                snapshot.user_confirmed,
                snapshot.needs_review,
                now_iso(),
                snapshot.booth_id
            ],
        )
        .sql()?;
    Ok(())
}

/// Clear both locks so the next reclassification may replace this placement.
/// Returns the pinned snapshot so the unpin itself can be undone.
pub(crate) fn unpin_product_classification(
    connection: &Connection,
    booth_id: &str,
) -> Result<ClassificationSnapshot, String> {
    let product = require_product(connection, booth_id)?;
    if !classification_pinned(&product) {
        return Err("この商品は手動で固定されていません".into());
    }
    let snapshot = classification_snapshot(&product);
    connection
        .execute(
            "UPDATE products SET category_locked = 0, genre_locked = 0, updated_at = ?1 WHERE booth_id = ?2",
            params![now_iso(), booth_id],
        )
        .sql()?;
    Ok(snapshot)
}

/// Add one manual avatar link and lock links so reresolve keeps it.
/// Category and genre locks stay as they are, so this is not a classification pin.
pub(crate) fn attach_product_to_avatar(
    connection: &mut Connection,
    avatar_id: &str,
    booth_id: &str,
) -> Result<(), String> {
    if !avatar_exists(connection, avatar_id)? {
        return Err(format!("アバター {avatar_id} が対応表にありません"));
    }
    if product_excluded(connection, booth_id)? {
        return Err("この商品は除外されています".into());
    }
    let product = require_product(connection, booth_id)?;
    match product.category {
        Some(Category::Avatar) => {
            return Err("アバター本体は衣装として追加できません".into());
        }
        Some(Category::World) => {
            return Err("ワールドはアバターに追加できません".into());
        }
        _ => {}
    }
    let tx = connection.transaction().sql()?;
    tx.execute(
        "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, 'manual')
         ON CONFLICT(product_id, avatar_id) DO UPDATE SET source = 'manual'",
        params![booth_id, avatar_id],
    )
    .sql()?;
    tx.execute(
        "UPDATE products SET links_locked = 1, updated_at = ?1 WHERE booth_id = ?2",
        params![now_iso(), booth_id],
    )
    .sql()?;
    tx.commit().sql()?;
    Ok(())
}

pub(crate) fn update_product(
    connection: &mut Connection,
    update: &ProductUpdate,
) -> Result<(), String> {
    let mut product = require_product(connection, &update.booth_id)?;
    let threshold = settings(connection)?.confidence_threshold;
    if let Some(category) = &update.category {
        product.category = Some(Category::parse(category).ok_or("カテゴリが不正です")?);
        product.category_locked = true;
    }
    if let Some(genre) = update.genre.as_deref().filter(|value| !value.is_empty()) {
        let category = product.category.ok_or("ジャンルを選ぶ前にカテゴリを選んでください")?;
        product.genre = Some(crate::genre::validate(category, Some(genre)));
        product.genre_locked = true;
    } else if update.category.is_some() {
        product.genre = product.category.map(|category| crate::genre::validate(category, product.genre.as_deref()));
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
    let linked_ids = if let Some(ids) = &replacement {
        ids.clone()
    } else {
        link_ids(connection, &product.booth_id)?
    };
    product.needs_review = review_from_product(
        connection,
        &product,
        product.category,
        &product.unresolved_names,
        &linked_ids,
        threshold,
    )?;
    let category = product
        .category
        .map(|category| category.as_str().to_string());
    let tx = connection.transaction().sql()?;
    tx.execute(
        "UPDATE products SET category = ?1, category_locked = ?2, genre = ?3, genre_locked = ?4, is_generic = ?5,
         unresolved_names_json = ?6, links_locked = ?7, user_confirmed = ?8, needs_review = ?9,
         updated_at = ?10 WHERE booth_id = ?11",
        params![
            category,
            product.category_locked,
            product.genre,
            product.genre_locked,
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

/// Discard work started for a folder that the user has since changed.
pub(crate) fn apply_library_scan(
    connection: &mut Connection,
    root: &str,
    outcome: &ScanOutcome,
) -> Result<crate::model::ScanReport, String> {
    if settings(connection)?.library_folder != root {
        return Err("調査中にライブラリのフォルダが変更されたため、以前の結果は反映しませんでした".into());
    }
    apply_scan(connection, root, outcome)
}

pub(crate) fn apply_scan(
    connection: &mut Connection,
    root: &str,
    outcome: &ScanOutcome,
) -> Result<crate::model::ScanReport, String> {
    let products = load_products(connection)?;
    let excluded = excluded_product_ids(connection)?;
    let tx = connection.transaction().sql()?;
    tx.execute(
        "DELETE FROM local_files WHERE (link_source IS NULL OR link_source != 'manual')
         AND COALESCE(linked_product_id, '') NOT IN (SELECT booth_id FROM product_exclusions)
         AND COALESCE(candidate_product_id, '') NOT IN (SELECT booth_id FROM product_exclusions)",
        [],
    )
    .sql()?;
    let now = now_iso();
    let mut linked = 0u32;
    let mut seen = HashSet::new();
    for entry in &outcome.entries {
        seen.insert(normalize_path_key(&entry.path));
        let retained: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM local_files WHERE path = ?1 AND
             (linked_product_id IN (SELECT booth_id FROM product_exclusions) OR candidate_product_id IN (SELECT booth_id FROM product_exclusions)))",
            [&entry.path], |row| row.get(0),
        ).sql()?;
        if retained {
            continue;
        }
        if entry.labels.iter().any(|label| {
            excluded.iter().any(|id| contains_booth_id(label, id))
        }) {
            continue;
        }
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
            (
                Some(id.clone()),
                Some(id.clone()),
                *score,
                Some("auto".to_string()),
            )
        } else if let Some(top) = scores.iter().max_by(|left, right| {
            left.score
                .partial_cmp(&right.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
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
    sync_scanned_downloads(&tx)?;
    tx.commit().sql()?;
    Ok(crate::model::ScanReport {
        indexed: outcome.entries.len() as u32,
        linked,
        truncated: outcome.truncated,
        root: root.to_string(),
    })
}

pub(crate) fn mark_description_seen(connection: &Connection, booth_id: &str) -> Result<(), String> {
    note_unavailable(connection, booth_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractTarget {
    Unclassified,
    Stale,
    All,
}

pub(crate) fn parse_extract_target(value: &str) -> Result<ExtractTarget, String> {
    match value {
        "unclassified" => Ok(ExtractTarget::Unclassified),
        "stale" => Ok(ExtractTarget::Stale),
        "all" => Ok(ExtractTarget::All),
        _ => Err("分類対象が不正です".into()),
    }
}

pub(crate) fn products_for_extract(
    connection: &Connection,
    target: ExtractTarget,
) -> Result<Vec<ExtractJob>, String> {
    let rows = match target {
        ExtractTarget::All => {
            let mut statement = connection
                .prepare(
                    "SELECT booth_id, title, description_snippet FROM products
                     WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)
                       AND NOT (category_locked = 1 AND genre_locked = 1)
                     ORDER BY updated_at DESC",
                )
                .sql()?;
            statement
                .query_map([], |row| {
                    Ok(ExtractJob {
                        booth_id: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                    })
                })
                .sql()?
                .collect::<Result<Vec<_>, _>>()
                .sql()?
        }
        ExtractTarget::Unclassified => {
            let mut statement = connection
                .prepare(
                    "SELECT booth_id, title, description_snippet FROM products
                     WHERE (extracted_at IS NULL OR category IS NULL)
                       AND NOT (category_locked = 1 AND genre_locked = 1)
                       AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
                     ORDER BY imported_at ASC",
                )
                .sql()?;
            statement
                .query_map([], |row| {
                    Ok(ExtractJob {
                        booth_id: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                    })
                })
                .sql()?
                .collect::<Result<Vec<_>, _>>()
                .sql()?
        }
        ExtractTarget::Stale => {
            let mut statement = connection
                .prepare(
                    "SELECT booth_id, title, description_snippet FROM products
                     WHERE extracted_at IS NOT NULL
                       AND category IS NOT NULL
                       AND classifier_version < ?1
                       AND description_snippet != ?2
                       AND NOT (category_locked = 1 AND genre_locked = 1)
                       AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
                     ORDER BY updated_at DESC",
                )
                .sql()?;
            statement
                .query_map(
                    params![
                        crate::classifier::CLASSIFIER_VERSION,
                        crate::model::UNAVAILABLE_DESCRIPTION
                    ],
                    |row| {
                        Ok(ExtractJob {
                            booth_id: row.get(0)?,
                            title: row.get(1)?,
                            description: row.get(2)?,
                        })
                    },
                )
                .sql()?
                .collect::<Result<Vec<_>, _>>()
                .sql()?
        }
    };
    Ok(rows)
}

pub(crate) fn classification_status(connection: &Connection) -> Result<crate::model::ClassificationStatus, String> {
    let stale_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM products
             WHERE extracted_at IS NOT NULL
               AND category IS NOT NULL
               AND classifier_version < ?1
               AND description_snippet != ?2
               AND NOT (category_locked = 1 AND genre_locked = 1)
               AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)",
            params![
                crate::classifier::CLASSIFIER_VERSION,
                crate::model::UNAVAILABLE_DESCRIPTION
            ],
            |row| row.get(0),
        )
        .sql()?;
    Ok(crate::model::ClassificationStatus {
        classifier_version: crate::classifier::CLASSIFIER_VERSION,
        stale_count: stale_count as u32,
    })
}

pub(crate) fn thumbnail_jobs(connection: &Connection) -> Result<Vec<ThumbJob>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id, thumbnail_url, thumbnail_path FROM products
             WHERE thumbnail_url IS NOT NULL AND thumbnail_url != ''
               AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)",
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

pub(crate) fn product_ids_by_category(
    connection: &Connection,
    category: Category,
) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id FROM products
             WHERE category = ?1 AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
             ORDER BY title COLLATE NOCASE, booth_id",
        )
        .sql()?;
    let rows = statement
        .query_map([category.as_str()], |row| row.get(0))
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(crate) fn product_ids_by_genre(connection: &Connection, kind: &str, genre: Option<&str>) -> Result<Vec<String>, String> {
    let condition = match kind { "world" => "category = 'world'", "asset" => "category <> 'world' AND category <> 'avatar'", _ => return Err("ジャンルの種類が不正です".into()) };
    let genre_filter = if genre == Some(crate::genre::UNSET) {
        // Keys are compile-time constants, so inlining them is safe.
        let known = crate::genre::kind_genres(kind)
            .unwrap_or_default()
            .iter()
            .map(|(key, _)| format!("'{key}'"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("(?1 IS NOT NULL AND (genre IS NULL OR genre NOT IN ({known})))")
    } else {
        "(?1 IS NULL OR genre = ?1)".to_string()
    };
    let sql = format!("SELECT booth_id FROM products WHERE {condition} AND {genre_filter} AND booth_id NOT IN (SELECT booth_id FROM product_exclusions) ORDER BY title COLLATE NOCASE, booth_id");
    let mut statement = connection.prepare(&sql).sql()?;
    let rows = statement.query_map([genre], |row| row.get(0)).sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(crate) fn load_snapshot(connection: &Connection) -> Result<Snapshot, String> {
    let products = load_products(connection)?;
    let active: HashSet<_> = products.iter().map(|p| p.booth_id.as_str()).collect();
    let excluded = excluded_product_ids(connection)?;
    let all_links = load_links(connection)?;
    let links: Vec<_> = all_links
        .iter()
        .filter(|link| active.contains(link.product_id.as_str()))
        .cloned()
        .collect();
    let mut avatars = load_avatars(connection)?;
    let active_bodies: HashSet<_> = products
        .iter()
        .filter(|p| p.category == Some(Category::Avatar))
        .map(|p| p.booth_id.as_str())
        .collect();
    let owned: HashSet<_> = links
        .iter()
        .filter(|link| active_bodies.contains(link.product_id.as_str()))
        .map(|link| link.avatar_id.as_str())
        .collect();
    // An excluded or invalidated body cannot keep a historical ownership flag visible.
    for avatar in &mut avatars {
        avatar.owned = avatar.owned && owned.contains(avatar.id.as_str());
    }
    Ok(Snapshot {
        products,
        files: load_files(connection)?
            .into_iter()
            .filter(|file| {
                !file
                    .linked_product_id
                    .as_ref()
                    .is_some_and(|id| excluded.contains(id))
                    && (file.linked_product_id.is_some()
                        || !file
                            .candidate_product_id
                            .as_ref()
                            .is_some_and(|id| excluded.contains(id)))
            })
            .collect(),
        avatars,
        base_bodies: load_bases(connection)?,
        links,
        variations: load_variations(connection)?,
        download_rows: load_download_rows(connection)?,
        placed_downloads: Vec::new(),
        product_flags: load_product_flags(connection)?,
        remote_configured: false,
        remote_checked: true,
        remote_reachable: false,
    })
}

pub(crate) fn product_excluded(connection: &Connection, booth_id: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM product_exclusions WHERE booth_id = ?1)",
            [booth_id],
            |row| row.get(0),
        )
        .sql()
}

fn excluded_product_ids(connection: &Connection) -> Result<HashSet<String>, String> {
    let mut query = connection
        .prepare("SELECT booth_id FROM product_exclusions")
        .sql()?;
    query
        .query_map([], |row| row.get(0))
        .sql()?
        .collect::<Result<_, _>>()
        .sql()
}

pub(crate) fn managed_products(connection: &Connection) -> Result<Vec<ManagedProduct>, String> {
    let mut query = connection
        .prepare(
            "SELECT p.booth_id, p.title, p.shop, p.category, e.booth_id IS NOT NULL
        FROM products p LEFT JOIN product_exclusions e ON p.booth_id = e.booth_id
        ORDER BY p.imported_at DESC, p.booth_id",
        )
        .sql()?;
    query
        .query_map([], |row| {
            let category: Option<String> = row.get(3)?;
            Ok(ManagedProduct {
                booth_id: row.get(0)?,
                title: row.get(1)?,
                shop: row.get(2)?,
                category: category.as_deref().and_then(Category::parse),
                excluded: row.get(4)?,
            })
        })
        .sql()?
        .collect::<Result<_, _>>()
        .sql()
}

pub(crate) fn set_product_excluded(
    connection: &mut Connection,
    booth_id: &str,
    excluded: bool,
) -> Result<(), String> {
    require_product(connection, booth_id)?;
    let tx = connection.transaction().sql()?;
    if excluded {
        tx.execute(
            "INSERT OR IGNORE INTO product_exclusions (booth_id, excluded_at) VALUES (?1, ?2)",
            params![booth_id, now_iso()],
        )
        .sql()?;
    } else {
        tx.execute(
            "DELETE FROM product_exclusions WHERE booth_id = ?1",
            [booth_id],
        )
        .sql()?;
    }
    tx.commit().sql()
}

fn load_variations(connection: &Connection) -> Result<Vec<VariationFact>, String> {
    let mut statement = connection
        .prepare("SELECT booth_id, name, owned FROM product_variations")
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok(VariationFact {
                booth_id: row.get(0)?,
                name: row.get(1)?,
                owned: row.get::<_, i64>(2)? != 0,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn load_download_rows(connection: &Connection) -> Result<Vec<DownloadFact>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id, variation, filename, size_bytes, downloaded, local_path
             FROM product_files",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            let size: Option<i64> = row.get(3)?;
            Ok(DownloadFact {
                booth_id: row.get(0)?,
                variation: row.get(1)?,
                filename: row.get(2)?,
                size_bytes: size.and_then(|value| u64::try_from(value).ok()),
                downloaded: row.get::<_, i64>(4)? != 0,
                local_path: row.get(5)?,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn load_product_flags(connection: &Connection) -> Result<Vec<ProductFlag>, String> {
    let mut statement = connection
        .prepare("SELECT booth_id, is_free, is_adult, unpublished FROM products")
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            let free: Option<i64> = row.get(1)?;
            let adult: Option<i64> = row.get(2)?;
            let unpublished: Option<i64> = row.get(3)?;
            Ok(ProductFlag {
                booth_id: row.get(0)?,
                is_free: free.map(|value| value != 0),
                is_adult: adult.map(|value| value != 0),
                unpublished: unpublished.map(|value| value != 0),
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn booth_id_in_path(path: &str, ids: &HashSet<String>) -> Option<String> {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect();
    let dirs = if parts.len() > 1 {
        &parts[..parts.len() - 1]
    } else {
        &parts[..]
    };
    if let Some(exact) = dirs.iter().rev().find(|part| ids.contains(**part)) {
        return Some((*exact).to_string());
    }
    let mut found = Vec::new();
    for id in ids {
        if contains_booth_id(path, id) {
            found.push(id.clone());
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}

fn sync_scanned_downloads(connection: &Connection) -> Result<(), String> {
    let mut id_query = connection.prepare("SELECT booth_id FROM products").sql()?;
    let product_ids: HashSet<String> = id_query
        .query_map([], |row| row.get(0))
        .sql()?
        .collect::<Result<HashSet<_>, _>>()
        .sql()?;
    drop(id_query);
    let mut statement = connection
        .prepare("SELECT path, filename, size, linked_product_id FROM local_files WHERE is_dir = 0")
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    drop(statement);
    for (path, filename, size, linked) in rows {
        let Some(filename) = crate::library_files::safe_filename(&filename) else {
            continue;
        };
        let booth_id = linked
            .filter(|id| product_ids.contains(id))
            .or_else(|| booth_id_in_path(&path, &product_ids));
        let Some(booth_id) = booth_id else {
            continue;
        };
        let existing: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE booth_id = ?1 AND filename = ?2",
                params![booth_id, filename],
                |row| row.get(0),
            )
            .sql()?;
        if existing > 0 {
            connection
                .execute(
                    "UPDATE product_files SET downloaded = 1, local_path = ?1,
                       size_bytes = COALESCE(size_bytes, ?2)
                     WHERE booth_id = ?3 AND filename = ?4
                       AND (downloaded = 0 OR local_path IS NULL OR size_bytes IS NULL)",
                    params![path, size, booth_id, filename],
                )
                .sql()?;
        } else {
            connection
                .execute(
                    "INSERT INTO product_files (
                        booth_id, variation, filename, size_bytes, downloaded, local_path
                     ) VALUES (?1, '', ?2, ?3, 1, ?4)",
                    params![booth_id, filename, size, path],
                )
                .sql()?;
        }
    }
    Ok(())
}

pub(crate) fn add_free_product(connection: &mut Connection, url: &str) -> Result<String, String> {
    let booth_id =
        booth_id_from_url(url).ok_or_else(|| "商品のURLからIDを読めません".to_string())?;
    if product_by_id(connection, &booth_id)?.is_some() {
        let _ = merge_flag(connection, &booth_id, "is_free", Some(true))?;
        return Ok(booth_id);
    }
    let incoming = IncomingProduct {
        booth_id: booth_id.clone(),
        title: format!("商品 {booth_id}"),
        shop: String::new(),
        description_snippet: String::new(),
        url: format!("https://booth.pm/ja/items/{booth_id}"),
        thumbnail_url: None,
        variations: Vec::new(),
        downloads: Vec::new(),
        is_free: Some(true),
        is_adult: None,
        unpublished: None,
    };
    upsert(connection, &incoming)?;
    Ok(booth_id)
}

#[derive(Debug)]
pub(crate) struct DownloadPlan {
    pub source: std::path::PathBuf,
    pub library: Option<std::path::PathBuf>,
    pub booth_id: String,
    pub variation: String,
}

pub(crate) fn prepare_download(
    connection: &mut Connection,
    notice: &DownloadFinished,
) -> Result<DownloadPlan, String> {
    let saved = settings(connection)?;
    if !saved.route_downloads {
        return Err("ダウンロードの自動整理はオフです".into());
    }
    if saved.library_folder.trim().is_empty() {
        return Err("ライブラリのフォルダを選んでから、自動整理をオンにしてください".into());
    }
    let booth_id = notice.booth_id.trim();
    if !crate::library_files::valid_booth_id(booth_id) {
        return Err("商品IDを特定できません".into());
    }
    if product_excluded(connection, booth_id)? {
        return Err("この商品は除外されています".into());
    }
    let source = std::path::PathBuf::from(notice.path.trim());
    let downloads = crate::library_files::downloads_folder()?;
    crate::library_files::staged_download_ok(&source, &downloads, booth_id)?;
    let actual_name = crate::library_files::safe_filename(
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    );
    let Some(filename) = actual_name else {
        return Err("ファイル名を読めません".into());
    };
    if let Some(claimed) = crate::library_files::safe_filename(&notice.filename) {
        if filename != claimed {
            return Err("ファイル名がパスと一致しません".into());
        }
    }
    let variation = crate::library_files::clean_variation(&notice.variation).unwrap_or_default();
    let title = notice
        .title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let title: String = title.chars().take(300).collect();
    if product_by_id(connection, booth_id)?.is_none() {
        let incoming = IncomingProduct {
            booth_id: booth_id.to_string(),
            title: if title.is_empty() {
                format!("商品 {booth_id}")
            } else {
                title.clone()
            },
            shop: String::new(),
            description_snippet: String::new(),
            url: format!("https://booth.pm/ja/items/{booth_id}"),
            thumbnail_url: None,
            variations: Vec::new(),
            downloads: Vec::new(),
            is_free: if notice.is_free == Some(true) {
                Some(true)
            } else {
                None
            },
            is_adult: if notice.is_adult == Some(true) {
                Some(true)
            } else {
                None
            },
            unpublished: None,
        };
        upsert(connection, &incoming)?;
    } else {
        if notice.is_free == Some(true) {
            let _ = merge_flag(connection, booth_id, "is_free", Some(true))?;
        }
        if notice.is_adult == Some(true) {
            let _ = merge_flag(connection, booth_id, "is_adult", Some(true))?;
        }
    }
    Ok(DownloadPlan {
        source,
        library: if saved.library_folder.trim().is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(saved.library_folder.trim()))
        },
        booth_id: booth_id.to_string(),
        variation,
    })
}

pub(crate) fn record_placed_download(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    path: &std::path::Path,
    size: Option<u64>,
) -> Result<(), String> {
    let disk_size = std::fs::metadata(path).ok().map(|meta| meta.len());
    let size = disk_size.or(size);
    let filename = crate::library_files::safe_filename(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
    .ok_or_else(|| "ファイル名を読めません".to_string())?;
    let variation = crate::library_files::clean_variation(variation).unwrap_or_default();
    if !variation.is_empty() {
        let _ = merge_variation(
            connection,
            booth_id,
            &IncomingVariation {
                name: variation.clone(),
                owned: true,
            },
        )?;
    }
    connection
        .execute(
            "INSERT INTO product_files (booth_id, variation, filename, size_bytes, downloaded, local_path)
             VALUES (?1, ?2, ?3, ?4, 1, ?5)
             ON CONFLICT(booth_id, variation, filename) DO UPDATE SET
               downloaded = 1,
               local_path = excluded.local_path,
               size_bytes = COALESCE(excluded.size_bytes, product_files.size_bytes)",
            params![
                booth_id,
                variation,
                filename,
                size.map(|value| value as i64),
                path.display().to_string()
            ],
        )
        .sql()?;
    let bytes = std::fs::metadata(path)
        .map(|meta| meta.len() as i64)
        .unwrap_or(0);
    connection
        .execute(
            "INSERT INTO local_files (
                path, filename, is_dir, size, linked_product_id, candidate_product_id,
                match_score, link_source, scanned_at
             ) VALUES (?1, ?2, 0, ?3, ?4, ?4, 1, 'download', ?5)
             ON CONFLICT(path) DO UPDATE SET
               filename = excluded.filename,
               size = excluded.size,
               linked_product_id = excluded.linked_product_id,
               candidate_product_id = excluded.candidate_product_id,
               match_score = 1,
               link_source = 'download',
               scanned_at = excluded.scanned_at",
            params![
                path.display().to_string(),
                filename,
                bytes,
                booth_id,
                now_iso()
            ],
        )
        .sql()?;
    Ok(())
}

pub(crate) const BROWSER_OFF_MESSAGE: &str =
    "内蔵ブラウザはオフです。拡張のダウンロードを使ってください";
pub(crate) const BROWSER_LOGIN_MESSAGE: &str =
    "ログインの期限が切れています。内蔵ブラウザでログインしてから、もう一度押してください";
pub(crate) const BROWSER_AGE_MESSAGE: &str =
    "年齢確認は自動では通過しません。内蔵ブラウザで自分で確認してから、もう一度押してください";
pub(crate) const BROWSER_DOWNLOAD_FAILED: &str =
    "ダウンロードできませんでした。内蔵ブラウザでログインしているか確認してください";
pub(crate) const BROWSER_BUSY_MESSAGE: &str = "別のファイルをダウンロード中です";
pub(crate) const BROWSER_MISSING_ID_MESSAGE: &str =
    "この行にはダウンロード番号がありません。拡張を更新して再インポートしてください";

#[derive(Debug, Clone)]
pub(crate) struct BrowserFilePlan {
    pub booth_id: String,
    pub variation: String,
    pub filename: String,
    pub downloadable_id: String,
}

pub(crate) fn plan_browser_download(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    filename: &str,
) -> Result<BrowserFilePlan, String> {
    let booth_id = booth_id.trim();
    if !crate::library_files::valid_booth_id(booth_id) {
        return Err("商品IDを特定できません".into());
    }
    if product_excluded(connection, booth_id)? {
        return Err("この商品は除外されています".into());
    }
    let saved = settings(connection)?;
    if !saved.builtin_browser {
        return Err(BROWSER_OFF_MESSAGE.into());
    }
    if !saved.route_downloads {
        return Err("ダウンロードの自動整理はオフです".into());
    }
    if saved.library_folder.trim().is_empty() {
        return Err("ライブラリのフォルダを選んでから、自動整理をオンにしてください".into());
    }
    let variation = crate::library_files::clean_variation(variation).unwrap_or_default();
    let filename = crate::library_files::safe_filename(filename)
        .ok_or_else(|| "ファイル名を読めません".to_string())?;
    let stored: Option<Option<String>> = connection
        .query_row(
            "SELECT downloadable_id FROM product_files
             WHERE booth_id = ?1 AND variation = ?2 AND filename = ?3",
            params![booth_id, variation, filename],
            |row| row.get(0),
        )
        .optional()
        .sql()?;
    let Some(stored) = stored else {
        return Err("ダウンロード項目がありません".into());
    };
    let downloadable_id = stored
        .as_deref()
        .filter(|id| crate::library_files::valid_booth_id(id))
        .map(str::to_string)
        .or_else(|| {
            crate::library_files::filename_is_number(&filename).then(|| filename.clone())
        })
        .ok_or_else(|| BROWSER_MISSING_ID_MESSAGE.to_string())?;
    Ok(BrowserFilePlan {
        booth_id: booth_id.to_string(),
        variation,
        filename,
        downloadable_id,
    })
}

/// A number-only row takes the real filename. A real filename is left as it is.
pub(crate) fn adopt_download_filename(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    old_filename: &str,
    new_filename: &str,
    downloadable_id: &str,
) -> Result<(), String> {
    let variation = crate::library_files::clean_variation(variation).unwrap_or_default();
    let old_filename = crate::library_files::safe_filename(old_filename)
        .ok_or_else(|| "ファイル名を読めません".to_string())?;
    let new_filename = crate::library_files::safe_filename(new_filename)
        .ok_or_else(|| "ファイル名を読めません".to_string())?;
    let downloadable_id = if crate::library_files::valid_booth_id(downloadable_id) {
        downloadable_id.to_string()
    } else {
        String::new()
    };
    let mut target = old_filename.clone();
    if old_filename != new_filename
        && filename_is_placeholder(&old_filename, &downloadable_id)
        && !filename_is_placeholder(&new_filename, &downloadable_id)
    {
        let clash: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_files
                 WHERE booth_id = ?1 AND variation = ?2 AND filename = ?3
                   AND NOT (variation = ?2 AND filename = ?4)",
                params![booth_id, variation, new_filename, old_filename],
                |row| row.get(0),
            )
            .sql()?;
        if clash == 0 {
            connection
                .execute(
                    "UPDATE product_files
                     SET filename = ?1, downloadable_id = COALESCE(NULLIF(?2, ''), downloadable_id)
                     WHERE booth_id = ?3 AND variation = ?4 AND filename = ?5",
                    params![new_filename, downloadable_id, booth_id, variation, old_filename],
                )
                .sql()?;
            target = new_filename;
        }
    }
    if !downloadable_id.is_empty() {
        connection
            .execute(
                "UPDATE product_files SET downloadable_id = ?1
                 WHERE booth_id = ?2 AND variation = ?3 AND filename = ?4
                   AND (downloadable_id IS NULL OR downloadable_id = '')",
                params![downloadable_id, booth_id, variation, target],
            )
            .sql()?;
    }
    Ok(())
}

pub(crate) fn commit_browser_download(
    connection: &mut Connection,
    plan: &BrowserFilePlan,
    source: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    let downloads = crate::library_files::downloads_folder()?;
    let source = crate::library_files::staged_download_ok(source, &downloads, &plan.booth_id)?;
    if crate::library_files::file_looks_like_login_page(&source) {
        let _ = std::fs::remove_file(&source);
        return Err(BROWSER_LOGIN_MESSAGE.into());
    }
    let actual = crate::library_files::safe_filename(
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
    .ok_or_else(|| "ファイル名を読めません".to_string())?;
    if !crate::library_files::is_packaged_asset(&actual) {
        let _ = std::fs::remove_file(&source);
        return Err(BROWSER_DOWNLOAD_FAILED.into());
    }
    adopt_download_filename(
        connection,
        &plan.booth_id,
        &plan.variation,
        &plan.filename,
        &actual,
        &plan.downloadable_id,
    )?;
    let notice = crate::model::DownloadFinished {
        booth_id: plan.booth_id.clone(),
        path: source.display().to_string(),
        filename: actual,
        size: std::fs::metadata(&source).ok().map(|meta| meta.len()),
        is_free: None,
        is_adult: None,
        variation: plan.variation.clone(),
        title: String::new(),
    };
    let ready = prepare_download(connection, &notice)?;
    let placed =
        crate::library_files::place_into_library(&ready.source, ready.library.as_deref(), &ready.booth_id)?;
    record_placed_download(
        connection,
        &ready.booth_id,
        &ready.variation,
        &placed,
        notice.size,
    )?;
    Ok(placed)
}

pub(crate) struct SweepReport {
    pub moved: u32,
    pub left: Vec<crate::model::StagedLeftover>,
}

pub(crate) fn route_staged_download(connection: &mut Connection, source: &Path) -> Result<(), String> {
    let booth_id = source
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let filename = crate::library_files::safe_filename(
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
    .ok_or_else(|| "ファイル名を読めません".to_string())?;
    if !crate::library_files::valid_booth_id(booth_id) {
        return Err("商品IDを特定できません".into());
    }
    let notice = DownloadFinished {
        booth_id: booth_id.to_string(),
        path: source.display().to_string(),
        filename,
        size: None,
        is_free: None,
        is_adult: None,
        variation: String::new(),
        title: String::new(),
    };
    let plan = prepare_download(connection, &notice)?;
    let placed = crate::library_files::place_into_library(
        &plan.source,
        plan.library.as_deref(),
        &plan.booth_id,
    )?;
    record_placed_download(
        connection,
        &plan.booth_id,
        &plan.variation,
        &placed,
        notice.size,
    )
}

/// One pass over `Downloads/VRCLibMan/<booth id>/<file>`. No network and no directory walk
/// past that exact shape. Files that fail the same checks as `/download-finished` stay put.
pub(crate) fn sweep_staged_downloads(connection: &mut Connection) -> SweepReport {
    let mut report = SweepReport {
        moved: 0,
        left: Vec::new(),
    };
    let Ok(downloads) = crate::library_files::downloads_folder() else {
        return report;
    };
    for path in crate::library_files::staged_download_candidates(&downloads) {
        let booth_id = path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        match route_staged_download(connection, &path) {
            Ok(()) => report.moved += 1,
            Err(reason) => report.left.push(crate::model::StagedLeftover {
                path: path.display().to_string(),
                booth_id,
                filename,
                reason,
            }),
        }
    }
    report
}

pub(crate) fn require_download(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    filename: &str,
) -> Result<(), String> {
    let variation = crate::library_files::clean_variation(variation).unwrap_or_default();
    let filename = crate::library_files::safe_filename(filename)
        .ok_or_else(|| "ファイル名を読めません".to_string())?;
    let found: Option<String> = connection
        .query_row(
            "SELECT filename FROM product_files
             WHERE booth_id = ?1 AND variation = ?2 AND filename = ?3",
            params![booth_id, variation, filename],
            |row| row.get(0),
        )
        .optional()
        .sql()?;
    if found.is_none() {
        return Err("ダウンロード項目がありません".into());
    }
    Ok(())
}

pub(crate) fn local_source_for(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    filename: &str,
) -> Result<std::path::PathBuf, String> {
    require_download(connection, booth_id, variation, filename)?;
    let variation = crate::library_files::clean_variation(variation).unwrap_or_default();
    let filename = crate::library_files::safe_filename(filename).unwrap();
    let stored: Option<String> = connection
        .query_row(
            "SELECT local_path FROM product_files
             WHERE booth_id = ?1 AND variation = ?2 AND filename = ?3",
            params![booth_id, variation, filename],
            |row| row.get(0),
        )
        .optional()
        .sql()?
        .flatten();
    if let Some(path) = stored.filter(|path| !path.is_empty()) {
        return Ok(std::path::PathBuf::from(path));
    }
    let scanned: Option<String> = connection
        .query_row(
            "SELECT path FROM local_files
             WHERE linked_product_id = ?1 AND filename = ?2 AND is_dir = 0
             LIMIT 1",
            params![booth_id, filename],
            |row| row.get(0),
        )
        .optional()
        .sql()?;
    scanned
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "手元にファイルがありません".into())
}

pub(crate) fn remember_file_size(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    filename: &str,
    size: u64,
) -> Result<(), String> {
    let variation = crate::library_files::clean_variation(variation).unwrap_or_default();
    let filename = crate::library_files::safe_filename(filename)
        .ok_or_else(|| "ファイル名を読めません".to_string())?;
    connection
        .execute(
            "UPDATE product_files SET size_bytes = ?1
             WHERE booth_id = ?2 AND variation = ?3 AND filename = ?4",
            params![size as i64, booth_id, variation, filename],
        )
        .sql()?;
    Ok(())
}

pub(crate) fn note_downloaded_file(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    _filename: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let size = std::fs::metadata(path).ok().map(|meta| meta.len());
    record_placed_download(connection, booth_id, variation, path, size)
}

fn booth_id_from_url(url: &str) -> Option<String> {
    let start = url.find("/items/")?;
    let rest = &url[start + "/items/".len()..];
    let id: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if crate::library_files::valid_booth_id(&id) {
        Some(id)
    } else {
        None
    }
}

pub(crate) fn knowledge(connection: &Connection) -> Result<Knowledge, String> {
    Ok(Knowledge {
        avatars: load_avatars(connection)?,
        base_bodies: load_bases(connection)?,
    })
}

pub(crate) fn save_avatar(
    connection: &mut Connection,
    input: &AvatarInput,
) -> Result<String, String> {
    let display = input.display_name.trim();
    if display.is_empty() || display.chars().count() > 80 {
        return Err("表示名は1〜80文字にしてください".into());
    }
    let aliases = clean_aliases(&input.aliases, display);
    let notes: String = input.notes.chars().take(400).collect();
    let base = normalize_base(connection, input.base_body.as_deref())?;
    let requested = input
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
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

pub(crate) fn save_base_body(
    connection: &Connection,
    input: &BaseBodyInput,
) -> Result<String, String> {
    let display = input.display_name.trim();
    if display.is_empty() || display.chars().count() > 80 {
        return Err("素体名は1〜80文字にしてください".into());
    }
    let note: String = input.note.chars().take(400).collect();
    let requested = input
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
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

pub(crate) fn merge_avatars(
    connection: &mut Connection,
    from_id: &str,
    into_id: &str,
) -> Result<(), String> {
    if from_id == into_id {
        return Err("同じアバターには統合できません".into());
    }
    let source = avatar_by_id(connection, from_id)?.ok_or("統合元のアバターが見つかりません")?;
    let mut target =
        avatar_by_id(connection, into_id)?.ok_or("統合先のアバターが見つかりません")?;
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

fn merge_catalog(connection: &Connection) -> Result<u32, String> {
    let parsed = crate::catalog::bundled()?;
    let added = merge_catalog_entries(connection, &parsed.entries)?;
    if setting(connection, "catalog_fetched")?.is_none() {
        set_setting(connection, "catalog_fetched", &parsed.fetched_on)?;
        set_setting(connection, "catalog_upstream", &parsed.upstream_updated)?;
        set_setting(
            connection,
            "catalog_count",
            &parsed.entries.len().to_string(),
        )?;
    }
    Ok(added)
}

pub(crate) fn install_catalog(
    connection: &mut Connection,
    parsed: &crate::catalog::ParsedCatalog,
) -> Result<CatalogRefresh, String> {
    let added = merge_catalog_entries(connection, &parsed.entries)?;
    let fetched = tokyo_date();
    let upstream = if parsed.upstream_updated.is_empty() {
        setting(connection, "catalog_upstream")?.unwrap_or_default()
    } else {
        parsed.upstream_updated.clone()
    };
    set_setting(connection, "catalog_fetched", &fetched)?;
    set_setting(connection, "catalog_upstream", &upstream)?;
    set_setting(
        connection,
        "catalog_count",
        &parsed.entries.len().to_string(),
    )?;
    settle_dictionary(connection)?;
    reresolve_all(connection)?;
    Ok(CatalogRefresh {
        added,
        total: parsed.entries.len() as u32,
        upstream_updated: upstream,
    })
}

fn merge_catalog_entries(
    connection: &Connection,
    entries: &[crate::catalog::CatalogEntry],
) -> Result<u32, String> {
    let avatars = load_avatars(connection)?;
    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
    for avatar in &avatars {
        for name in std::iter::once(&avatar.display_name).chain(avatar.aliases.iter()) {
            by_name
                .entry(crate::normalize::normalize_label(name))
                .or_default()
                .push(avatar.id.clone());
        }
    }
    let mut added = 0u32;
    for entry in entries {
        if let Some(existing) = avatar_id_for_booth(connection, &entry.booth_id)? {
            remember_catalog_alias(connection, &existing, entry)?;
            continue;
        }
        let key = crate::normalize::normalize_label(&entry.display_name);
        let hits = by_name.get(&key).cloned().unwrap_or_default();
        let mut unique: Vec<String> = hits;
        unique.sort();
        unique.dedup();
        if unique.len() == 1 && booth_count(connection, &unique[0])? == 0 {
            bind_booth(connection, &unique[0], &entry.booth_id)?;
            remember_catalog_alias(connection, &unique[0], entry)?;
            if avatar_notes(connection, &unique[0])?.trim().is_empty() {
                connection
                    .execute(
                        "UPDATE avatars SET notes = ?1 WHERE id = ?2",
                        params![crate::catalog::catalog_note(entry), unique[0]],
                    )
                    .sql()?;
            }
            continue;
        }
        let id = format!("mf-{}", entry.booth_id);
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
                 VALUES (?1, ?2, NULL, ?3, 1, ?4)",
                params![
                    id,
                    entry.display_name,
                    to_json(&entry.aliases)?,
                    crate::catalog::catalog_note(entry)
                ],
            )
            .sql()?;
        bind_booth(connection, &id, &entry.booth_id)?;
        by_name.entry(key).or_default().push(id.clone());
        for alias in &entry.aliases {
            by_name
                .entry(crate::normalize::normalize_label(alias))
                .or_default()
                .push(id.clone());
        }
        if inserted > 0 {
            added += 1;
        }
    }
    Ok(added)
}

fn remember_catalog_alias(
    connection: &Connection,
    avatar_id: &str,
    entry: &crate::catalog::CatalogEntry,
) -> Result<(), String> {
    let mut aliases = avatar_aliases(connection, avatar_id)?;
    for alias in std::iter::once(&entry.display_name).chain(entry.aliases.iter()) {
        if alias.is_empty() || aliases.iter().any(|existing| existing == alias) {
            continue;
        }
        let display = avatar_display(connection, avatar_id)?;
        if alias == &display {
            continue;
        }
        aliases.push(alias.clone());
    }
    aliases.truncate(32);
    connection
        .execute(
            "UPDATE avatars SET aliases_json = ?1 WHERE id = ?2",
            params![to_json(&aliases)?, avatar_id],
        )
        .sql()?;
    Ok(())
}

fn bind_booth(connection: &Connection, avatar_id: &str, booth_id: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR IGNORE INTO avatar_sources (booth_id, avatar_id) VALUES (?1, ?2)",
            params![booth_id, avatar_id],
        )
        .sql()?;
    Ok(())
}

fn avatar_id_for_booth(connection: &Connection, booth_id: &str) -> Result<Option<String>, String> {
    connection
        .query_row(
            "SELECT avatar_id FROM avatar_sources WHERE booth_id = ?1",
            params![booth_id],
            |row| row.get(0),
        )
        .optional()
        .sql()
}

fn booth_count(connection: &Connection, avatar_id: &str) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM avatar_sources WHERE avatar_id = ?1",
            params![avatar_id],
            |row| row.get(0),
        )
        .sql()
}

fn avatar_display(connection: &Connection, id: &str) -> Result<String, String> {
    connection
        .query_row(
            "SELECT display_name FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()
}

fn avatar_aliases(connection: &Connection, id: &str) -> Result<Vec<String>, String> {
    let json: String = connection
        .query_row(
            "SELECT aliases_json FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(parse_list(&json))
}

fn avatar_notes(connection: &Connection, id: &str) -> Result<String, String> {
    connection
        .query_row(
            "SELECT notes FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()
}

pub(crate) fn settle_dictionary(connection: &mut Connection) -> Result<u32, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id FROM products
             WHERE user_confirmed = 0 AND links_locked = 0
               AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
               AND (
                 extracted_at IS NULL
                 OR booth_id IN (SELECT booth_id FROM avatar_sources)
                 OR (category = 'avatar' AND extracted_at IS NOT NULL)
               )",
        )
        .sql()?;
    let ids: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    drop(statement);
    let mut settled = 0;
    for id in ids {
        // Already-classified avatars are selected broadly, then kept only when
        // the title is one quoted original-model name tied to an unowned link.
        // That check uses NFKC helpers in Rust rather than SQL LIKE.
        if !settle_dictionary_includes(connection, &id)? {
            continue;
        }
        if settle_one(connection, &id, false)? {
            settled += 1;
        }
    }
    Ok(settled)
}

fn settle_dictionary_includes(connection: &Connection, booth_id: &str) -> Result<bool, String> {
    let product = require_product(connection, booth_id)?;
    if product.extracted_at.is_none() || catalog_body(connection, booth_id)?.is_some() {
        return Ok(true);
    }
    repairable_extracted_original_body(connection, &product)
}

fn single_quoted_original_title(title: &str) -> bool {
    crate::compat::strong_category_override(title) == Some(Category::Avatar)
        && crate::normalize::quoted_segments(title).len() == 1
}

fn repairable_extracted_original_body(
    connection: &Connection,
    product: &Product,
) -> Result<bool, String> {
    if product.category != Some(Category::Avatar) || product.extracted_at.is_none() {
        return Ok(false);
    }
    if product.user_confirmed
        || product.links_locked
        || (product.category_locked && product.category != Some(Category::Avatar))
        || product.description_snippet == crate::model::UNAVAILABLE_DESCRIPTION
        || product_excluded(connection, &product.booth_id)?
        || !single_quoted_original_title(&product.title)
    {
        return Ok(false);
    }
    unowned_link_matches_quote(connection, product)
}

fn unowned_link_matches_quote(connection: &Connection, product: &Product) -> Result<bool, String> {
    let segments = crate::normalize::quoted_segments(&product.title);
    let Some(segment) = segments.first() else {
        return Ok(false);
    };
    if segments.len() != 1 {
        return Ok(false);
    }
    let quoted = crate::normalize::normalize_label(segment);
    if quoted.is_empty() {
        return Ok(false);
    }
    let mut statement = connection
        .prepare(
            "SELECT avatars.display_name, avatars.aliases_json, avatars.owned
             FROM product_avatars
             JOIN avatars ON avatars.id = product_avatars.avatar_id
             WHERE product_avatars.product_id = ?1",
        )
        .sql()?;
    let rows = statement
        .query_map(params![product.booth_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .sql()?;
    for row in rows {
        let (display, aliases_json, owned) = row.sql()?;
        if owned != 0 {
            continue;
        }
        if crate::normalize::normalize_label(&display) == quoted
            || parse_list(&aliases_json)
                .iter()
                .any(|alias| crate::normalize::normalize_label(alias) == quoted)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn skip_model(connection: &mut Connection, booth_id: &str) -> Result<bool, String> {
    if product_excluded(connection, booth_id)? {
        return Ok(true);
    }
    let product = require_product(connection, booth_id)?;
    if product.user_confirmed || product.links_locked {
        return Ok(false);
    }
    settle_one(connection, booth_id, true)
}

fn settle_one(connection: &mut Connection, booth_id: &str, redo: bool) -> Result<bool, String> {
    if product_excluded(connection, booth_id)? {
        return Ok(false);
    }
    let product = require_product(connection, booth_id)?;
    if classification_pinned(&product) || product.user_confirmed || product.links_locked {
        return Ok(false);
    }
    if let Some((avatar_id, display)) = catalog_body(connection, booth_id)? {
        if product.category_locked && product.category != Some(Category::Avatar) {
            return Ok(false);
        }
        write_catalog_body(connection, booth_id, &avatar_id, &display)?;
        return Ok(true);
    }
    // redo (skip_model) and startup repair both reach this before the model.
    // An already-classified original-model body must win here, not after another
    // extraction that would only rewrite the same unowned placeholder.
    if settle_named_original_body(connection, &product)? {
        return Ok(true);
    }
    if product.extracted_at.is_some() && !redo {
        return Ok(false);
    }
    // Other bodies outside the profile list still need a manual ownership mark.
    let avatars = load_avatars(connection)?;
    let pieces = crate::compat::explicit_dedication_pieces(&product.title, &avatars);
    if pieces.is_empty() {
        return Ok(false);
    }
    let category =
        crate::compat::category_for_listed_product(&product.title, &product.description_snippet);
    if matches!(category, Category::Other | Category::Avatar) {
        return Ok(false);
    }
    let threshold = settings(connection)?.confidence_threshold;
    apply_extraction(
        connection,
        booth_id,
        &Extraction {
            category,
            genre: None,
            compatible_avatars: pieces,
            is_generic: false,
            confidence: 0.95,
        },
        threshold,
    )?;
    Ok(true)
}

// A single quoted name in an explicit original-model title identifies the body
// without using compatibility names from its description as ownership evidence.
// extracted_at is intentionally not a guard: a model run can already have stored
// category avatar and an owned-<id> link while leaving avatars.owned at 0.
fn settle_named_original_body(connection: &mut Connection, product: &Product) -> Result<bool, String> {
    // A model-set avatar category is allowed, and so is an unlocked category
    // (unset, or not category_locked). A category locked to something else is not.
    if product_excluded(connection, &product.booth_id)?
        || product.user_confirmed
        || product.links_locked
        || (product.category_locked && product.category != Some(Category::Avatar))
        || product.description_snippet == crate::model::UNAVAILABLE_DESCRIPTION
        || !single_quoted_original_title(&product.title)
    {
        return Ok(false);
    }
    let name = crate::normalize::suggested_display_name(&product.title);
    if name == "名称未設定" || crate::normalize::is_blocked_avatar_label(&name) {
        return Ok(false);
    }
    let avatars = load_avatars(connection)?;
    let names = vec![name];
    let output = resolve_product(
        &ResolveInput {
            booth_id: &product.booth_id,
            title: &product.title,
            description: &product.description_snippet,
            category: Some(Category::Avatar),
            raw_names: &names,
            is_generic: false,
            confidence: Some(1.0),
            user_confirmed: true,
            threshold: settings(connection)?.confidence_threshold,
            allow_create: true,
        },
        &avatars,
    );
    // A failed name inference is an unresolved product, not a database failure.
    // In particular, do not fall back to other names outside the quotation.
    let Some(avatar_id) = output.avatar_ids.first() else {
        return Ok(false);
    };
    let tx = connection.transaction().sql()?;
    for draft in &output.created_avatars {
        insert_placeholder(&tx, &draft.id, &draft.display_name, &draft.aliases)?;
    }
    save_owned_body(&tx, product, avatar_id)?;
    tx.commit().sql()?;
    Ok(true)
}

fn catalog_body(
    connection: &Connection,
    booth_id: &str,
) -> Result<Option<(String, String)>, String> {
    connection
        .query_row(
            "SELECT avatars.id, avatars.display_name
             FROM avatar_sources
             JOIN avatars ON avatars.id = avatar_sources.avatar_id
             WHERE avatar_sources.booth_id = ?1",
            params![booth_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .sql()
}

fn write_catalog_body(
    connection: &Connection,
    booth_id: &str,
    avatar_id: &str,
    display: &str,
) -> Result<(), String> {
    let now = now_iso();
    connection
        .execute(
            "UPDATE products SET category = 'avatar', compatible_avatars_json = ?1,
             unresolved_names_json = '[]', is_generic = 0, confidence = 1, needs_review = 0,
             user_confirmed = 1, extracted_at = COALESCE(extracted_at, ?2), updated_at = ?2
             WHERE booth_id = ?3 AND category_locked = 0",
            params![to_json(&vec![display.to_string()])?, now, booth_id],
        )
        .sql()?;
    replace_links(connection, booth_id, &[avatar_id.to_string()], "catalog")?;
    connection
        .execute(
            "UPDATE avatars SET owned = 1 WHERE id = ?1",
            params![avatar_id],
        )
        .sql()?;
    Ok(())
}

pub(crate) fn mark_owned_body(
    connection: &mut Connection,
    booth_id: &str,
) -> Result<String, String> {
    let product = require_product(connection, booth_id)?;
    if matches!(
        product.category,
        Some(
            Category::Outfit
                | Category::Hair
                | Category::Accessory
                | Category::Texture
                | Category::Gimmick
                | Category::World
                | Category::Other
        )
    ) {
        return Err("この商品はアバター本体ではない分類です".into());
    }
    let avatar_id = if let Some((id, _)) = catalog_body(connection, booth_id)? {
        id
    } else if product.links_locked {
        let links = link_ids(connection, booth_id)?;
        links
            .into_iter()
            .next()
            .ok_or_else(|| "結び付いたアバターがありません".to_string())?
    } else {
        let avatars = load_avatars(connection)?;
        let threshold = settings(connection)?.confidence_threshold;
        let output = resolve_product(
            &ResolveInput {
                booth_id,
                title: product.title.as_str(),
                description: product.description_snippet.as_str(),
                category: Some(Category::Avatar),
                raw_names: &[],
                is_generic: false,
                confidence: Some(1.0),
                user_confirmed: true,
                threshold,
                allow_create: true,
            },
            &avatars,
        );
        let Some(id) = output.avatar_ids.first().cloned() else {
            return Err("アバター名を商品名から取り出せませんでした".into());
        };
        for draft in &output.created_avatars {
            insert_placeholder(connection, &draft.id, &draft.display_name, &draft.aliases)?;
        }
        id
    };
    save_owned_body(connection, &product, &avatar_id)?;
    Ok(avatar_id)
}

fn save_owned_body(connection: &Connection, product: &Product, avatar_id: &str) -> Result<(), String> {
    let booth_id = &product.booth_id;
    connection
        .execute(
            "UPDATE avatars SET owned = 1 WHERE id = ?1",
            params![avatar_id],
        )
        .sql()?;
    let display = avatar_display(connection, &avatar_id)?;
    let now = now_iso();
    connection
        .execute(
            "UPDATE products SET category = 'avatar', compatible_avatars_json = ?1,
             unresolved_names_json = '[]', is_generic = 0, confidence = 1, needs_review = 0,
             user_confirmed = 1, extracted_at = COALESCE(extracted_at, ?2), updated_at = ?2
             WHERE booth_id = ?3 AND category_locked = 0",
            params![to_json(&vec![display])?, now, booth_id],
        )
        .sql()?;
    if !product.links_locked {
        replace_links(connection, booth_id, &[avatar_id.to_string()], "owned")?;
    }
    Ok(())
}

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
    let settings = settings(connection)?;
    Ok(SetupStatus {
        complete: setting(connection, "setup_complete")?.as_deref() == Some("1"),
        model: settings.ollama_model,
        default_model: crate::model::DEFAULT_OLLAMA_MODEL.into(),
        backend: settings.llm_backend,
    })
}

pub(crate) fn protected_names(connection: &Connection) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for avatar in load_avatars(connection)? {
        if !avatar.display_name.trim().is_empty() {
            names.push(avatar.display_name);
        }
        names.extend(avatar.aliases.into_iter().filter(|name| !name.trim().is_empty()));
    }
    Ok(names)
}

pub(crate) fn mark_guardrail_review(connection: &Connection, booth_id: &str) -> Result<(), String> {
    let now = now_iso();
    connection
        .execute(
            "UPDATE products SET
               category = CASE WHEN category_locked = 1 THEN category ELSE COALESCE(category, 'other') END,
               needs_review = 1,
               confidence = 0.2,
               user_confirmed = 0,
               extracted_at = ?1,
               updated_at = ?1
             WHERE booth_id = ?2",
            params![now, booth_id],
        )
        .sql()?;
    Ok(())
}

pub(crate) fn set_setup_complete(connection: &Connection, complete: bool) -> Result<(), String> {
    set_setting(
        connection,
        "setup_complete",
        if complete { "1" } else { "0" },
    )
}

pub(crate) fn avatar_candidates(connection: &Connection) -> Result<Vec<AvatarCandidate>, String> {
    let products = load_products(connection)?;
    let mut counts: HashMap<String, u32> = HashMap::new();
    for product in products {
        let mut seen = HashSet::new();
        for name in product.unresolved_names {
            if name.trim().is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            *counts.entry(name).or_default() += 1;
        }
    }
    let mut listed: Vec<AvatarCandidate> = counts
        .into_iter()
        .map(|(name, product_count)| AvatarCandidate {
            name,
            product_count,
        })
        .collect();
    listed.sort_by(|left, right| {
        right
            .product_count
            .cmp(&left.product_count)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(listed)
}

pub(crate) fn register_candidate(
    connection: &mut Connection,
    name: &str,
) -> Result<String, String> {
    let (display, aliases) = crate::catalog::split_bilingual_name(name);
    let display = if display.is_empty() {
        name.trim().to_string()
    } else {
        display
    };
    if display.is_empty() || display.chars().count() > 80 {
        return Err("アバター名を確認してください".into());
    }
    if let Some(existing) = unique_avatar_named(connection, &display)? {
        remember_alias(connection, &existing, name)?;
        for alias in &aliases {
            remember_alias(connection, &existing, alias)?;
        }
        reresolve_all(connection)?;
        return Ok(existing);
    }
    let id = generate_id(connection, &display)?;
    connection
        .execute(
            "INSERT INTO avatars (id, display_name, base_body, aliases_json, is_builtin, notes)
             VALUES (?1, ?2, NULL, ?3, 0, '')",
            params![id, display, to_json(&aliases)?],
        )
        .sql()?;
    reresolve_all(connection)?;
    Ok(id)
}

pub(crate) fn alias_candidate(
    connection: &mut Connection,
    name: &str,
    avatar_id: &str,
) -> Result<(), String> {
    if avatar_by_id(connection, avatar_id)?.is_none() {
        return Err("アバターが見つかりません".into());
    }
    remember_alias(connection, avatar_id, name)?;
    let (display, aliases) = crate::catalog::split_bilingual_name(name);
    if display != name {
        remember_alias(connection, avatar_id, &display)?;
    }
    for alias in aliases {
        remember_alias(connection, avatar_id, &alias)?;
    }
    reresolve_all(connection)?;
    Ok(())
}

fn unique_avatar_named(connection: &Connection, display: &str) -> Result<Option<String>, String> {
    let key = crate::normalize::normalize_label(display);
    let avatars = load_avatars(connection)?;
    let mut hits = Vec::new();
    for avatar in avatars {
        let matches = std::iter::once(&avatar.display_name)
            .chain(avatar.aliases.iter())
            .any(|name| crate::normalize::normalize_label(name) == key);
        if matches {
            hits.push(avatar.id);
        }
    }
    hits.sort();
    hits.dedup();
    if hits.len() == 1 {
        Ok(hits.pop())
    } else {
        Ok(None)
    }
}

fn remember_alias(connection: &Connection, avatar_id: &str, alias: &str) -> Result<(), String> {
    let alias = alias.trim();
    if alias.is_empty() {
        return Ok(());
    }
    let display = avatar_display(connection, avatar_id)?;
    if alias == display {
        return Ok(());
    }
    let mut aliases = avatar_aliases(connection, avatar_id)?;
    if aliases.iter().any(|existing| existing == alias) {
        return Ok(());
    }
    aliases.push(alias.to_string());
    aliases.truncate(32);
    connection
        .execute(
            "UPDATE avatars SET aliases_json = ?1 WHERE id = ?2",
            params![to_json(&aliases)?, avatar_id],
        )
        .sql()?;
    Ok(())
}

fn tokyo_date() -> String {
    let offset = chrono::FixedOffset::east_opt(9 * 3600)
        .unwrap_or(chrono::FixedOffset::east_opt(0).unwrap());
    chrono::Utc::now()
        .with_timezone(&offset)
        .format("%Y-%m-%d")
        .to_string()
}

fn decide_category(existing: &Product, llm: Category, confidence: f64) -> Option<Category> {
    if existing.category_locked {
        return existing.category;
    }
    if let Some(category) = crate::compat::strong_category_override(&existing.title) {
        return Some(category);
    }
    if llm == Category::Avatar && crate::normalize::is_avatar_count_title(&existing.title) {
        return Some(Category::Other);
    }
    if crate::compat::world_hint_overrides(
        &existing.title,
        &existing.description_snippet,
        llm,
        confidence,
    ) {
        return Some(Category::World);
    }
    Some(llm)
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
    Ok(())
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
    let mut statement = connection.prepare(&format!(
        "{PRODUCT_SELECT} WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)"
    )).sql()?;
    let rows = statement.query_map([], map_product).sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

fn load_avatars(connection: &Connection) -> Result<Vec<Avatar>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, display_name, base_body, aliases_json, is_builtin, owned, notes FROM avatars",
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
    use crate::view::{avatar_cards, avatar_detail, products_by_ids, queue_page};

    #[test]
    fn bookmarks_persist_edits_and_intentionally_empty_lists() {
        let connection = open_memory().unwrap();
        assert_eq!(browser_bookmarks(&connection).unwrap().len(), 2);
        save_browser_bookmark(&connection, None, "テストのサイト", "https://example.com/path?q=1").unwrap();
        ensure_ready(&connection).unwrap();
        assert!(browser_bookmarks(&connection).unwrap().iter().any(|item| item.title == "テストのサイト" && item.url == "https://example.com/path?q=1"));
        save_browser_bookmark(&connection, Some("https://example.com/path?q=1"), "変更した名前", "https://example.com/other").unwrap();
        assert!(browser_bookmarks(&connection).unwrap().iter().any(|item| item.title == "変更した名前"));
        let before = browser_bookmarks(&connection).unwrap();
        assert!(save_browser_bookmark(&connection, None, "不正", "javascript:alert(1)").is_err());
        assert_eq!(browser_bookmarks(&connection).unwrap(), before);
        for item in before { delete_browser_bookmark(&connection, &item.url).unwrap(); }
        ensure_ready(&connection).unwrap();
        assert!(browser_bookmarks(&connection).unwrap().is_empty());
    }

    #[test]
    fn version_one_migration_preserves_library_and_saved_model() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        connection.execute_batch("PRAGMA user_version = 1;
                INSERT INTO settings VALUES ('ollama_model', 'gemma4:12b');
                INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('9001', '既存商品', 't', 't');
                INSERT INTO local_files (path, filename, linked_product_id, link_source, scanned_at) VALUES ('/tmp/existing.zip', 'existing.zip', '9001', 'manual', 't');").unwrap();
        migrate(&connection).unwrap();
        migrate(&connection).unwrap();
        ensure_ready(&connection).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
        let unpublished: Option<i64> = connection
            .query_row(
                "SELECT unpublished FROM products WHERE booth_id = '9001'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unpublished, None);
        assert_eq!(require_product(&connection, "9001").unwrap().genre.as_deref(), Some("other"));
        assert_eq!(settings(&connection).unwrap().ollama_model, "gemma4:12b");
        assert_eq!(managed_products(&connection).unwrap().len(), 1);
        assert!(!product_excluded(&connection, "9001").unwrap());
        assert_eq!(
            load_files(&connection).unwrap()[0].link_source.as_deref(),
            Some("manual")
        );
    }

    #[test]
    fn exclusions_hide_products_from_views_and_jobs_and_restore_without_data_loss() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "1001",
            &extraction(Category::Avatar, &["ライム"], false, 0.99),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "1004",
            &extraction(Category::Outfit, &["ライム"], false, 0.99),
            0.6,
        )
        .unwrap();
        let original = require_product(&connection, "1004").unwrap();
        let before = load_snapshot(&connection).unwrap();
        set_product_excluded(&mut connection, "1004", true).unwrap();
        set_product_excluded(&mut connection, "1004", true).unwrap();
        let excluded = load_snapshot(&connection).unwrap();
        assert_eq!(excluded.products.len(), before.products.len() - 1);
        assert!(excluded.links.iter().all(|link| link.product_id != "1004"));
        assert!(avatar_detail(&excluded, "manuka")
            .unwrap()
            .unsupported_outfits
            .iter()
            .all(|item| item.booth_id != "1004"));
        assert!(
            crate::view::product_options(&excluded)
                .iter()
                .all(|p| p.booth_id != "1004")
        );
        assert!(
            products_for_extract(&connection, ExtractTarget::All)
                .unwrap()
                .iter()
                .all(|p| p.booth_id != "1004")
        );
        assert!(
            products_for_extract(&connection, ExtractTarget::Unclassified)
                .unwrap()
                .iter()
                .all(|p| p.booth_id != "1004")
        );
        assert!(
            managed_products(&connection)
                .unwrap()
                .iter()
                .any(|p| p.booth_id == "1004" && p.excluded)
        );
        apply_extraction(
            &mut connection,
            "1004",
            &extraction(Category::Other, &[], true, 0.1),
            0.6,
        )
        .unwrap();
        reresolve_all(&mut connection).unwrap();
        assert_eq!(
            require_product(&connection, "1004").unwrap().category,
            original.category
        );
        set_product_excluded(&mut connection, "1004", false).unwrap();
        let restored = load_snapshot(&connection).unwrap();
        assert_eq!(restored.products.len(), before.products.len());
        assert!(avatar_detail(&restored, "manuka")
            .unwrap()
            .unsupported_outfits
            .iter()
            .any(|item| item.booth_id == "1004"));
        assert!(
            restored
                .links
                .iter()
                .any(|link| link.product_id == "1004" && link.avatar_id == "lime")
        );
        assert_eq!(
            require_product(&connection, "1004").unwrap().category,
            original.category
        );
        assert!(set_product_excluded(&mut connection, "9999", true).is_err());
    }

    #[test]
    fn excluded_body_stays_excluded_after_reimport_and_does_not_count_as_owned() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        mark_owned_body(&mut connection, "1001").unwrap();
        assert!(
            load_snapshot(&connection)
                .unwrap()
                .avatars
                .iter()
                .any(|a| a.id == "lime" && a.owned)
        );
        set_product_excluded(&mut connection, "1001", true).unwrap();
        let updated = SAMPLE.replace("共通素体", "更新された説明");
        import_products(&mut connection, &parse_export(&updated).unwrap()).unwrap();
        ensure_ready(&connection).unwrap();
        assert!(product_excluded(&connection, "1001").unwrap());
        let snapshot = load_snapshot(&connection).unwrap();
        assert!(snapshot.products.iter().all(|p| p.booth_id != "1001"));
        assert!(snapshot.avatars.iter().any(|a| a.id == "lime" && !a.owned));
        assert_eq!(crate::view::library_stats(&snapshot, "fixture").products, 5);
        set_product_excluded(&mut connection, "1001", false).unwrap();
        assert!(
            load_snapshot(&connection)
                .unwrap()
                .products
                .iter()
                .any(|p| p.booth_id == "1001")
        );
    }

    #[test]
    fn rescanning_preserves_excluded_file_links_without_reintroducing_them() {
        let mut connection = open_memory().unwrap();
        import_products(
            &mut connection,
            &parse_export(&SAMPLE.replace("1004", "91004").replace("1005", "91005")).unwrap(),
        )
        .unwrap();
        let entry = |id: &str| crate::scan::ScanEntry {
            path: format!("/tmp/vrclibman-exclusions/{id}.zip"),
            filename: format!("{id}.zip"),
            is_dir: false,
            size: 10,
            labels: vec![format!("{id}.zip")],
        };
        let mut outcome = ScanOutcome {
            entries: vec![entry("91004"), entry("91005")],
            truncated: false,
        };
        apply_scan(&mut connection, "/tmp/vrclibman-exclusions", &outcome).unwrap();
        set_product_excluded(&mut connection, "91004", true).unwrap();
        outcome.entries.push(crate::scan::ScanEntry {
            path: "/tmp/vrclibman-exclusions/new-1004.zip".into(),
            ..entry("91004")
        });
        apply_scan(&mut connection, "/tmp/vrclibman-exclusions", &outcome).unwrap();
        assert_eq!(load_files(&connection).unwrap().len(), 2);
        assert_eq!(load_snapshot(&connection).unwrap().files.len(), 1);
        set_product_excluded(&mut connection, "91004", false).unwrap();
        let restored = load_snapshot(&connection).unwrap();
        assert_eq!(restored.files.len(), 2);
        assert!(
            restored
                .files
                .iter()
                .any(|f| f.linked_product_id.as_deref() == Some("91004"))
        );
    }

    #[test]
    fn stale_scan_cannot_revert_the_folder_or_replace_newer_results() {
        let mut connection = open_memory().unwrap();
        let outcome = ScanOutcome {
            entries: vec![crate::scan::ScanEntry {
                path: "/tmp/vrclibman-new/item.zip".into(),
                filename: "item.zip".into(),
                is_dir: false,
                size: 10,
                labels: vec!["item".into()],
            }],
            truncated: false,
        };
        set_setting(&connection, "library_folder", "/tmp/vrclibman-new").unwrap();
        apply_library_scan(&mut connection, "/tmp/vrclibman-new", &outcome).unwrap();
        let stale = ScanOutcome {
            entries: vec![],
            truncated: false,
        };
        assert!(apply_library_scan(&mut connection, "/tmp/vrclibman-old", &stale).is_err());
        assert_eq!(
            settings(&connection).unwrap().library_folder,
            "/tmp/vrclibman-new"
        );
        assert_eq!(load_files(&connection).unwrap().len(), 1);
    }

    #[test]
    fn excluded_products_cannot_start_or_route_downloads() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        set_product_excluded(&mut connection, "1004", true).unwrap();
        set_setting(&connection, "route_downloads", "1").unwrap();
        set_setting(
            &connection,
            "library_folder",
            "/tmp/vrclibman-excluded-downloads",
        )
        .unwrap();
        let notice: DownloadFinished = serde_json::from_value(serde_json::json!({
            "boothId": "1004", "path": "/tmp/no-file-should-be-touched.zip"
        }))
        .unwrap();
        assert!(
            prepare_download(&mut connection, &notice)
                .unwrap_err()
                .contains("除外")
        );
        assert!(
            plan_browser_download(&connection, "1004", "", "body.zip")
                .unwrap_err()
                .contains("除外")
        );
    }

    #[test]
    fn settings_patches_merge_current_values_and_validate_before_writing() {
        let connection = open_memory().unwrap();
        update_settings(
            &connection,
            &SettingsPatch {
                ollama_model: Some("gemma4:12b".into()),
                ..Default::default()
            },
        )
        .unwrap();
        set_setting(&connection, "library_folder", "/unavailable/saved/library").unwrap();
        set_setting(&connection, "builtin_browser", "1").unwrap();
        let saved = update_settings(
            &connection,
            &SettingsPatch {
                confidence_threshold: Some(0.75),
                background_mode: Some(true),
                launch_at_login: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(saved.ollama_model, "gemma4:12b");
        assert_eq!(saved.confidence_threshold, 0.75);
        assert!(saved.builtin_browser && saved.background_mode && saved.launch_at_login);
        let invalid = SettingsPatch {
            ollama_model: Some("invalid model!".into()),
            confidence_threshold: Some(0.1),
            ..Default::default()
        };
        assert!(update_settings(&connection, &invalid).is_err());
        assert_eq!(settings(&connection).unwrap().confidence_threshold, 0.75);
        assert!(
            update_settings(
                &connection,
                &SettingsPatch {
                    library_folder: Some("relative".into()),
                    ..Default::default()
                }
            )
            .is_err()
        );
        let saved = update_settings(
            &connection,
            &SettingsPatch {
                background_mode: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!saved.background_mode && !saved.launch_at_login);
    }

    #[test]
    fn failed_settings_write_rolls_back_the_entire_patch() {
        let connection = open_memory().unwrap();
        connection.execute_batch("CREATE TRIGGER fail_settings BEFORE UPDATE ON settings WHEN NEW.key = 'confidence_threshold' BEGIN SELECT RAISE(ABORT, 'fixture write failure'); END;").unwrap();
        let patch = SettingsPatch {
            ollama_model: Some("qwen3.5:4b".into()),
            confidence_threshold: Some(0.75),
            ..Default::default()
        };
        assert!(update_settings(&connection, &patch).is_err());
        let saved = settings(&connection).unwrap();
        assert_eq!(saved.ollama_model, "qwen3.5:9b");
        assert_eq!(saved.confidence_threshold, 0.6);
    }

    #[test]
    fn products_by_category_returns_only_requested_category() {
        let connection = open_memory().unwrap();
        for (booth_id, title, category) in [
            ("910001", "二番目のワールド", "world"),
            ("910002", "衣装", "outfit"),
            ("910003", "一番目のワールド", "world"),
        ] {
            connection
                .execute(
                    "INSERT INTO products
                     (booth_id, title, category, imported_at, updated_at)
                     VALUES (?1, ?2, ?3, '2026-01-01', '2026-01-01')",
                    params![booth_id, title, category],
                )
                .unwrap();
        }

        assert_eq!(
            product_ids_by_category(&connection, Category::World).unwrap(),
            vec!["910003".to_string(), "910001".to_string()]
        );
    }

    #[test]
    fn path_compare_folds_case_only_on_windows() {
        assert_eq!(
            normalize_path_key_os("/Users/A/VRCLibMan/File.ZIP", false),
            "/Users/A/VRCLibMan/File.ZIP"
        );
        assert!(is_under_os(
            "/Users/A/VRCLibMan/File.ZIP",
            "/Users/A/VRCLibMan",
            false
        ));
        assert!(!is_under_os(
            "/Users/A/VRCLibMan/File.ZIP",
            "/users/a/vrclibman",
            false
        ));
        assert!(!is_under_os(
            "/Users/A/Library/a.txt",
            "/Users/A/Lib",
            false
        ));
        assert_eq!(
            normalize_path_key_os("C:/Users/A/VRCLibMan/File.ZIP", true),
            "c:\\users\\a\\vrclibman\\file.zip"
        );
        assert!(is_under_os(
            "C:/Users/A/Lib/a.txt",
            "c:\\users\\a\\lib",
            true
        ));
        assert!(!is_under_os(
            "C:/Users/A/Library/a.txt",
            "C:/Users/A/Lib",
            true
        ));
    }

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
            genre: None,
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
            &extraction(
                Category::Avatar,
                &["ライム", "プラム", "シフォン", "ショコラ"],
                false,
                0.93,
            ),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "1002",
            &extraction(Category::Avatar, &["プラム"], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "1003",
            &extraction(Category::Avatar, &["マヌカ"], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "1004",
            &extraction(Category::Outfit, &["ライム"], false, 0.88),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "1005",
            &extraction(Category::Gimmick, &[], true, 0.84),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "1006",
            &extraction(Category::Hair, &["しなの"], false, 0.86),
            0.6,
        )
        .unwrap();

        let snapshot = load_snapshot(&mut connection).unwrap();
        let lime_links: Vec<_> = snapshot
            .links
            .iter()
            .filter(|link| link.product_id == "1001")
            .map(|link| link.avatar_id.as_str())
            .collect();
        assert_eq!(lime_links, vec!["lime"]);

        let cards = avatar_cards(&snapshot);
        assert!(cards.iter().any(|card| card.id == "lime" && !card.owned));
        assert!(!cards.iter().any(|card| card.id == "plum"));
        assert!(cards.iter().any(|card| card.id == "shinano" && !card.owned));

        let plum = avatar_detail(&snapshot, "plum").unwrap();
        assert!(
            plum.sections
                .iter()
                .flat_map(|section| &section.items)
                .any(|item| { item.booth_id == "1004" && item.via_base_body })
        );
        let manuka = avatar_detail(&snapshot, "manuka").unwrap();
        assert!(
            !manuka
                .sections
                .iter()
                .flat_map(|section| &section.items)
                .any(|item| item.booth_id == "1004")
        );
        assert!(
            manuka
                .generic_items
                .iter()
                .any(|item| item.booth_id == "1005")
        );
    }

    #[test]
    fn manual_link_survives_reresolve_and_description_change_clears_extraction() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "1004",
            &extraction(Category::Outfit, &["ライム"], false, 0.9),
            0.6,
        )
        .unwrap();
        update_product(
            &mut connection,
            &ProductUpdate {
                booth_id: "1004".into(),
                category: Some("outfit".into()),
                genre: None,
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

    #[test]
    fn manual_genre_lock_survives_extraction_and_genre_query_excludes_products() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        update_product(&mut connection, &ProductUpdate {
            booth_id: "1004".into(), category: Some("outfit".into()), genre: Some("shader".into()),
            is_generic: None, avatar_ids: None, confirm: None,
        }).unwrap();
        apply_extraction(&mut connection, "1004", &Extraction {
            category: Category::Outfit, genre: Some("outfit".into()), compatible_avatars: vec![],
            is_generic: false, confidence: 0.9,
        }, 0.6).unwrap();
        let product = require_product(&connection, "1004").unwrap();
        assert_eq!(product.genre.as_deref(), Some("shader"));
        assert!(product.genre_locked);
        assert!(product_ids_by_genre(&connection, "asset", Some("shader")).unwrap().contains(&"1004".into()));
        set_product_excluded(&mut connection, "1004", true).unwrap();
        assert!(!product_ids_by_genre(&connection, "asset", Some("shader")).unwrap().contains(&"1004".into()));
    }

    #[test]
    fn world_genre_query_lists_missing_or_foreign_genres_as_unset() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        update_product(&mut connection, &ProductUpdate {
            booth_id: "1004".into(), category: Some("world".into()), genre: Some("skybox".into()),
            is_generic: None, avatar_ids: None, confirm: None,
        }).unwrap();
        let has = |ids: Vec<String>| ids.contains(&"1004".to_string());
        assert!(has(product_ids_by_genre(&connection, "world", Some("skybox")).unwrap()));
        assert!(!has(product_ids_by_genre(&connection, "world", Some(crate::genre::UNSET)).unwrap()));
        connection.execute("UPDATE products SET genre = 'hair' WHERE booth_id = '1004'", []).unwrap();
        assert!(has(product_ids_by_genre(&connection, "world", Some(crate::genre::UNSET)).unwrap()));
        assert!(has(product_ids_by_genre(&connection, "world", None).unwrap()));
        assert!(!has(product_ids_by_genre(&connection, "asset", Some(crate::genre::UNSET)).unwrap()));
        connection.execute("UPDATE products SET genre = NULL WHERE booth_id = '1004'", []).unwrap();
        assert!(has(product_ids_by_genre(&connection, "world", Some(crate::genre::UNSET)).unwrap()));
        assert!(!has(product_ids_by_genre(&connection, "world", Some("skybox")).unwrap()));
    }

    #[test]
    fn sample_export_on_file_database_reaches_list_and_detail() {
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-sample-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        std::fs::create_dir_all(&dir.0).unwrap();
        let json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../examples/sample-booth-export.json"),
        )
        .unwrap();
        let parsed = parse_export(&json).unwrap();
        assert_eq!(parsed.products.len(), 7);

        let db_path = dir.0.join("library.sqlite");
        let mut connection = open(&db_path).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        let report = import_products(&mut connection, &parsed).unwrap();
        assert_eq!(report.inserted, 7);
        assert_eq!(report.skipped, 0);
        let before = load_snapshot(&mut connection).unwrap();
        let mut owned_ids: Vec<_> = avatar_cards(&before)
            .into_iter()
            .filter(|card| card.owned)
            .map(|card| card.id)
            .collect();
        owned_ids.sort();
        assert_eq!(
            owned_ids,
            vec!["lime".to_string(), "manuka".to_string(), "plum".to_string()]
        );
        assert_eq!(queue_page(&before, "unextracted", 0, 20).unextracted, 4);
        assert!(
            before
                .products
                .iter()
                .any(|product| product.booth_id == "4876459"
                    && product.description_snippet.contains("共通素体"))
        );

        // サンプルの説明文に書いてある名前だけを、抽出結果として渡す。
        let facts = [
            (
                "4876459",
                extraction(
                    Category::Avatar,
                    &["ライム", "プラム", "シフォン", "ショコラ"],
                    false,
                    0.93,
                ),
            ),
            (
                "7770415",
                extraction(Category::Avatar, &["プラム", "ライム"], false, 0.9),
            ),
            (
                "5058077",
                extraction(Category::Avatar, &["マヌカ"], false, 0.9),
            ),
            (
                "8800101",
                extraction(
                    Category::Outfit,
                    &["しなの", "ライム", "マヌカ"],
                    false,
                    0.88,
                ),
            ),
            (
                "8800102",
                extraction(Category::Hair, &["マヌカ"], false, 0.86),
            ),
            ("8800103", extraction(Category::Gimmick, &[], true, 0.84)),
            (
                "8800104",
                extraction(Category::Outfit, &["架空ちゃん"], false, 0.9),
            ),
        ];
        for (booth_id, extraction) in facts {
            apply_extraction(&mut connection, booth_id, &extraction, 0.6).unwrap();
        }

        drop(connection);
        let mut connection = open(&db_path).unwrap();
        let again = import_products(&mut connection, &parsed).unwrap();
        assert_eq!(again.unchanged, 7);
        let snapshot = load_snapshot(&mut connection).unwrap();
        let mut ids: Vec<_> = avatar_cards(&snapshot)
            .into_iter()
            .map(|card| card.id)
            .collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![
                "lime".to_string(),
                "manuka".to_string(),
                "plum".to_string(),
                "shinano".to_string()
            ]
        );
        assert!(
            snapshot
                .avatars
                .iter()
                .any(|avatar| avatar.id == "shinano" && !avatar.owned)
        );
        assert!(
            snapshot
                .avatars
                .iter()
                .any(|avatar| avatar.id == "lime" && avatar.owned)
        );
        assert!(snapshot.avatars.iter().any(|avatar| avatar.id == "shinano"));
        assert!(
            snapshot
                .avatars
                .iter()
                .all(|avatar| avatar.id != "owned-8800104")
        );

        let lime = avatar_detail(&snapshot, "lime").unwrap();
        assert!(
            lime.owned_products
                .iter()
                .any(|item| item.booth_id == "4876459")
        );
        assert!(lime.siblings.iter().any(|sibling| sibling.id == "plum"));
        assert!(lime.siblings.iter().all(|sibling| sibling.id != "manuka"));
        let lime_outfits = section_items(&lime, "outfit");
        assert_eq!(lime_outfits, vec![("8800101".to_string(), false)]);
        assert!(section_items(&lime, "hair").is_empty());
        assert!(
            lime.generic_items
                .iter()
                .any(|item| item.booth_id == "8800103")
        );

        let plum = avatar_detail(&snapshot, "plum").unwrap();
        assert_eq!(
            section_items(&plum, "outfit"),
            vec![("8800101".to_string(), true)]
        );
        assert!(
            plum.generic_items
                .iter()
                .any(|item| item.booth_id == "8800103")
        );

        let manuka = avatar_detail(&snapshot, "manuka").unwrap();
        assert_eq!(
            section_items(&manuka, "outfit"),
            vec![("8800101".to_string(), false)]
        );
        assert_eq!(
            section_items(&manuka, "hair"),
            vec![("8800102".to_string(), false)]
        );
        assert!(
            manuka
                .generic_items
                .iter()
                .any(|item| item.booth_id == "8800103")
        );
        assert!(manuka.siblings.is_empty());

        let review = queue_page(&snapshot, "review", 0, 20);
        assert_eq!(review.unextracted, 0);
        assert_eq!(review.needs_review, 1);
        assert_eq!(review.products.len(), 1);
        assert_eq!(review.products[0].booth_id, "8800104");
        assert!(
            review.products[0]
                .unresolved_names
                .iter()
                .any(|name| name == "架空ちゃん")
        );
    }

    #[test]
    fn many_compatible_names_come_from_the_description_list() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [{
                "boothId": "8550001",
                "title": "部屋着(3アバター対応)",
                "description": "対応アバター\n\"サンプル工房 ライム_ver1.00 https://booth.pm/ja/items/1001\" \"プラム_ver1.01 https://booth.pm/ja/items/1002\" \"あまとうさぎ ショコラ_ver1.02 https://booth.pm/ja/items/1003\"\n基本仕様\n布です。"
            }]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let product = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "8550001")
            .unwrap();
        assert_eq!(product.category, Some(Category::Outfit));
        assert!(
            !product
                .compatible_avatars
                .iter()
                .any(|name| name.contains("アバター対応"))
        );
        let mut ids: Vec<_> = snapshot
            .links
            .iter()
            .filter(|link| link.product_id == "8550001")
            .map(|link| link.avatar_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["chocolat", "lime", "plum"]);
        assert!(
            avatar_cards(&snapshot)
                .iter()
                .all(|card| !card.id.starts_with("owned-"))
        );
    }

    #[test]
    fn fresh_database_defaults_to_qwen9b_and_keeps_a_saved_model() {
        let connection = open_memory().unwrap();
        assert_eq!(settings(&connection).unwrap().ollama_model, "qwen3.5:9b");
        save_settings(
            &connection,
            &SettingsInput {
                ollama_base_url: "http://127.0.0.1:11434".into(),
                ollama_model: "qwen3:4b".into(),
                confidence_threshold: 0.6,
                library_folder: String::new(),
                builtin_browser: false,
                remote_library_folder: String::new(),
                remote_auto_sync: false,
                route_downloads: false,
                llm_backend: "ollama".into(),
                background_mode: false,
                launch_at_login: false,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
                ui_rail_width: 232,
                ui_genre_width: 200,
                check_update_on_startup: false,
                ui_update_dismissed_version: String::new(),
            },
        )
        .unwrap();
        ensure_ready(&connection).unwrap();
        assert_eq!(settings(&connection).unwrap().ollama_model, "qwen3:4b");
        assert!(!settings(&connection).unwrap().builtin_browser);
        assert!(!setup_status(&connection).unwrap().complete);
    }

    #[test]
    fn existing_library_or_pairing_skips_setup_until_reopened() {
        let connection = open_memory().unwrap();
        assert!(setting(&connection, "setup_complete").unwrap().is_none());
        connection
            .execute(
                "INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('4242', '架空の服', 't', 't')",
                [],
            )
            .unwrap();
        ensure_ready(&connection).unwrap();
        assert!(setup_status(&connection).unwrap().complete);
        set_setup_complete(&connection, false).unwrap();
        ensure_ready(&connection).unwrap();
        assert!(!setup_status(&connection).unwrap().complete);

        let paired = open_memory().unwrap();
        set_extension_origin(
            &paired,
            "chrome-extension://abcdefghijklmnopabcdefghijklmnop",
        )
        .unwrap();
        ensure_ready(&paired).unwrap();
        assert!(setup_status(&paired).unwrap().complete);
        let status = setup_status(&paired).unwrap();
        assert_eq!(status.default_model, "qwen3.5:9b");
        assert_eq!(status.model, "qwen3.5:9b");
    }

    #[test]
    fn builtin_browser_stays_off_until_the_user_enables_it() {
        let connection = open_memory().unwrap();
        assert!(!settings(&connection).unwrap().builtin_browser);
        assert_eq!(settings(&connection).unwrap().ollama_model, "qwen3.5:9b");
        save_settings(
            &connection,
            &SettingsInput {
                ollama_base_url: "http://127.0.0.1:11434".into(),
                ollama_model: "gemma4:12b".into(),
                confidence_threshold: 0.6,
                library_folder: String::new(),
                builtin_browser: true,
                remote_library_folder: String::new(),
                remote_auto_sync: false,
                route_downloads: false,
                llm_backend: "ollama".into(),
                background_mode: false,
                launch_at_login: false,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
                ui_rail_width: 232,
                ui_genre_width: 200,
                check_update_on_startup: false,
                ui_update_dismissed_version: String::new(),
            },
        )
        .unwrap();
        ensure_ready(&connection).unwrap();
        let saved = settings(&connection).unwrap();
        assert!(saved.builtin_browser);
        assert_eq!(saved.ollama_model, "gemma4:12b");
    }

    #[test]
    fn unavailable_items_are_not_left_unextracted() {
        let mut connection = open_memory().unwrap();
        let json = r#"{"products":[
            {"boothId":"6874323","title":"消えた商品","shop":"shop","description":"公開終了（取得できず）"},
            {"boothId":"1000","title":"普通の商品","shop":"shop","description":"説明"}
        ]}"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        connection
            .execute(
                "UPDATE products SET description_snippet = ?1, category = NULL, extracted_at = 't',
                 needs_review = 1, user_confirmed = 0 WHERE booth_id = '1000'",
                params![crate::model::UNAVAILABLE_DESCRIPTION],
            )
            .unwrap();
        ensure_ready(&connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        for id in ["6874323", "1000"] {
            let product = snapshot
                .products
                .iter()
                .find(|product| product.booth_id == id)
                .unwrap();
            assert_eq!(product.category, Some(Category::Other), "{id}");
            assert!(product.extracted_at.is_some(), "{id}");
            assert!(!product.needs_review, "{id}");
            assert!(product.user_confirmed, "{id}");
        }
        let page = queue_page(&snapshot, "unextracted", 0, 20);
        assert_eq!(page.unextracted, 0);
        assert_eq!(page.needs_review, 0);
    }

    #[test]
    fn unpublished_library_flag_survives_merge_and_is_not_unavailable() {
        let mut connection = open_memory().unwrap();
        let hidden = r#"{"boothId":"424242","title":"ダミー衣装","shop":"ダミー工房","description":"説明だけ","isFree":true,"unpublished":true}"#;
        import_products(&mut connection, &parse_export(hidden).unwrap()).unwrap();
        let (free, unpublished, category, confirmed): (Option<i64>, Option<i64>, Option<String>, i64) = connection
            .query_row(
                "SELECT is_free, unpublished, category, user_confirmed FROM products WHERE booth_id = '424242'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(free, Some(1));
        assert_eq!(unpublished, Some(1));
        assert_eq!(category, None);
        assert_eq!(confirmed, 0);
        let again = r#"{"boothId":"424242","title":"ダミー衣装","shop":"ダミー工房","description":"説明だけ"}"#;
        import_products(&mut connection, &parse_export(again).unwrap()).unwrap();
        let kept: Option<i64> = connection
            .query_row(
                "SELECT unpublished FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, Some(1));
        let gone = r#"{"boothId":"515151","title":"消えたダミー","description":"公開終了（取得できず）"}"#;
        import_products(&mut connection, &parse_export(gone).unwrap()).unwrap();
        let closed: (Option<String>, Option<i64>) = connection
            .query_row(
                "SELECT category, unpublished FROM products WHERE booth_id = '515151'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(closed.0.as_deref(), Some("other"));
        assert_eq!(closed.1, None);
        let snapshot = load_snapshot(&connection).unwrap();
        let shown = products_by_ids(&snapshot, &["424242".into(), "515151".into()]);
        assert_eq!(shown[0].booth_id, "424242");
        assert!(shown[0].unpublished);
        assert!(shown[0].is_free);
        assert_eq!(shown[0].category, None);
        assert!(!shown[1].unpublished);
        assert_eq!(shown[1].category, Some(Category::Other));
        let clear = r#"{"boothId":"424242","title":"ダミー衣装","unpublished":false}"#;
        import_products(&mut connection, &parse_export(clear).unwrap()).unwrap();
        let cleared: Option<i64> = connection
            .query_row(
                "SELECT unpublished FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cleared, Some(0));
        let after = load_snapshot(&connection).unwrap();
        let hidden_view = products_by_ids(&after, &["424242".into()]);
        assert!(!hidden_view[0].unpublished);
        assert!(hidden_view[0].is_free);
    }

    #[test]
    fn sale_brackets_and_alias_forms_follow_rules_not_a_weak_model() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"5007531","title":"【半額セール中】真冬 Mafuyu / オリジナル3Dモデル","shop":"s","description":"真冬のオリジナル3Dモデルです。"},
                {"boothId":"4023598","title":"【半額セール中】【オリジナル3Dモデル】Nayu - ナユ","shop":"s","description":"ナユのオリジナル3Dモデルです。"},
                {"boothId":"4876459","title":"ライム -Lime- / オリジナル3Dモデル","shop":"こまど","description":"ライムのオリジナル3Dモデルです。"},
                {"boothId":"1011","title":"【プラム専用】Hairグラデーション","shop":"s","description":"髪です。"},
                {"boothId":"1012","title":"ショコラ -Chocolat- 対応ワンピース","shop":"s","description":"衣装です。"},
                {"boothId":"1013","title":"rurune -ルルネ- 用ヘア","shop":"s","description":"髪です。"},
                {"boothId":"1014","title":"manuka-マヌカ対応部屋着","shop":"s","description":"服です。"},
                {"boothId":"1015","title":"Selestia対応アクセサリ","shop":"s","description":"小物です。"},
                {"boothId":"1016","title":"プラムちゃん専用ウィッグ","shop":"s","description":"髪です。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "5007531",
            &extraction(Category::Avatar, &["真冬 Mafuyu"], false, 0.4),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "4023598",
            &extraction(Category::Avatar, &["Nayu"], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "4876459",
            &extraction(Category::Outfit, &[], false, 0.2),
            0.6,
        )
        .unwrap();
        for (id, category) in [
            ("1011", Category::Outfit),
            ("1012", Category::Gimmick),
            ("1013", Category::Outfit),
            ("1014", Category::Gimmick),
            ("1015", Category::Outfit),
            ("1016", Category::Outfit),
        ] {
            apply_extraction(
                &mut connection,
                id,
                &extraction(category, &[], false, 0.2),
                0.6,
            )
            .unwrap();
        }

        let snapshot = load_snapshot(&mut connection).unwrap();
        assert!(
            snapshot
                .avatars
                .iter()
                .all(|avatar| avatar.display_name != "半額セール中")
        );
        let linked = |id: &str| -> Vec<String> {
            snapshot
                .links
                .iter()
                .filter(|link| link.product_id == id)
                .map(|link| link.avatar_id.clone())
                .collect()
        };
        let name_of = |id: &str| {
            snapshot
                .avatars
                .iter()
                .find(|avatar| avatar.id == id)
                .map(|avatar| avatar.display_name.clone())
                .unwrap_or_default()
        };
        let mafuyu = linked("5007531");
        let nayu = linked("4023598");
        assert_eq!(mafuyu.len(), 1);
        assert_eq!(nayu.len(), 1);
        assert_ne!(mafuyu[0], nayu[0]);
        assert_eq!(name_of(&mafuyu[0]), "真冬");
        assert_eq!(name_of(&nayu[0]), "ナユ");
        assert_eq!(
            snapshot
                .avatars
                .iter()
                .find(|avatar| avatar.id == nayu[0])
                .unwrap()
                .aliases,
            vec!["Nayu".to_string()]
        );

        let lime = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "4876459")
            .unwrap();
        assert_eq!(lime.category, Some(Category::Avatar));
        assert!(!lime.needs_review);
        assert_eq!(linked("4876459"), vec!["lime".to_string()]);

        let hair = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "1011")
            .unwrap();
        assert_eq!(hair.category, Some(Category::Outfit));
        assert_eq!(linked("1011"), vec!["plum".to_string()]);
        assert!(!hair.needs_review);

        assert_eq!(linked("1012"), vec!["chocolat".to_string()]);
        assert_eq!(linked("1013"), vec!["rurune".to_string()]);
        assert_eq!(linked("1014"), vec!["manuka".to_string()]);
        assert_eq!(linked("1015"), vec!["celestia".to_string()]);
        assert_eq!(linked("1016"), vec!["plum".to_string()]);
        for id in ["1012", "1013", "1014", "1015", "1016"] {
            let product = snapshot
                .products
                .iter()
                .find(|product| product.booth_id == id)
                .unwrap();
            assert!(
                !product.needs_review,
                "{id} category={:?} names={:?} unresolved={:?} links={:?}",
                product.category,
                product.compatible_avatars,
                product.unresolved_names,
                linked(id)
            );
        }
    }

    #[test]
    fn title_fragments_and_weak_keywords_do_not_override_the_model() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"7825066","title":"【プラム Plum】Eye Textures Pack (Full Pack)","shop":"s","description":"瞳のテクスチャです。"},
                {"boothId":"4511962","title":"むにゅりっぷ LipSync【プラム対応】","shop":"s","description":"プラムのオリジナル3Dモデル向けの口ギミックです。"},
                {"boothId":"2000","title":"エク対応ギミック","shop":"s","description":"エク用です。"},
                {"boothId":"7592511","title":"ミルキーチル","shop":"s","description":"Eku対応の衣装です。"},
                {"boothId":"2001","title":"エク - Eku","shop":"s","description":"エクのオリジナル3Dモデルです。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "7825066",
            &extraction(Category::Texture, &["プラム"], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "4511962",
            &extraction(Category::Gimmick, &["プラム"], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "2000",
            &extraction(Category::Gimmick, &["エク"], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "7592511",
            &extraction(Category::Outfit, &["Eku"], false, 0.9),
            0.6,
        )
        .unwrap();
        let before = load_snapshot(&mut connection).unwrap();
        let eye = before
            .products
            .iter()
            .find(|product| product.booth_id == "7825066")
            .unwrap();
        assert!(
            eye.unresolved_names.is_empty(),
            "{:?}",
            eye.unresolved_names
        );
        assert!(eye.compatible_avatars.iter().all(|name| name == "プラム"));
        assert!(!eye.needs_review);
        let lips = before
            .products
            .iter()
            .find(|product| product.booth_id == "4511962")
            .unwrap();
        assert_eq!(lips.category, Some(Category::Gimmick));
        assert!(
            before
                .links
                .iter()
                .any(|link| link.product_id == "4511962" && link.avatar_id == "plum")
        );
        let catalog_eku = before
            .avatars
            .iter()
            .find(|avatar| avatar.display_name == "エク")
            .unwrap();
        assert!(
            before
                .links
                .iter()
                .any(|link| link.product_id == "2000" && link.avatar_id == catalog_eku.id)
        );

        apply_extraction(
            &mut connection,
            "2001",
            &extraction(Category::Avatar, &["エク"], false, 0.9),
            0.6,
        )
        .unwrap();
        reresolve_all(&mut connection).unwrap();
        let after = load_snapshot(&mut connection).unwrap();
        let eku_id = after
            .links
            .iter()
            .find(|link| link.product_id == "2001")
            .unwrap()
            .avatar_id
            .clone();
        assert!(
            after
                .links
                .iter()
                .any(|link| link.product_id == "2000" && link.avatar_id == eku_id)
        );
        assert!(
            after
                .links
                .iter()
                .any(|link| link.product_id == "7592511" && link.avatar_id == eku_id)
        );
        let eku = after
            .avatars
            .iter()
            .find(|avatar| avatar.id == eku_id)
            .unwrap();
        assert_eq!(eku.display_name, "エク");
        assert_eq!(eku.aliases, vec!["Eku".to_string()]);
        assert!(
            crate::resolve::resolve_name("Eku", &after.avatars)
                .unwrap()
                .id
                == eku_id
        );
        assert!(crate::resolve::resolve_name("PC版", &after.avatars).is_none());
    }

    #[test]
    fn kipfel_title_links_after_the_avatar_product_exists() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"8835445","title":"【Kipfel】ねこネコパーカー【MA対応】","shop":"s","description":"パーカーです。"},
                {"boothId":"5813187","title":"キプフェル Kipfel / オリジナル3Dモデル","shop":"s","description":"キプフェルのオリジナル3Dモデルです。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "8835445",
            &extraction(Category::Outfit, &[], true, 0.9),
            0.6,
        )
        .unwrap();
        let early = load_snapshot(&mut connection).unwrap();
        let kipfel = early
            .avatars
            .iter()
            .find(|avatar| avatar.display_name == "キプフェル")
            .unwrap();
        assert!(
            early
                .links
                .iter()
                .any(|link| link.product_id == "8835445" && link.avatar_id == kipfel.id)
        );
        let avatar = early
            .products
            .iter()
            .find(|product| product.booth_id == "5813187")
            .unwrap();
        assert_eq!(avatar.category, Some(Category::Avatar));
        assert!(!avatar.needs_review);
        reresolve_all(&mut connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let hoodie = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "8835445")
            .unwrap();
        assert!(!hoodie.is_generic);
        assert!(!hoodie.needs_review);
        assert!(
            snapshot
                .links
                .iter()
                .any(|link| link.product_id == "8835445" && link.avatar_id == kipfel.id)
        );
    }

    #[test]
    fn texture_and_gimmick_without_avatars_are_generic() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"7001","title":"瞳テクスチャ","shop":"s","description":"目の模様です。"},
                {"boothId":"7002","title":"光るギミック","shop":"s","description":"小物です。"},
                {"boothId":"7003","title":"【プラム対応】瞳","shop":"s","description":"プラムの瞳です。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "7001",
            &extraction(Category::Texture, &[], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "7002",
            &extraction(Category::Gimmick, &[], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "7003",
            &extraction(Category::Texture, &["プラム"], false, 0.9),
            0.6,
        )
        .unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let texture = snapshot.products.iter().find(|product| product.booth_id == "7001").unwrap();
        let gimmick = snapshot.products.iter().find(|product| product.booth_id == "7002").unwrap();
        let named = snapshot.products.iter().find(|product| product.booth_id == "7003").unwrap();
        assert!(texture.is_generic);
        assert!(gimmick.is_generic);
        assert!(!named.is_generic);
    }

    #[test]
    fn accessories_without_avatar_names_are_generic() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"6409298","title":"Neon Halo","shop":"s","description":"光る輪です。"},
                {"boothId":"3318785","title":"ピアス【PB対応】","shop":"s","description":"耳飾りです。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "6409298",
            &extraction(Category::Accessory, &[], false, 1.0),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "3318785",
            &extraction(Category::Accessory, &["PB"], false, 1.0),
            0.6,
        )
        .unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        for id in ["6409298", "3318785"] {
            let product = snapshot
                .products
                .iter()
                .find(|product| product.booth_id == id)
                .unwrap();
            assert!(product.is_generic, "{id}");
            assert!(
                product.unresolved_names.is_empty(),
                "{id} {:?}",
                product.unresolved_names
            );
            assert!(!product.needs_review, "{id}");
            assert!(snapshot.links.iter().all(|link| link.product_id != id));
        }
    }

    #[test]
    fn a_seen_reading_pair_folds_a_later_romaji_name() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"5420885","title":"Dreamy Step","shop":"s","description":"衣装です。"},
                {"boothId":"6980080","title":"Calm Loop Hair","shop":"s","description":"髪です。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "5420885",
            &extraction(Category::Outfit, &["萌(Moe)"], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "6980080",
            &extraction(Category::Hair, &["Moe"], false, 0.9),
            0.6,
        )
        .unwrap();
        reresolve_all(&mut connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        for id in ["5420885", "6980080"] {
            let product = snapshot
                .products
                .iter()
                .find(|product| product.booth_id == id)
                .unwrap();
            assert!(
                product.unresolved_names.is_empty(),
                "{id} {:?}",
                product.unresolved_names
            );
            assert!(
                snapshot.links.iter().any(|link| {
                    link.product_id == id
                        && snapshot.avatars.iter().any(|avatar| {
                            avatar.id == link.avatar_id && avatar.display_name == "萌"
                        })
                }),
                "{id}"
            );
        }
    }

    #[test]
    fn a_low_confidence_placeholder_stays_in_review() {
        let mut connection = open_memory().unwrap();
        // No explicit original-model title: this case still depends on model extraction.
        let json = r#"{"products":[{"boothId":"3001","title":"架空ちゃん","shop":"s","description":"架空ちゃんです。"}]}"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "3001",
            &extraction(Category::Avatar, &[], false, 0.4),
            0.6,
        )
        .unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let product = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "3001")
            .unwrap();
        assert!(product.needs_review);
    }

    #[test]
    fn mamehinata_face_animation_is_grounded_on_the_established_placeholder() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"6436480","title":"まめひなたフェイスアニメーションセット","shop":"s","description":"表情のアニメーションです。"},
                {"boothId":"4340548","title":"まめひなた Mamehinata / オリジナル3Dモデル","shop":"s","description":"まめひなたのオリジナル3Dモデルです。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "6436480",
            &extraction(Category::Gimmick, &["まめひなた"], false, 1.0),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "4340548",
            &extraction(Category::Avatar, &["まめひなた"], false, 1.0),
            0.6,
        )
        .unwrap();
        reresolve_all(&mut connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let face = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "6436480")
            .unwrap();
        assert!(face.unresolved_names.is_empty());
        assert!(!face.needs_review);
        assert!(
            snapshot
                .links
                .iter()
                .any(|link| { link.product_id == "6436480" && link.avatar_id == "mf-4340548" })
        );
    }

    #[test]
    fn catalog_booth_id_registers_the_avatar_product() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"3016885","title":"透羽","shop":"mio","description":"アバターです。"},
                {"boothId":"8800991","title":"【架空丸対応】パーカー","shop":"s","description":"衣装です。"},
                {"boothId":"8800992","title":"【架空丸対応】ウィッグ","shop":"s","description":"髪です。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let body = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "3016885")
            .unwrap();
        assert_eq!(body.category, Some(Category::Avatar));
        assert!(body.extracted_at.is_some());
        assert!(!body.needs_review);
        assert!(snapshot.links.iter().any(|link| {
            link.product_id == "3016885"
                && snapshot.avatars.iter().any(|avatar| {
                    avatar.id == link.avatar_id && avatar.display_name == "透羽" && avatar.owned
                })
        }));
        let kikyo = snapshot
            .avatars
            .iter()
            .find(|avatar| avatar.display_name == "桔梗")
            .unwrap();
        assert!(!kikyo.owned);
        let candidates = avatar_candidates(&connection).unwrap();
        let fictional = candidates
            .iter()
            .find(|item| item.name == "架空丸")
            .unwrap();
        assert_eq!(fictional.product_count, 2);
        let id = register_candidate(&mut connection, "架空丸").unwrap();
        let after = load_snapshot(&mut connection).unwrap();
        assert!(
            after
                .links
                .iter()
                .any(|link| link.product_id == "8800991" && link.avatar_id == id)
        );
        assert!(
            avatar_candidates(&connection)
                .unwrap()
                .iter()
                .all(|item| item.name != "架空丸")
        );
        let known = known_booth_ids(&connection).unwrap();
        assert!(known.iter().any(|item| item == "3016885"));
        assert!(known.iter().any(|item| item == "8800991"));
    }

    #[test]
    fn a_named_original_avatar_outside_the_catalog_is_owned_on_import() {
        let mut connection = open_memory().unwrap();
        let json = r#"{"products":[
            {"boothId":"8800771","title":"九尾オリジナル3Dモデル「輝夜」-kaguya-","description":"対応アバター\nライム\n利用規約"},
            {"boothId":"8800772","title":"【輝夜対応】衣装","description":"衣装です。"}
        ]}"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        reresolve_all(&mut connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        let body = snapshot.products.iter().find(|p| p.booth_id == "8800771").unwrap();
        assert_eq!(body.category, Some(Category::Avatar));
        assert!(!body.needs_review);
        let kaguya = snapshot.avatars.iter().find(|a| a.display_name == "輝夜").unwrap();
        assert!(kaguya.owned);
        assert!(kaguya.aliases.iter().any(|alias| alias.eq_ignore_ascii_case("kaguya")));
        assert!(snapshot.links.iter().any(|link| link.product_id == "8800771" && link.avatar_id == kaguya.id));
        assert!(snapshot.links.iter().any(|link| link.product_id == "8800772" && link.avatar_id == kaguya.id));
        assert!(!snapshot.avatars.iter().find(|a| a.id == "lime").unwrap().owned);
        // Reimport and reopening must not duplicate or forget the owned body.
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        settle_dictionary(&mut connection).unwrap();
        let after = load_snapshot(&mut connection).unwrap();
        assert_eq!(after.avatars.iter().filter(|a| a.display_name == "輝夜").count(), 1);
        assert!(after.avatars.iter().any(|a| a.display_name == "輝夜" && a.owned));
    }

    #[test]
    fn quoted_body_identity_wins_over_known_names_outside_the_quotation() {
        for (index, title, expected_name, expected_id) in [
            (0, "オリジナル3Dモデル「輝夜」 ライム対応", "輝夜", "owned-8800780"),
            (1, "オリジナル3Dモデル「プラム」 ライム対応", "プラム", "plum"),
        ] {
            let mut connection = open_memory().unwrap();
            let booth_id = format!("880078{index}");
            let json = serde_json::json!({"boothId": booth_id, "title": title}).to_string();
            import_products(&mut connection, &parse_export(&json).unwrap()).unwrap();
            reresolve_all(&mut connection).unwrap();
            let snapshot = load_snapshot(&mut connection).unwrap();
            let owned = snapshot.avatars.iter().find(|a| a.id == expected_id).unwrap();
            assert_eq!(owned.display_name, expected_name);
            assert!(owned.owned);
            assert_eq!(link_ids(&connection, &booth_id).unwrap(), vec![expected_id.to_string()]);
            assert!(!snapshot.avatars.iter().find(|a| a.id == "lime").unwrap().owned);
        }
    }

    #[test]
    fn failed_body_name_inference_does_not_block_import_or_reopening() {
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-unresolved-body-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        let path = dir.0.join("library.sqlite");
        let parsed = parse_export(r#"{"boothId":"8800782","title":"オリジナル3Dモデル「ReviewOnlyBody」","description":"ReviewOnlyBody（朏）"}"#).unwrap();
        {
            let mut connection = open(&path).unwrap();
            import_products(&mut connection, &parsed).unwrap();
            let product = require_product(&connection, "8800782").unwrap();
            assert!(product.needs_review);
            assert!(!product.user_confirmed);
            assert!(product.extracted_at.is_none());
            assert!(link_ids(&connection, "8800782").unwrap().is_empty());
        }
        {
            let mut connection = open(&path).unwrap();
            settle_dictionary(&mut connection).unwrap();
            let product = require_product(&connection, "8800782").unwrap();
            assert!(product.needs_review);
            assert!(product.extracted_at.is_none());
            assert!(link_ids(&connection, "8800782").unwrap().is_empty());
        }
    }

    #[test]
    fn automatic_body_registration_propagates_database_errors_and_rolls_back() {
        let mut connection = open_memory().unwrap();
        let parsed = parse_export(r#"{"boothId":"8800783","title":"オリジナル3Dモデル「輝夜」"}"#).unwrap();
        upsert(&connection, &parsed.products[0]).unwrap();
        connection.execute_batch(
            "CREATE TRIGGER fail_body_link BEFORE INSERT ON product_avatars
             BEGIN SELECT RAISE(ABORT, 'fixture body link failure'); END;"
        ).unwrap();
        let error = settle_dictionary(&mut connection).unwrap_err();
        assert!(error.contains("fixture body link failure"));
        assert!(!avatar_exists(&connection, "owned-8800783").unwrap());
        let product = require_product(&connection, "8800783").unwrap();
        assert!(product.extracted_at.is_none());
        assert!(!product.user_confirmed);
    }

    #[test]
    fn named_original_bodies_settle_existing_imports_but_preserve_manual_decisions() {
        let mut connection = open_memory().unwrap();
        let parsed = parse_export(r#"{"boothId":"8800773","title":"九尾オリジナル3Dモデル「輝夜」-kaguya-"}"#).unwrap();
        upsert(&connection, &parsed.products[0]).unwrap();
        connection.execute("UPDATE products SET category = 'outfit', category_locked = 1 WHERE booth_id = '8800773'", []).unwrap();
        settle_dictionary(&mut connection).unwrap();
        assert_eq!(require_product(&connection, "8800773").unwrap().category, Some(Category::Outfit));
        assert!(link_ids(&connection, "8800773").unwrap().is_empty());
        connection.execute("UPDATE products SET category = NULL, category_locked = 0, links_locked = 1 WHERE booth_id = '8800773'", []).unwrap();
        settle_dictionary(&mut connection).unwrap();
        assert!(link_ids(&connection, "8800773").unwrap().is_empty());
        connection.execute("UPDATE products SET links_locked = 0 WHERE booth_id = '8800773'", []).unwrap();
        settle_dictionary(&mut connection).unwrap();
        let snapshot = load_snapshot(&mut connection).unwrap();
        assert!(snapshot.avatars.iter().any(|a| a.display_name == "輝夜" && a.owned));
    }

    fn insert_dummy_product(connection: &Connection, booth_id: &str, title: &str, description: &str) {
        let json = serde_json::json!({
            "boothId": booth_id,
            "title": title,
            "description": description,
        })
        .to_string();
        let parsed = parse_export(&json).unwrap();
        upsert(connection, &parsed.products[0]).unwrap();
    }

    fn force_stuck_classified(connection: &Connection, booth_id: &str, display: &str) {
        let avatar_id = format!("owned-{booth_id}");
        connection
            .execute(
                "UPDATE products SET category = 'avatar', category_locked = 0, links_locked = 0,
                 user_confirmed = 0, needs_review = 0, is_generic = 0, confidence = 0.9,
                 extracted_at = '2026-01-01T00:00:00Z', classifier_version = 2,
                 compatible_avatars_json = ?1, unresolved_names_json = '[]'
                 WHERE booth_id = ?2",
                params![to_json(&vec![display.to_string()]).unwrap(), booth_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT OR IGNORE INTO avatars
                 (id, display_name, base_body, aliases_json, is_builtin, owned, notes)
                 VALUES (?1, ?2, NULL, '[]', 0, 0, '')",
                params![avatar_id, display],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE avatars SET owned = 0, display_name = ?2 WHERE id = ?1",
                params![avatar_id, display],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM product_avatars WHERE product_id = ?1",
                params![booth_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, 'extracted')",
                params![booth_id, avatar_id],
            )
            .unwrap();
    }

    fn assert_owned_original(connection: &Connection, booth_id: &str, display: &str) {
        let avatar_id = format!("owned-{booth_id}");
        let snapshot = load_snapshot(connection).unwrap();
        let matched: Vec<_> = snapshot
            .avatars
            .iter()
            .filter(|avatar| avatar.display_name == display)
            .collect();
        assert_eq!(matched.len(), 1, "{display}");
        assert!(matched[0].owned, "{display}");
        assert_eq!(matched[0].id, avatar_id);
        assert!(
            snapshot.links.iter().any(|link| {
                link.product_id == booth_id && link.avatar_id == avatar_id && link.source == "owned"
            }),
            "{booth_id} {:?}",
            snapshot
                .links
                .iter()
                .filter(|link| link.product_id == booth_id)
                .collect::<Vec<_>>()
        );
    }

    fn assert_still_unowned(connection: &Connection, booth_id: &str, display: &str) {
        let avatar_id = format!("owned-{booth_id}");
        let owned: i64 = connection
            .query_row(
                "SELECT owned FROM avatars WHERE id = ?1",
                params![avatar_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owned, 0, "{display}");
        let source: String = connection
            .query_row(
                "SELECT source FROM product_avatars WHERE product_id = ?1 AND avatar_id = ?2",
                params![booth_id, avatar_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source, "extracted", "{display}");
        let copies: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM avatars WHERE display_name = ?1",
                params![display],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(copies, 1, "{display}");
    }

    #[test]
    fn model_classified_quoted_original_body_becomes_owned() {
        let mut connection = open_memory().unwrap();
        let booth_id = "7701001";
        let json = serde_json::json!({
            "boothId": booth_id,
            "title": "オリジナル3Dモデル「テスト子」-testko-",
            "description": "テスト子のダミー説明です。",
        })
        .to_string();
        import_products(&mut connection, &parse_export(&json).unwrap()).unwrap();
        // The model path used to leave this body classified but unowned.
        force_stuck_classified(&connection, booth_id, "テスト子");
        apply_extraction(
            &mut connection,
            booth_id,
            &extraction(Category::Avatar, &["テスト子"], false, 0.9),
            0.6,
        )
        .unwrap();
        assert_owned_original(&connection, booth_id, "テスト子");
        settle_dictionary(&mut connection).unwrap();
        assert_owned_original(&connection, booth_id, "テスト子");
    }

    #[test]
    fn skip_model_owns_a_classified_quoted_original_body_without_the_model() {
        let mut connection = open_memory().unwrap();
        let booth_id = "7701002";
        insert_dummy_product(
            &connection,
            booth_id,
            "オリジナル3Dモデル「テスト子」-testko-",
            "テスト子のダミー説明です。",
        );
        force_stuck_classified(&connection, booth_id, "テスト子");
        let settled = skip_model(&mut connection, booth_id).unwrap();
        assert!(settled);
        assert_owned_original(&connection, booth_id, "テスト子");
        let copies: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM avatars WHERE display_name = 'テスト子'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(copies, 1);
    }

    #[test]
    fn quoted_original_body_guards_stay_unowned() {
        let mut connection = open_memory().unwrap();
        let cases = [
            ("7701101", "オリジナル3Dモデル「テスト錠」-testjo-", "テスト錠", "locked"),
            ("7701102", "オリジナル3Dモデル「テスト鎖」-testsa-", "テスト鎖", "links"),
            ("7701103", "オリジナル3Dモデル「テスト認」-testnin-", "テスト認", "confirmed"),
            ("7701104", "オリジナル3Dモデル「テスト甲」「テスト乙」", "テスト甲", "quotes"),
            ("7701105", "オリジナル3Dモデル「テスト除」-testjo-", "テスト除", "excluded"),
            ("7701106", "オリジナル3Dモデル「テスト無」-testmu-", "テスト無", "unavailable"),
        ];
        for (booth_id, title, display, _kind) in cases {
            insert_dummy_product(&connection, booth_id, title, "ダミー説明です。");
            force_stuck_classified(&connection, booth_id, display);
        }
        connection
            .execute(
                "UPDATE products SET category = 'outfit', category_locked = 1 WHERE booth_id = '7701101'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE products SET links_locked = 1 WHERE booth_id = '7701102'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE products SET user_confirmed = 1 WHERE booth_id = '7701103'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE products SET description_snippet = ?1 WHERE booth_id = '7701106'",
                params![crate::model::UNAVAILABLE_DESCRIPTION],
            )
            .unwrap();
        set_product_excluded(&mut connection, "7701105", true).unwrap();

        settle_dictionary(&mut connection).unwrap();
        for (booth_id, _title, display, _kind) in cases {
            let product = require_product(&connection, booth_id).unwrap();
            assert!(
                !settle_named_original_body(&mut connection, &product).unwrap(),
                "{display}"
            );
            let _ = skip_model(&mut connection, booth_id).unwrap();
            assert_still_unowned(&connection, booth_id, display);
        }
    }

    #[test]
    fn reimport_does_not_duplicate_a_quoted_original_body() {
        let mut connection = open_memory().unwrap();
        let booth_id = "7701003";
        let json = serde_json::json!({
            "boothId": booth_id,
            "title": "オリジナル3Dモデル「テスト子」-testko-",
            "description": "テスト子のダミー説明です。",
        })
        .to_string();
        let parsed = parse_export(&json).unwrap();
        import_products(&mut connection, &parsed).unwrap();
        apply_extraction(
            &mut connection,
            booth_id,
            &extraction(Category::Avatar, &["テスト子"], false, 0.9),
            0.6,
        )
        .unwrap();
        import_products(&mut connection, &parsed).unwrap();
        settle_dictionary(&mut connection).unwrap();
        assert_owned_original(&connection, booth_id, "テスト子");

        let stuck = "7701004";
        insert_dummy_product(
            &connection,
            stuck,
            "オリジナル3Dモデル「テスト再」-testre-",
            "テスト再のダミー説明です。",
        );
        force_stuck_classified(&connection, stuck, "テスト再");
        let again = serde_json::json!({
            "boothId": stuck,
            "title": "オリジナル3Dモデル「テスト再」-testre-",
            "description": "テスト再のダミー説明です。",
        })
        .to_string();
        import_products(&mut connection, &parse_export(&again).unwrap()).unwrap();
        assert_owned_original(&connection, stuck, "テスト再");
        assert_owned_original(&connection, booth_id, "テスト子");
    }

    #[test]
    fn startup_repairs_a_classified_quoted_original_body() {
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-owned-body-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        std::fs::create_dir_all(&dir.0).unwrap();
        let path = dir.0.join("library.sqlite");
        {
            let connection = open(&path).unwrap();
            insert_dummy_product(
                &connection,
                "7701005",
                "オリジナル3Dモデル「テスト子」-testko-",
                "テスト子のダミー説明です。",
            );
            force_stuck_classified(&connection, "7701005", "テスト子");
            let owned: i64 = connection
                .query_row(
                    "SELECT owned FROM avatars WHERE id = 'owned-7701005'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(owned, 0);
        }
        {
            let connection = open(&path).unwrap();
            assert_owned_original(&connection, "7701005", "テスト子");
        }
        {
            let connection = open(&path).unwrap();
            assert_owned_original(&connection, "7701005", "テスト子");
        }
    }

    #[test]
    fn an_unlisted_avatar_body_stays_unowned_until_marked() {
        let mut connection = open_memory().unwrap();
        let json = r#"{
            "products": [
                {"boothId":"3001","title":"架空丸","shop":"s","description":"架空丸です。"},
                {"boothId":"3002","title":"【桔梗対応】パーカー","shop":"s","description":"衣装です。"}
            ]
        }"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        let before = load_snapshot(&mut connection).unwrap();
        assert!(
            before
                .avatars
                .iter()
                .any(|avatar| avatar.display_name == "桔梗" && !avatar.owned)
        );
        assert!(
            before
                .products
                .iter()
                .any(|product| product.booth_id == "3001" && product.extracted_at.is_none())
        );
        let id = mark_owned_body(&mut connection, "3001").unwrap();
        let after = load_snapshot(&mut connection).unwrap();
        let owned = after.avatars.iter().find(|avatar| avatar.id == id).unwrap();
        assert!(owned.owned);
        assert_eq!(owned.display_name, "架空丸");
        let body = after
            .products
            .iter()
            .find(|product| product.booth_id == "3001")
            .unwrap();
        assert_eq!(body.category, Some(Category::Avatar));
        assert!(!body.needs_review);
        assert!(
            after
                .avatars
                .iter()
                .any(|avatar| avatar.display_name == "桔梗" && !avatar.owned)
        );
        let cards = avatar_cards(&after);
        assert!(cards.iter().any(|card| card.id == id && card.owned));
        assert!(
            cards
                .iter()
                .any(|card| card.display_name == "桔梗" && !card.owned)
        );
    }

    #[test]
    fn purchased_variations_stay_owned_and_partial_pages_keep_files() {
        let mut connection = open_memory().unwrap();
        let first = r#"{"boothId":"424242","title":"架空の服","isFree":true,"variations":[{"name":"通常","owned":true},{"name":"特典","owned":false}],"downloads":[{"filename":"body.unitypackage","variation":"通常","sizeBytes":12}]}"#;
        import_products(&mut connection, &parse_export(first).unwrap()).unwrap();
        let second = r#"{"boothId":"424242","title":"架空の服","variations":[{"name":"通常","owned":false}],"downloads":[{"filename":"extra.zip","sizeBytes":3}]}"#;
        let report = import_products(&mut connection, &parse_export(second).unwrap()).unwrap();
        assert_eq!(report.updated, 1);
        let owned: i64 = connection
            .query_row(
                "SELECT owned FROM product_variations WHERE booth_id = '424242' AND name = '通常'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owned, 1);
        let bonus: i64 = connection
            .query_row(
                "SELECT owned FROM product_variations WHERE booth_id = '424242' AND name = '特典'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(bonus, 0);
        let files: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(files, 2);
        connection
            .execute(
                "UPDATE product_files SET downloaded = 1, local_path = 'C:\\Assets\\body.unitypackage' WHERE filename = 'body.unitypackage'",
                [],
            )
            .unwrap();
        let again = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"body.unitypackage","variation":"通常","sizeBytes":12}]}"#;
        import_products(&mut connection, &parse_export(again).unwrap()).unwrap();
        let downloaded: i64 = connection
            .query_row(
                "SELECT downloaded FROM product_files WHERE filename = 'body.unitypackage'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(downloaded, 1);
        let free: Option<i64> = connection
            .query_row(
                "SELECT is_free FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(free, Some(1));
        let explicit = r#"{"boothId":"424242","title":"架空の服","isFree":false}"#;
        import_products(&mut connection, &parse_export(explicit).unwrap()).unwrap();
        let free: Option<i64> = connection
            .query_row(
                "SELECT is_free FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(free, Some(0));
    }

    #[test]
    fn an_import_without_files_warns_and_a_downloadable_id_keeps_its_row() {
        let mut connection = open_memory().unwrap();
        let empty = r#"{"boothId":"424242","title":"架空の服"}"#;
        let report = import_products(&mut connection, &parse_export(empty).unwrap()).unwrap();
        assert_eq!(report.variations, 0);
        assert_eq!(report.downloads, 0);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("0 件"))
        );
        let placeholder = r#"{"boothId":"424242","title":"架空の服","variations":[{"name":"通常","owned":true}],"downloads":[{"filename":"2336025","downloadableId":"2336025","variation":"通常"}]}"#;
        import_products(&mut connection, &parse_export(placeholder).unwrap()).unwrap();
        let named = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"body.unitypackage","downloadableId":"2336025","variation":"通常","sizeBytes":12}]}"#;
        let report = import_products(&mut connection, &parse_export(named).unwrap()).unwrap();
        assert_eq!(report.downloads, 1);
        assert_eq!(report.variations, 0);
        assert!(
            report
                .warnings
                .iter()
                .all(|warning| !warning.contains("0 件"))
        );
        let filename: String = connection
            .query_row(
                "SELECT filename FROM product_files WHERE booth_id = '424242' AND downloadable_id = '2336025'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(filename, "body.unitypackage");
        assert_eq!(report.unnamed_downloads, 0);
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn a_legacy_id_filename_is_replaced_on_the_same_row() {
        let mut connection = open_memory().unwrap();
        let legacy = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"2336025","variation":"通常"}]}"#;
        let first = import_products(&mut connection, &parse_export(legacy).unwrap()).unwrap();
        assert_eq!(first.downloads, 1);
        assert_eq!(first.unnamed_downloads, 1);
        let named = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"body.unitypackage","downloadableId":"2336025","variation":"通常"}]}"#;
        let second = import_products(&mut connection, &parse_export(named).unwrap()).unwrap();
        assert_eq!(second.unnamed_downloads, 0);
        let (filename, id): (String, String) = connection
            .query_row(
                "SELECT filename, downloadable_id FROM product_files WHERE booth_id = '424242'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(filename, "body.unitypackage");
        assert_eq!(id, "2336025");
        let again = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"2336025","downloadableId":"2336025","variation":"通常"}]}"#;
        import_products(&mut connection, &parse_export(again).unwrap()).unwrap();
        let kept: String = connection
            .query_row(
                "SELECT filename FROM product_files WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, "body.unitypackage");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn a_folder_scan_creates_local_file_rows_from_product_ids() {
        let mut connection = open_memory().unwrap();
        let json = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"known.unitypackage"}]}"#;
        import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
        let outcome = crate::scan::ScanOutcome {
            truncated: false,
            entries: vec![
                crate::scan::ScanEntry {
                    path: "/library/424242/known.unitypackage".into(),
                    filename: "known.unitypackage".into(),
                    is_dir: false,
                    size: 12,
                    labels: vec!["known".into(), "424242".into()],
                },
                crate::scan::ScanEntry {
                    path: "/library/clothes/pack-424242/bonus.zip".into(),
                    filename: "bonus.zip".into(),
                    is_dir: false,
                    size: 4,
                    labels: vec!["bonus".into()],
                },
                crate::scan::ScanEntry {
                    path: "/library/other/note.txt".into(),
                    filename: "note.txt".into(),
                    is_dir: false,
                    size: 1,
                    labels: vec!["note".into()],
                },
            ],
        };
        apply_scan(&mut connection, "/library", &outcome).unwrap();
        let known: (i64, String) = connection
            .query_row(
                "SELECT downloaded, local_path FROM product_files
                 WHERE booth_id = '424242' AND filename = 'known.unitypackage'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(known.0, 1);
        assert_eq!(known.1, "/library/424242/known.unitypackage");
        let bonus: (i64, String) = connection
            .query_row(
                "SELECT downloaded, local_path FROM product_files
                 WHERE booth_id = '424242' AND filename = 'bonus.zip'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(bonus.0, 1);
        assert!(bonus.1.contains("bonus.zip"));
        let note: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE filename = 'note.txt'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(note, 0);
    }

    #[test]
    fn free_url_and_routed_download_do_not_fetch_booth() {
        let mut connection = open_memory().unwrap();
        let id =
            add_free_product(&mut connection, "https://booth.pm/ja/items/424242?ref=1").unwrap();
        assert_eq!(id, "424242");
        assert!(add_free_product(&mut connection, "https://example.test/nope").is_err());
        let free: i64 = connection
            .query_row(
                "SELECT is_free FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(free, 1);
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        add_free_product(&mut connection, "https://booth.pm/ja/items/424242").unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let root = std::env::temp_dir().join(format!("vrclibman-accept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let staging = root.join("Downloads").join("VRCLibMan").join("515151");
        let library = root.join("library");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&library).unwrap();
        let source = staging.join("free.unitypackage");
        std::fs::write(&source, b"free").unwrap();
        let notice = crate::model::DownloadFinished {
            booth_id: "515151".into(),
            path: source.display().to_string(),
            filename: "free.unitypackage".into(),
            size: Some(3),
            is_free: Some(true),
            is_adult: None,
            variation: "通常".into(),
            title: "無料の小物".into(),
        };
        assert!(
            prepare_download(&mut connection, &notice)
                .unwrap_err()
                .contains("オフ")
        );
        assert!(source.exists());
        let refused = save_settings(
            &connection,
            &SettingsInput {
                ollama_base_url: "http://127.0.0.1:11434".into(),
                ollama_model: "gemma4:12b".into(),
                confidence_threshold: 0.6,
                library_folder: String::new(),
                builtin_browser: false,
                remote_library_folder: String::new(),
                remote_auto_sync: false,
                route_downloads: true,
                llm_backend: "ollama".into(),
                background_mode: false,
                launch_at_login: false,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
                ui_rail_width: 232,
                ui_genre_width: 200,
                check_update_on_startup: false,
                ui_update_dismissed_version: String::new(),
            },
        );
        assert!(refused.unwrap_err().contains("ライブラリ"));
        save_settings(
            &connection,
            &SettingsInput {
                ollama_base_url: "http://127.0.0.1:11434".into(),
                ollama_model: "gemma4:12b".into(),
                confidence_threshold: 0.6,
                library_folder: library.display().to_string(),
                builtin_browser: false,
                remote_library_folder: String::new(),
                remote_auto_sync: false,
                route_downloads: true,
                llm_backend: "ollama".into(),
                background_mode: false,
                launch_at_login: false,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
                ui_rail_width: 232,
                ui_genre_width: 200,
                check_update_on_startup: false,
                ui_update_dismissed_version: String::new(),
            },
        )
        .unwrap();
        struct ClearDownloads;
        impl Drop for ClearDownloads {
            fn drop(&mut self) {
                crate::library_files::set_test_downloads(None);
            }
        }
        let _clear = ClearDownloads;
        crate::library_files::set_test_downloads(Some(root.join("Downloads")));
        let outside = root.join("secret.unitypackage");
        std::fs::write(&outside, b"no").unwrap();
        let bad = crate::model::DownloadFinished {
            path: outside.display().to_string(),
            filename: "secret.unitypackage".into(),
            ..notice.clone()
        };
        assert!(
            prepare_download(&mut connection, &bad)
                .unwrap_err()
                .contains("VRCLibMan")
        );
        assert!(outside.exists());
        let wrong = crate::model::DownloadFinished {
            booth_id: "900001".into(),
            ..notice.clone()
        };
        assert!(prepare_download(&mut connection, &wrong).is_err());
        assert!(source.exists());
        let plan = prepare_download(&mut connection, &notice).unwrap();
        let placed = crate::library_files::place_into_library(
            &plan.source,
            plan.library.as_deref(),
            &plan.booth_id,
        )
        .unwrap();
        record_placed_download(
            &connection,
            &plan.booth_id,
            &plan.variation,
            &placed,
            notice.size,
        )
        .unwrap();
        assert!(!source.exists());
        assert_eq!(placed, library.join("515151").join("free.unitypackage"));
        let created_free: i64 = connection
            .query_row(
                "SELECT is_free FROM products WHERE booth_id = '515151'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(created_free, 1);
        let title: String = connection
            .query_row(
                "SELECT title FROM products WHERE booth_id = '515151'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "無料の小物");
        let downloaded: i64 = connection
            .query_row(
                "SELECT downloaded FROM product_files WHERE booth_id = '515151'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(downloaded, 1);
        let stored_size: i64 = connection
            .query_row(
                "SELECT size_bytes FROM product_files WHERE booth_id = '515151'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_size, 4);
        std::fs::write(&placed, b"free!").unwrap();
        remember_file_size(&connection, "515151", "通常", "free.unitypackage", 5).unwrap();
        let stored_size: i64 = connection
            .query_row(
                "SELECT size_bytes FROM product_files WHERE booth_id = '515151'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_size, 5);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn browser_settings(
        connection: &Connection,
        library: &std::path::Path,
        browser: bool,
        route: bool,
    ) -> Result<(), String> {
        save_settings(
            connection,
            &SettingsInput {
                ollama_base_url: "http://127.0.0.1:11434".into(),
                ollama_model: "gemma4:12b".into(),
                confidence_threshold: 0.6,
                library_folder: library.display().to_string(),
                builtin_browser: browser,
                remote_library_folder: String::new(),
                remote_auto_sync: false,
                route_downloads: route,
                llm_backend: "ollama".into(),
                background_mode: false,
                launch_at_login: false,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
                ui_rail_width: 232,
                ui_genre_width: 200,
                check_update_on_startup: false,
                ui_update_dismissed_version: String::new(),
            },
        )
    }

    #[test]
    fn browser_download_needs_the_opt_in_and_routing_before_any_file_moves() {
        let mut connection = open_memory().unwrap();
        let root = crate::library_files::resolved_temp_dir().join(format!("vrclibman-browser-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let library = root.join("library");
        let downloads = root.join("Downloads");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::create_dir_all(&downloads).unwrap();
        let payload = r#"{"boothId":"424242","title":"架空の服","downloads":[{"filename":"2336025","variation":"通常","downloadableId":"2336025"},{"filename":"note.txt","variation":"","downloadableId":""}]}"#;
        import_products(&mut connection, &parse_export(payload).unwrap()).unwrap();
        assert!(plan_browser_download(&connection, "424242", "通常", "2336025")
            .unwrap_err()
            .contains("オフです。拡張"));
        browser_settings(&connection, &library, true, false).unwrap();
        assert!(plan_browser_download(&connection, "424242", "通常", "2336025")
            .unwrap_err()
            .contains("自動整理はオフ"));
        browser_settings(&connection, &library, true, true).unwrap();
        assert!(plan_browser_download(&connection, "424242", "", "note.txt")
            .unwrap_err()
            .contains("ダウンロード番号"));
        let plan = plan_browser_download(&connection, "424242", "通常", "2336025").unwrap();
        assert_eq!(plan.downloadable_id, "2336025");
        let legacy = r#"{"boothId":"515151","title":"架空の服","downloads":[{"filename":"8187247","variation":""}]}"#;
        import_products(&mut connection, &parse_export(legacy).unwrap()).unwrap();
        let legacy = plan_browser_download(&connection, "515151", "", "8187247").unwrap();
        assert_eq!(legacy.downloadable_id, "8187247");

        struct ClearDownloads;
        impl Drop for ClearDownloads {
            fn drop(&mut self) {
                crate::library_files::set_test_downloads(None);
            }
        }
        let _clear = ClearDownloads;
        crate::library_files::set_test_downloads(Some(downloads.clone()));
        let staging = crate::library_files::prepare_staging_dir(&downloads, "424242").unwrap();
        let html = staging.join("login.html");
        std::fs::write(&html, b"<!doctype html><html><head><title>login</title></head></html>").unwrap();
        let html_plan = BrowserFilePlan {
            filename: "2336025".into(),
            ..plan.clone()
        };
        let refused = commit_browser_download(&mut connection, &html_plan, &html).unwrap_err();
        assert!(refused.contains("ログイン"));
        assert!(!html.exists());
        let still: String = connection
            .query_row(
                "SELECT filename FROM product_files WHERE booth_id = '424242' AND variation = '通常'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(still, "2336025");

        let packed = staging.join("body.unitypackage");
        std::fs::write(&packed, b"PK\x03\x04dummy").unwrap();
        let placed = commit_browser_download(&mut connection, &plan, &packed).unwrap();
        assert_eq!(placed, library.join("424242").join("body.unitypackage"));
        assert!(!packed.exists());
        let (filename, downloaded, id): (String, i64, String) = connection
            .query_row(
                "SELECT filename, downloaded, downloadable_id FROM product_files
                 WHERE booth_id = '424242' AND variation = '通常'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(filename, "body.unitypackage");
        assert_eq!(downloaded, 1);
        assert_eq!(id, "2336025");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_files WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        adopt_download_filename(
            &connection,
            "424242",
            "通常",
            "body.unitypackage",
            "2336025",
            "2336025",
        )
        .unwrap();
        let kept: String = connection
            .query_row(
                "SELECT filename FROM product_files WHERE booth_id = '424242' AND variation = '通常'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, "body.unitypackage");
        let title: String = connection
            .query_row(
                "SELECT title FROM products WHERE booth_id = '424242'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!title.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_the_database_keeps_generic_rows_until_extract() {
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let dir = Temp(std::env::temp_dir().join(format!(
            "vrclibman-world-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )));
        std::fs::create_dir_all(&dir.0).unwrap();
        let db_path = dir.0.join("library.sqlite");
        let json = r#"{
            "products": [
                {"boothId":"9101","title":"見本の小物","shop":"見本","description":"どのアバターでも使える小物です"},
                {"boothId":"9102","title":"見本のワールド小物","shop":"見本","description":"ワールドに置く小物です"},
                {"boothId":"9103","title":"表情ギミック","shop":"見本","description":"アバターに付けるギミックです"},
                {"boothId":"9104","title":"ミホン用ギミック","shop":"見本","description":"ミホンに付けるギミックです"}
            ]
        }"#;
        {
            let mut connection = open(&db_path).unwrap();
            import_products(&mut connection, &parse_export(json).unwrap()).unwrap();
            apply_extraction(
                &mut connection,
                "9101",
                &extraction(Category::Gimmick, &[], true, 0.9),
                0.6,
            )
            .unwrap();
            connection
                .execute(
                    "UPDATE products SET category = 'gimmick', is_generic = 1, extracted_at = 't',
                     needs_review = 0 WHERE booth_id = '9102'",
                    [],
                )
                .unwrap();
            let _ = connection.execute_batch("PRAGMA wal_checkpoint(FULL);");
        }
        {
            let connection = open(&db_path).unwrap();
            let snapshot = load_snapshot(&connection).unwrap();
            for id in ["9101", "9102"] {
                let product = snapshot
                    .products
                    .iter()
                    .find(|product| product.booth_id == id)
                    .unwrap();
                assert_eq!(product.category, Some(Category::Gimmick), "{id}");
                assert!(product.is_generic, "{id}");
            }
            for id in ["9103", "9104"] {
                let untouched = snapshot
                    .products
                    .iter()
                    .find(|product| product.booth_id == id)
                    .unwrap();
                assert!(untouched.category.is_none(), "{id}");
                assert!(!untouched.is_generic, "{id}");
            }
        }
        let mut connection = open(&db_path).unwrap();
        apply_extraction(
            &mut connection,
            "9102",
            &extraction(Category::Gimmick, &[], true, 0.84),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "9101",
            &extraction(Category::Gimmick, &[], true, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "9103",
            &extraction(Category::Gimmick, &[], false, 0.9),
            0.6,
        )
        .unwrap();
        apply_extraction(
            &mut connection,
            "9104",
            &extraction(Category::Gimmick, &["ミホン"], false, 0.9),
            0.6,
        )
        .unwrap();
        let snapshot = load_snapshot(&connection).unwrap();
        let world = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "9102")
            .unwrap();
        assert_eq!(world.category, Some(Category::World));
        assert!(!world.is_generic);
        let generic = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "9101")
            .unwrap();
        assert_eq!(generic.category, Some(Category::Gimmick));
        assert!(generic.is_generic);
        let gimmick = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "9103")
            .unwrap();
        // 対応アバターが無いギミックは、モデルが汎用でないと答えても汎用になる。
        assert_eq!(gimmick.category, Some(Category::Gimmick));
        assert!(gimmick.is_generic);
        let targeted = snapshot
            .products
            .iter()
            .find(|product| product.booth_id == "9104")
            .unwrap();
        assert_eq!(targeted.category, Some(Category::Gimmick));
        assert!(!targeted.is_generic);
    }

    #[test]
    fn launch_at_login_is_stored_only_with_background_mode() {
        let connection = open_memory().unwrap();
        let saved = settings(&connection).unwrap();
        assert!(!saved.background_mode);
        assert!(!saved.launch_at_login);
        assert_eq!(saved.ollama_model, "qwen3.5:9b");
        let base = SettingsInput {
            ollama_base_url: "http://127.0.0.1:11434".into(),
            ollama_model: "gemma4:12b".into(),
            confidence_threshold: 0.6,
            library_folder: String::new(),
            builtin_browser: false,
            remote_library_folder: String::new(),
            remote_auto_sync: false,
            route_downloads: false,
            llm_backend: "ollama".into(),
            background_mode: false,
            launch_at_login: true,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
            ui_rail_width: 232,
            ui_genre_width: 200,
            check_update_on_startup: false,
            ui_update_dismissed_version: String::new(),
        };
        save_settings(&connection, &base).unwrap();
        let saved = settings(&connection).unwrap();
        assert!(!saved.background_mode);
        assert!(!saved.launch_at_login);
        assert!(!crate::presence::autostart_should_enable(
            saved.background_mode,
            saved.launch_at_login
        ));
        save_settings(
            &connection,
            &SettingsInput {
                background_mode: true,
                launch_at_login: true,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
                ..base
            },
        )
        .unwrap();
        let saved = settings(&connection).unwrap();
        assert!(saved.background_mode);
        assert!(saved.launch_at_login);
        assert!(crate::presence::autostart_should_enable(
            saved.background_mode,
            saved.launch_at_login
        ));
        assert_eq!(saved.ollama_model, "gemma4:12b");
    }

    #[test]
    fn opening_the_database_and_the_sweep_do_not_require_booth() {
        let mut connection = open_memory().unwrap();
        assert_eq!(settings(&connection).unwrap().ollama_model, "qwen3.5:9b");
        let root = std::env::temp_dir().join(format!(
            "vrclibman-sweep-empty-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Downloads")).unwrap();
        struct ClearDownloads;
        impl Drop for ClearDownloads {
            fn drop(&mut self) {
                crate::library_files::set_test_downloads(None);
            }
        }
        let _clear = ClearDownloads;
        crate::library_files::set_test_downloads(Some(root.join("Downloads")));
        let report = sweep_staged_downloads(&mut connection);
        assert_eq!(report.moved, 0);
        assert!(report.left.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn startup_sweep_leaves_files_when_routing_is_off() {
        let mut connection = open_memory().unwrap();
        let root = std::env::temp_dir().join(format!("vrclibman-sweep-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root
            .join("Downloads")
            .join("VRCLibMan")
            .join("626262")
            .join("body.unitypackage");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"pkg").unwrap();
        struct ClearDownloads;
        impl Drop for ClearDownloads {
            fn drop(&mut self) {
                crate::library_files::set_test_downloads(None);
            }
        }
        let _clear = ClearDownloads;
        crate::library_files::set_test_downloads(Some(root.join("Downloads")));
        let report = sweep_staged_downloads(&mut connection);
        assert_eq!(report.moved, 0);
        assert!(source.exists());
        assert_eq!(std::fs::read(&source).unwrap(), b"pkg");
        assert!(report.left.iter().any(|item| {
            item.booth_id == "626262"
                && item.filename == "body.unitypackage"
                && item.reason.contains("オフ")
        }));
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM products WHERE booth_id = '626262'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn startup_sweep_moves_only_the_staged_file_without_network() {
        let mut connection = open_memory().unwrap();
        let root = crate::library_files::resolved_temp_dir().join(format!("vrclibman-sweep-on-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let downloads = root.join("Downloads");
        let library = root.join("library");
        std::fs::create_dir_all(&library).unwrap();
        let source = downloads
            .join("VRCLibMan")
            .join("616161")
            .join("body.unitypackage");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"pkg").unwrap();
        let nested = downloads
            .join("VRCLibMan")
            .join("616161")
            .join("extra")
            .join("nested.unitypackage");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, b"nested").unwrap();
        let invalid = downloads
            .join("VRCLibMan")
            .join("nope")
            .join("skip.unitypackage");
        std::fs::create_dir_all(invalid.parent().unwrap()).unwrap();
        std::fs::write(&invalid, b"skip").unwrap();
        let outside = root.join("outside.unitypackage");
        std::fs::write(&outside, b"secret").unwrap();
        let foreign = root
            .join("other")
            .join("VRCLibMan")
            .join("616161")
            .join("foreign.unitypackage");
        std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
        std::fs::write(&foreign, b"foreign").unwrap();
        let pocket = root.join("pocket").join("717171").join("via.unitypackage");
        std::fs::create_dir_all(pocket.parent().unwrap()).unwrap();
        std::fs::write(&pocket, b"via").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, source.parent().unwrap().join("link.unitypackage"))
                .unwrap();
            std::os::unix::fs::symlink(pocket.parent().unwrap(), downloads.join("VRCLibMan").join("717171"))
                .unwrap();
            let linked_root = root.join("linked-downloads");
            std::fs::create_dir_all(&linked_root).unwrap();
            std::os::unix::fs::symlink(root.join("pocket"), linked_root.join("VRCLibMan")).unwrap();
            assert!(crate::library_files::staged_download_candidates(&linked_root).is_empty());
            assert!(pocket.exists());
        }
        save_settings(
            &connection,
            &SettingsInput {
                ollama_base_url: "http://127.0.0.1:11434".into(),
                ollama_model: "gemma4:12b".into(),
                confidence_threshold: 0.6,
                library_folder: library.display().to_string(),
                builtin_browser: false,
                remote_library_folder: String::new(),
                remote_auto_sync: false,
                route_downloads: true,
                llm_backend: "ollama".into(),
                background_mode: false,
                launch_at_login: false,
                unload_ollama_after_classification: true,
                classification_dismissed_version: 0,
                ui_rail_width: 232,
                ui_genre_width: 200,
                check_update_on_startup: false,
                ui_update_dismissed_version: String::new(),
            },
        )
        .unwrap();
        struct ClearDownloads;
        impl Drop for ClearDownloads {
            fn drop(&mut self) {
                crate::library_files::set_test_downloads(None);
            }
        }
        let _clear = ClearDownloads;
        crate::library_files::set_test_downloads(Some(downloads));
        assert!(route_staged_download(&mut connection, &outside).is_err());
        assert!(outside.exists());
        assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
        assert!(route_staged_download(&mut connection, &foreign).is_err());
        assert!(foreign.exists());
        let report = sweep_staged_downloads(&mut connection);
        assert_eq!(report.moved, 1);
        assert!(!source.exists());
        let placed = library.join("616161").join("body.unitypackage");
        assert_eq!(std::fs::read(&placed).unwrap(), b"pkg");
        assert!(nested.exists());
        assert!(invalid.exists());
        assert!(outside.exists());
        assert!(foreign.exists());
        assert!(pocket.exists());
        assert_eq!(std::fs::read(&pocket).unwrap(), b"via");
        #[cfg(unix)]
        {
            let link = root
                .join("Downloads")
                .join("VRCLibMan")
                .join("616161")
                .join("link.unitypackage");
            assert!(link.exists());
            assert!(report.left.iter().any(|item| {
                item.filename == "link.unitypackage" && item.reason.contains("シンボリックリンク")
            }));
            assert_eq!(std::fs::read(&outside).unwrap(), b"secret");
        }
        let (title, description, url): (String, String, String) = connection
            .query_row(
                "SELECT title, description_snippet, url FROM products WHERE booth_id = '616161'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(title, "商品 616161");
        assert!(description.is_empty());
        assert_eq!(url, "https://booth.pm/ja/items/616161");
        let variation: String = connection
            .query_row(
                "SELECT variation FROM product_files WHERE booth_id = '616161' AND filename = 'body.unitypackage'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(variation, "");
        let foreign_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM products WHERE booth_id = '717171'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(foreign_count, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stale_classification_counts_only_old_versions() {
        let mut connection = open_memory().unwrap();
        import_products(&mut connection, &parse_export(SAMPLE).unwrap()).unwrap();
        apply_extraction(
            &mut connection,
            "1004",
            &extraction(Category::Outfit, &["ライム"], false, 0.9),
            0.6,
        )
        .unwrap();
        let before = classification_status(&connection).unwrap().stale_count;
        connection
            .execute(
                "UPDATE products SET classifier_version = 1 WHERE booth_id = '1004'",
                [],
            )
            .unwrap();
        let after = classification_status(&connection).unwrap().stale_count;
        assert!(after > before);
        assert!(
            products_for_extract(&connection, ExtractTarget::Stale)
                .unwrap()
                .iter()
                .any(|job| job.booth_id == "1004")
        );
    }

    #[test]
    fn booth_interval_is_safe_by_default_and_only_accepts_known_values() {
        let connection = open_memory().unwrap();
        assert_eq!(booth_page_interval_ms(&connection).unwrap(), 1000);
        for interval in [1000, 500, 250] {
            set_booth_page_interval_ms(&connection, interval).unwrap();
            assert_eq!(booth_page_interval_ms(&connection).unwrap(), interval);
        }
        assert!(set_booth_page_interval_ms(&connection, 50).is_err());
        assert_eq!(booth_page_interval_ms(&connection).unwrap(), 250);
        set_setting(&connection, "booth_page_interval_ms", "invalid").unwrap();
        assert_eq!(booth_page_interval_ms(&connection).unwrap(), 1000);
    }

    #[test]
    fn pinned_move_keeps_kind_and_confirmation_across_extension_reimport() {
        let mut connection = open_memory().unwrap();
        let id = add_free_product(&mut connection, "https://booth.pm/ja/items/424242").unwrap();
        assert_eq!(id, "424242");
        let stub = require_product(&connection, "424242").unwrap();
        assert!(stub.category.is_none());
        assert!(stub.description_snippet.is_empty());
        assert!(!stub.category_locked && !stub.genre_locked);

        apply_extraction(
            &mut connection,
            "424242",
            &extraction(Category::Outfit, &[], false, 0.9),
            0.6,
        )
        .unwrap();
        let classified = require_product(&connection, "424242").unwrap();
        let extracted_at = classified.extracted_at.clone().unwrap();
        assert_eq!(classified.category, Some(Category::Outfit));
        assert!(!classified.user_confirmed);

        let previous = move_product_genre(&mut connection, "424242", "world", "furniture_props").unwrap();
        assert_eq!(previous.category.as_deref(), Some("outfit"));
        assert!(!previous.category_locked && !previous.genre_locked);
        let moved = require_product(&connection, "424242").unwrap();
        assert_eq!(moved.category, Some(Category::World));
        assert_eq!(moved.genre.as_deref(), Some("furniture_props"));
        assert!(moved.category_locked && moved.genre_locked);
        assert_eq!(moved.extracted_at.as_deref(), Some(extracted_at.as_str()));
        assert!(product_ids_by_genre(&connection, "world", Some("furniture_props"))
            .unwrap()
            .contains(&"424242".into()));
        assert!(!product_ids_by_genre(&connection, "asset", None)
            .unwrap()
            .contains(&"424242".into()));

        update_product(
            &mut connection,
            &ProductUpdate {
                booth_id: "424242".into(),
                category: None,
                genre: None,
                is_generic: None,
                avatar_ids: None,
                confirm: Some(true),
            },
        )
        .unwrap();
        connection
            .execute(
                "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES ('424242', 'lime', 'extracted')",
                [],
            )
            .unwrap();
        let confirmed = require_product(&connection, "424242").unwrap();
        assert!(confirmed.user_confirmed);
        assert!(!confirmed.needs_review);
        assert!(confirmed.category_locked && confirmed.genre_locked);
        assert!(!confirmed.links_locked);

        let resent = parse_export(
            r#"[{"boothId":"424242","title":"見本の小物","shop":"見本","description":"対応アバター\nライム_ver1.00 https://booth.pm/ja/items/1001\n基本仕様\n布です。"}]"#,
        )
        .unwrap();
        import_products(&mut connection, &resent).unwrap();
        // The same payload again is an unchanged product, which still runs compatibility.
        import_products(&mut connection, &resent).unwrap();

        let kept = require_product(&connection, "424242").unwrap();
        assert_eq!(kept.category, Some(Category::World));
        assert_eq!(kept.genre.as_deref(), Some("furniture_props"));
        assert!(kept.category_locked && kept.genre_locked);
        assert!(kept.user_confirmed);
        assert!(!kept.needs_review);
        assert_eq!(kept.extracted_at.as_deref(), Some(extracted_at.as_str()));
        assert!(kept.description_snippet.contains("ライム"));
        assert_eq!(link_ids(&connection, "424242").unwrap(), vec!["lime".to_string()]);
        let queued = queue_page(&load_snapshot(&connection).unwrap(), "review", 0, 50);
        assert!(!queued.products.iter().any(|product| product.booth_id == "424242"));
        let unextracted = queue_page(&load_snapshot(&connection).unwrap(), "unextracted", 0, 50);
        assert!(!unextracted.products.iter().any(|product| product.booth_id == "424242"));

        apply_extraction(
            &mut connection,
            "424242",
            &extraction(Category::Hair, &["ライム"], false, 0.99),
            0.6,
        )
        .unwrap();
        let still = require_product(&connection, "424242").unwrap();
        assert_eq!(still.category, Some(Category::World));
        assert_eq!(still.genre.as_deref(), Some("furniture_props"));
        assert!(still.user_confirmed);

        for target in [ExtractTarget::Unclassified, ExtractTarget::Stale, ExtractTarget::All] {
            assert!(
                !products_for_extract(&connection, target)
                    .unwrap()
                    .iter()
                    .any(|job| job.booth_id == "424242"),
                "{target:?}"
            );
        }
        connection
            .execute(
                "UPDATE products SET classifier_version = 1 WHERE booth_id = '424242'",
                [],
            )
            .unwrap();
        let pinned_stale = classification_status(&connection).unwrap().stale_count;
        assert!(
            !products_for_extract(&connection, ExtractTarget::Stale)
                .unwrap()
                .iter()
                .any(|job| job.booth_id == "424242")
        );

        let pinned = unpin_product_classification(&connection, "424242").unwrap();
        assert!(pinned.category_locked && pinned.genre_locked);
        let open = require_product(&connection, "424242").unwrap();
        assert_eq!(open.category, Some(Category::World));
        assert_eq!(open.genre.as_deref(), Some("furniture_props"));
        assert!(!open.category_locked && !open.genre_locked);
        assert_eq!(
            classification_status(&connection).unwrap().stale_count,
            pinned_stale + 1
        );
        assert!(products_for_extract(&connection, ExtractTarget::Stale)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "424242"));
        assert!(products_for_extract(&connection, ExtractTarget::All)
            .unwrap()
            .iter()
            .any(|job| job.booth_id == "424242"));

        restore_product_classification(&connection, &previous).unwrap();
        let restored = require_product(&connection, "424242").unwrap();
        assert_eq!(restored.category, Some(Category::Outfit));
        assert_eq!(restored.genre, previous.genre);
        assert!(!restored.category_locked && !restored.genre_locked);
        assert!(!restored.user_confirmed);

        assert!(move_product_genre(&mut connection, "424242", "asset", "skybox").is_err());
        assert!(move_product_genre(&mut connection, "424242", "world", "hair").is_err());
        assert!(move_product_genre(&mut connection, "424242", "avatar", "hair").is_err());
        move_product_genre(&mut connection, "424242", "asset", "shoes_socks").unwrap();
        let shoes = require_product(&connection, "424242").unwrap();
        assert_eq!(shoes.category, Some(Category::Outfit));
        assert_eq!(shoes.genre.as_deref(), Some("shoes_socks"));
        assert!(shoes.category_locked && shoes.genre_locked);
        move_product_genre(&mut connection, "424242", "asset", "hair").unwrap();
        let hair = require_product(&connection, "424242").unwrap();
        assert_eq!(hair.category, Some(Category::Hair));
        assert_eq!(hair.genre.as_deref(), Some("hair"));
    }

    #[test]
    fn manual_avatar_attach_locks_the_link_without_pinning_classification() {
        let mut connection = open_memory().unwrap();
        let id = add_free_product(&mut connection, "https://booth.pm/ja/items/515151").unwrap();
        apply_extraction(
            &mut connection,
            &id,
            &extraction(Category::Gimmick, &[], true, 0.9),
            0.6,
        )
        .unwrap();
        connection
            .execute(
                "INSERT INTO product_avatars (product_id, avatar_id, source) VALUES ('515151', 'plum', 'extracted')",
                [],
            )
            .unwrap();
        let before = require_product(&connection, "515151").unwrap();
        assert_eq!(before.category, Some(Category::Gimmick));
        assert!(!before.category_locked && !before.genre_locked && !before.links_locked);
        assert!(!before.user_confirmed);

        attach_product_to_avatar(&mut connection, "lime", "515151").unwrap();
        attach_product_to_avatar(&mut connection, "lime", "515151").unwrap();
        let attached = require_product(&connection, "515151").unwrap();
        assert_eq!(attached.category, Some(Category::Gimmick));
        assert_eq!(attached.genre, before.genre);
        assert!(!attached.category_locked && !attached.genre_locked);
        assert!(attached.links_locked);
        assert!(!attached.user_confirmed);
        assert_eq!(
            link_rows(&connection, "515151"),
            vec![
                ("lime".to_string(), "manual".to_string()),
                ("plum".to_string(), "extracted".to_string()),
            ]
        );

        reresolve_all(&mut connection).unwrap();
        assert_eq!(
            link_rows(&connection, "515151"),
            vec![
                ("lime".to_string(), "manual".to_string()),
                ("plum".to_string(), "extracted".to_string()),
            ]
        );

        let resent = parse_export(
            r#"[{"boothId":"515151","title":"見本のギミック","shop":"見本","description":"対応アバター\nライム_ver1.00 https://booth.pm/ja/items/1001\n基本仕様\n布です。"}]"#,
        )
        .unwrap();
        import_products(&mut connection, &resent).unwrap();
        let reimported = require_product(&connection, "515151").unwrap();
        assert!(reimported.category.is_none());
        assert!(!reimported.category_locked && !reimported.genre_locked);
        assert!(reimported.links_locked);
        assert!(!reimported.user_confirmed);
        assert!(reimported.needs_review);
        assert_eq!(
            link_rows(&connection, "515151"),
            vec![
                ("lime".to_string(), "manual".to_string()),
                ("plum".to_string(), "extracted".to_string()),
            ]
        );
        let detail = avatar_detail(&load_snapshot(&connection).unwrap(), "lime").unwrap();
        assert!(section_items(&detail, "uncategorized").iter().any(|(booth_id, _)| booth_id == "515151"));

        apply_extraction(
            &mut connection,
            "515151",
            &extraction(Category::Hair, &["ライム"], false, 0.99),
            0.6,
        )
        .unwrap();
        let reclassified = require_product(&connection, "515151").unwrap();
        assert_eq!(reclassified.category, Some(Category::Hair));
        assert!(!reclassified.category_locked && !reclassified.genre_locked);
        assert!(reclassified.links_locked);
        assert!(link_rows(&connection, "515151").iter().any(|(avatar, source)| avatar == "lime" && source == "manual"));

        let world = add_free_product(&mut connection, "https://booth.pm/ja/items/525252").unwrap();
        apply_extraction(&mut connection, &world, &extraction(Category::World, &[], false, 0.9), 0.6).unwrap();
        assert!(attach_product_to_avatar(&mut connection, "lime", "525252").unwrap_err().contains("ワールド"));
        assert!(link_rows(&connection, "525252").is_empty());

        let body = add_free_product(&mut connection, "https://booth.pm/ja/items/535353").unwrap();
        apply_extraction(&mut connection, &body, &extraction(Category::Avatar, &["ライム"], false, 0.9), 0.6).unwrap();
        assert!(attach_product_to_avatar(&mut connection, "plum", "535353").unwrap_err().contains("アバター本体"));
        assert!(!link_rows(&connection, "535353").iter().any(|(avatar, _)| avatar == "plum"));

        let hidden = add_free_product(&mut connection, "https://booth.pm/ja/items/545454").unwrap();
        set_product_excluded(&mut connection, &hidden, true).unwrap();
        assert!(attach_product_to_avatar(&mut connection, "lime", "545454").unwrap_err().contains("除外"));
        assert!(attach_product_to_avatar(&mut connection, "missing", "515151").is_err());
        assert!(attach_product_to_avatar(&mut connection, "lime", "999999").is_err());
    }

    fn link_rows(connection: &Connection, booth_id: &str) -> Vec<(String, String)> {
        let mut statement = connection
            .prepare("SELECT avatar_id, source FROM product_avatars WHERE product_id = ?1 ORDER BY avatar_id")
            .unwrap();
        statement
            .query_map(params![booth_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn section_items(detail: &crate::view::AvatarDetail, category: &str) -> Vec<(String, bool)> {
        detail
            .sections
            .iter()
            .filter(|section| section.category == category)
            .flat_map(|section| {
                section
                    .items
                    .iter()
                    .map(|item| (item.booth_id.clone(), item.via_base_body))
            })
            .collect()
    }
}
