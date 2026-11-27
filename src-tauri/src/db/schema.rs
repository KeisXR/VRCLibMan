//! Opening a library: the first schema, the migrations, and the defaults set at startup.
use super::*;

/// The `user_version` of a library after every migration below has run. Raise it together with
/// a new `if version < N` block in `migrate`; tests compare against this.
pub(crate) const SCHEMA_VERSION: i64 = 16;

pub(super) const SCHEMA: &str = "
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
    notes TEXT NOT NULL DEFAULT '',
    hero_image_path TEXT
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

pub(super) trait Sql<T> {
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
pub(crate) fn open_memory() -> Result<Connection, String> {
    finish_open(Connection::open_in_memory().sql()?, false)
}

pub(super) fn finish_open(mut connection: Connection, wal: bool) -> Result<Connection, String> {
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
        .sql()?;
    if wal {
        let _ = connection.execute_batch("PRAGMA journal_mode = WAL;");
    }
    migrate(&connection)?;
    // One transaction. A new library gets a few thousand seed and catalog rows, and a commit per
    // row syncs the file every time. Immediate, so a second connection waits instead of failing.
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .sql()?;
    let seeded = ensure_ready(&tx)?;
    let added = merge_catalog(&tx)?;
    // New seed and catalog rows are not changes. This must run before the startup settling below.
    crate::sync_meta::baseline_missing(&tx)?;
    tx.commit().sql()?;
    let mut run = ClassificationRun::new(KIND_STARTUP, CLASSIFIER_CATALOG);
    observe_all(&mut connection, &mut run, |connection| {
        settle_dictionary(connection)?;
        if added > 0 || seeded > 0 {
            reresolve_products(connection)?;
        }
        Ok(())
    })?;
    Ok(connection)
}

pub(super) fn migrate(connection: &Connection) -> Result<(), String> {
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
        )
        .sql()?;
        tx.commit().sql()?;
    }
    if version < 3 {
        let tx = connection.unchecked_transaction().sql()?;
        ensure_column(
            &tx,
            "products",
            "genre",
            "ALTER TABLE products ADD COLUMN genre TEXT",
        )?;
        ensure_column(
            &tx,
            "products",
            "genre_locked",
            "ALTER TABLE products ADD COLUMN genre_locked INTEGER NOT NULL DEFAULT 0",
        )?;
        let products = load_products(&tx)?;
        for product in products {
            let category = product.category.unwrap_or(Category::Other);
            let genre =
                crate::genre::classify_text(category, &product.title, &product.description_snippet)
                    .unwrap_or_else(|| crate::genre::validate(category, None));
            tx.execute(
                "UPDATE products SET genre = ?1 WHERE booth_id = ?2",
                params![genre, product.booth_id],
            )
            .sql()?;
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
        connection.execute_batch("PRAGMA user_version = 4;").sql()?;
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
        connection.execute_batch("PRAGMA user_version = 5;").sql()?;
    }
    // v6 is only `classification_moves` (manual genre moves, unpin, and undo).
    // Older libraries have no history. Current pins are the lock columns.
    if version < 6 {
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS classification_moves (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    booth_id TEXT NOT NULL,
                    at TEXT NOT NULL,
                    action TEXT NOT NULL CHECK (action IN ('move', 'unpin', 'restore')),
                    from_category TEXT,
                    from_genre TEXT,
                    to_category TEXT,
                    to_genre TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_classification_moves_booth ON classification_moves(booth_id);",
            )
            .sql()?;
        connection.execute_batch("PRAGMA user_version = 6;").sql()?;
    }
    // v7 stores the public BOOTH category and tags used before the model.
    // booth_detail_at is set once a public document, adult flag, age gate, or 404 was seen.
    if version < 7 {
        ensure_column(
            connection,
            "products",
            "booth_category_id",
            "ALTER TABLE products ADD COLUMN booth_category_id INTEGER",
        )?;
        ensure_column(
            connection,
            "products",
            "booth_category_name",
            "ALTER TABLE products ADD COLUMN booth_category_name TEXT",
        )?;
        ensure_column(
            connection,
            "products",
            "booth_category_parent",
            "ALTER TABLE products ADD COLUMN booth_category_parent TEXT",
        )?;
        ensure_column(
            connection,
            "products",
            "booth_tags",
            "ALTER TABLE products ADD COLUMN booth_tags TEXT",
        )?;
        ensure_column(
            connection,
            "products",
            "booth_detail_at",
            "ALTER TABLE products ADD COLUMN booth_detail_at TEXT",
        )?;
        connection.execute_batch("PRAGMA user_version = 7;").sql()?;
    }
    // v8 records who last changed each syncable group. Existing values get a baseline with no
    // device or time, so none of them looks newer than a real change on another device.
    if version < 8 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(crate::sync_meta::SYNC_META_TABLE).sql()?;
        crate::sync_meta::baseline_missing(&tx)?;
        tx.execute_batch("PRAGMA user_version = 8;").sql()?;
        tx.commit().sql()?;
    }
    // v9 is only `avatars.hero_image_path` (a copy in `hero_images`). NULL means the thumbnail.
    if version < 9 {
        ensure_column(
            connection,
            "avatars",
            "hero_image_path",
            "ALTER TABLE avatars ADD COLUMN hero_image_path TEXT",
        )?;
        connection.execute_batch("PRAGMA user_version = 9;").sql()?;
    }
    // v10 is only the classification log. It is local display history, outside the sync export.
    // Older libraries have no history, and product rows stay as they are.
    if version < 10 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(classification_log::TABLES).sql()?;
        tx.execute_batch("PRAGMA user_version = 10;").sql()?;
        tx.commit().sql()?;
    }
    // v11 is only `remote_pushes` (files automatic remote sync copied or found). It is local
    // state for one remote folder, outside the sync export. Older libraries start empty.
    if version < 11 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(crate::remote_sync::PUSHES_TABLE).sql()?;
        tx.execute_batch("PRAGMA user_version = 11;").sql()?;
        tx.commit().sql()?;
    }
    // v12 is only the purchase history (orders, their items, and labels). Older libraries start
    // empty; nothing is filled in until the user reads the BOOTH order history.
    if version < 12 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(crate::purchases::TABLES).sql()?;
        tx.execute_batch("PRAGMA user_version = 12;").sql()?;
        tx.commit().sql()?;
    }
    if version < 13 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(crate::purchased_editions::TABLES).sql()?;
        tx.execute_batch("PRAGMA user_version = 13;").sql()?;
        tx.commit().sql()?;
    }
    // v14 marks products read from the library gifts section and keeps their current BOOTH prices
    // (local cache, outside the sync export). Older libraries learn both on the next full read.
    if version < 14 {
        let tx = connection.unchecked_transaction().sql()?;
        ensure_column(
            &tx,
            "products",
            "received_gift",
            "ALTER TABLE products ADD COLUMN received_gift INTEGER",
        )?;
        tx.execute_batch(crate::received_gifts::TABLES).sql()?;
        tx.execute_batch("PRAGMA user_version = 14;").sql()?;
        tx.commit().sql()?;
    }
    // v15 moves the asset update watch out of one settings value into rows per product
    // (`asset_watch`, `asset_watch_history`). Local state, outside the sync export.
    if version < 15 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(crate::asset_updates::TABLES).sql()?;
        crate::asset_updates::move_legacy_watch(&tx)?;
        tx.execute_batch("PRAGMA user_version = 15;").sql()?;
        tx.commit().sql()?;
    }
    // v16 is only the Unity projects the user added, hid, or linked to an avatar. Local paths,
    // outside the sync export. The ALCOM list itself is read each time and never copied here.
    if version < 16 {
        let tx = connection.unchecked_transaction().sql()?;
        tx.execute_batch(super::unity_projects::UNITY_PROJECT_TABLES)
            .sql()?;
        tx.execute_batch("PRAGMA user_version = 16;").sql()?;
        tx.commit().sql()?;
    }
    debug_assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .ok(),
        Some(SCHEMA_VERSION),
        "SCHEMA_VERSION must name the last migration"
    );
    Ok(())
}

pub(super) fn ensure_product_flag_columns(connection: &Connection) -> Result<(), String> {
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

pub(super) fn ensure_column(
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

pub(super) fn ensure_owned_column(connection: &Connection) -> Result<(), String> {
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

/// Returns how many avatars a seed upgrade inserted, so the caller can re-resolve.
pub(super) fn ensure_ready(connection: &Connection) -> Result<u32, String> {
    crate::sync_meta::ensure_device_identity(connection)?;
    for (key, value) in [
        ("confidence_threshold", "0.6"),
        ("library_folder", ""),
        ("builtin_browser", "0"),
        ("browser_force_dark", "0"),
        ("mcp_enabled", "0"),
        ("mcp_include_adult", "0"),
        ("browser_auto_register", "1"),
        ("booth_link_target", "external"),
        ("booth_page_interval_ms", "1000"),
        ("remote_library_folder", ""),
        ("remote_auto_sync", "0"),
        ("route_downloads", "0"),
        ("background_mode", "0"),
        ("launch_at_login", "0"),
        ("classification_dismissed_version", "0"),
        ("ui_rail_width", "232"),
        ("ui_rail_collapsed", "0"),
        ("ui_genre_width", "200"),
        ("ui_genre_collapsed", "0"),
        ("preview_pane", "0"),
        ("preview_pane_width", "300"),
        ("avatar_hero", crate::model::DEFAULT_AVATAR_HERO),
        ("right_panel_tabs", crate::model::DEFAULT_RIGHT_PANEL_TABS),
        ("check_update_on_startup", "0"),
        ("ui_update_dismissed_version", ""),
        (crate::purchases::SHOW_PRICES_KEY, "0"),
    ] {
        connection
            .execute(
                "INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
            .sql()?;
    }
    let mut seeded = 0;
    if setting(connection, "initialized")?.as_deref() != Some("1") {
        merge_seed(connection, 0)?;
        set_setting(connection, "initialized", "1")?;
    } else {
        // Existing libraries get base-body groups added by a newer seed once.
        // Older groups are not merged again, so rows the user deleted stay gone.
        let stored = setting(connection, "seed_revision")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(1);
        if stored < seed_file()?.revision {
            seeded = merge_seed(connection, stored)?.inserted;
        }
    }
    set_setting(
        connection,
        "seed_revision",
        &seed_file()?.revision.to_string(),
    )?;
    close_unavailable_products(connection)?;
    skip_setup_for_existing_use(connection)?;
    Ok(seeded)
}

pub(super) fn skip_setup_for_existing_use(connection: &Connection) -> Result<(), String> {
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
