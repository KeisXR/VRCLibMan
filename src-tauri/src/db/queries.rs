//! Read-only lists and counts for the screens, thumbnails, and exclusions.
use super::*;

pub(crate) struct ThumbJob {
    pub booth_id: String,
    pub thumbnail_url: String,
    pub thumbnail_path: Option<String>,
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

pub(crate) fn product_ids_by_genre(
    connection: &Connection,
    kind: &str,
    genre: Option<&str>,
) -> Result<Vec<String>, String> {
    let condition = match kind {
        "world" => "category = 'world'",
        "asset" => "category <> 'world' AND category <> 'avatar'",
        _ => return Err("ジャンルの種類が不正です".into()),
    };
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
    let sql = format!(
        "SELECT booth_id FROM products WHERE {condition} AND {genre_filter} AND booth_id NOT IN (SELECT booth_id FROM product_exclusions) ORDER BY title COLLATE NOCASE, booth_id"
    );
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
    let editions = crate::purchased_editions::Catalog::load(connection)?;
    Ok(Snapshot {
        purchased_editions: editions.scopes(),
        manual_editions: editions.manual,
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
        file_watches: crate::asset_updates::file_watches(connection).unwrap_or_default(),
        remote_configured: false,
        remote_checked: true,
        remote_reachable: false,
    })
}

/// The library counts (`LibraryStats`) without reading the whole library. The app shell, the
/// avatar list and the import page ask for them on every refresh; the snapshot took tens to
/// hundreds of ms. Same numbers as `view::library_stats` on a snapshot (a test compares them).
pub(crate) fn library_stats(connection: &Connection) -> Result<LibraryStats, String> {
    // A category the app does not know reads as none (`Category::parse`), so it counts as unclassified.
    let known = [
        Category::Avatar,
        Category::Outfit,
        Category::Hair,
        Category::Accessory,
        Category::Texture,
        Category::Gimmick,
        Category::World,
        Category::Other,
    ]
    .iter()
    .map(|category| format!("'{}'", category.as_str()))
    .collect::<Vec<_>>()
    .join(", ");
    let sql = format!(
        "SELECT
            (SELECT COUNT(*) FROM products
              WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)),
            (SELECT COUNT(*) FROM avatars a
              WHERE a.owned != 0 AND EXISTS (
                SELECT 1 FROM product_avatars l JOIN products p ON p.booth_id = l.product_id
                 WHERE l.avatar_id = a.id AND p.category = 'avatar'
                   AND p.booth_id NOT IN (SELECT booth_id FROM product_exclusions))),
            (SELECT COUNT(*) FROM products
              WHERE (category IS NULL OR category NOT IN ({known}))
                AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)),
            (SELECT COUNT(*) FROM local_files
              WHERE linked_product_id IS NULL
                AND (candidate_product_id IS NULL
                     OR candidate_product_id NOT IN (SELECT booth_id FROM product_exclusions))),
            (SELECT COUNT(*) FROM local_files
              WHERE is_dir = 0 AND linked_product_id IS NOT NULL
                AND linked_product_id NOT IN (SELECT booth_id FROM product_exclusions))
            + (SELECT COUNT(*) FROM product_files WHERE local_path IS NOT NULL AND local_path != '')"
    );
    connection
        .query_row(&sql, [], |row| {
            Ok(LibraryStats {
                products: row.get(0)?,
                owned_avatars: row.get(1)?,
                unextracted: row.get(2)?,
                unlinked_files: row.get(3)?,
                registered_files: row.get(4)?,
            })
        })
        .sql()
}

/// Products to pick from (adding an item to an avatar, linking a file, the update targets),
/// without reading the whole library. Same rows and order as `view::product_options` on a
/// snapshot: the snapshot's product order, then by title.
pub(crate) fn product_options(
    connection: &Connection,
) -> Result<Vec<crate::view::ProductOption>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id, title, shop, category FROM products
             WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)",
        )
        .sql()?;
    let mut options = statement
        .query_map([], |row| {
            let category: Option<String> = row.get(3)?;
            Ok(crate::view::ProductOption {
                booth_id: row.get(0)?,
                title: row.get(1)?,
                shop: row.get(2)?,
                category: category.as_deref().and_then(Category::parse),
            })
        })
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    options.sort_by(|left, right| left.title.cmp(&right.title));
    Ok(options)
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

pub(super) fn excluded_product_ids(connection: &Connection) -> Result<HashSet<String>, String> {
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
