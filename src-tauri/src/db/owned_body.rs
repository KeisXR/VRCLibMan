//! Deciding which products are owned avatar bodies and naming them.
use super::*;

pub(crate) fn settle_dictionary(connection: &mut Connection) -> Result<u32, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id FROM products
             WHERE links_locked = 0
               AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
               AND (
                 (
                   user_confirmed = 0 AND (
                     extracted_at IS NULL
                     OR booth_id IN (SELECT booth_id FROM avatar_sources)
                     OR (category = 'avatar' AND extracted_at IS NOT NULL)
                   )
                 )
                 OR booth_id IN (
                   SELECT product_id FROM product_avatars
                   WHERE source IN ('owned', 'catalog')
                 )
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

pub(super) fn settle_dictionary_includes(
    connection: &Connection,
    booth_id: &str,
) -> Result<bool, String> {
    let product = require_product(connection, booth_id)?;
    if product.extracted_at.is_none() || catalog_body(connection, booth_id)?.is_some() {
        return Ok(true);
    }
    if written_body_link(connection, booth_id)? {
        return Ok(true);
    }
    repairable_extracted_original_body(connection, &product)
}

pub(super) fn single_quoted_original_title(title: &str) -> bool {
    crate::compat::strong_category_override(title) == Some(Category::Avatar)
        && crate::normalize::quoted_segments(title).len() == 1
}

pub(super) fn repairable_extracted_original_body(
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

pub(super) fn unowned_link_matches_quote(
    connection: &Connection,
    product: &Product,
) -> Result<bool, String> {
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

pub(crate) fn skip_settled(connection: &mut Connection, booth_id: &str) -> Result<bool, String> {
    if product_excluded(connection, booth_id)? {
        return Ok(true);
    }
    let product = require_product(connection, booth_id)?;
    // Catalog and owned-body rows are already classified. Refresh their
    // classifier version and do not classify them again.
    if protect_written_body(connection, &product)? {
        return Ok(true);
    }
    if product.user_confirmed || product.links_locked {
        return Ok(false);
    }
    settle_one(connection, booth_id, true)
}

pub(super) fn settle_one(
    connection: &mut Connection,
    booth_id: &str,
    redo: bool,
) -> Result<bool, String> {
    if product_excluded(connection, booth_id)? {
        return Ok(false);
    }
    let product = require_product(connection, booth_id)?;
    if classification_pinned(&product) || product.user_confirmed || product.links_locked {
        let _ = protect_written_body(connection, &product)?;
        return Ok(false);
    }
    if let Some((avatar_id, display)) = catalog_body(connection, booth_id)? {
        if product.category_locked && product.category != Some(Category::Avatar) {
            return Ok(false);
        }
        write_catalog_body(connection, booth_id, &avatar_id, &display)?;
        return Ok(true);
    }
    // redo (skip_settled) and startup repair both reach this before the rules.
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
    apply_extraction_inner(
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
        true,
    )?;
    Ok(true)
}

// A single quoted name in an explicit original-model title identifies the body
// without using compatibility names from its description as ownership evidence.
// extracted_at is intentionally not a guard: a model run can already have stored
// category avatar and an owned-<id> link while leaving avatars.owned at 0.
pub(super) fn settle_named_original_body(
    connection: &mut Connection,
    product: &Product,
) -> Result<bool, String> {
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

pub(super) fn catalog_body(
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

pub(super) fn written_body_link(connection: &Connection, booth_id: &str) -> Result<bool, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_avatars
             WHERE product_id = ?1 AND source IN ('owned', 'catalog')",
            params![booth_id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(count > 0)
}

/// A catalog match or owned-body registration is already a classification.
/// When its version is behind, refresh it and tell the caller to leave the
/// model unused. A current row, a manual lock, and an ended listing are left alone.
pub(super) fn protect_written_body(
    connection: &Connection,
    product: &Product,
) -> Result<bool, String> {
    if product.category_locked
        || product.category != Some(Category::Avatar)
        || product.extracted_at.is_none()
        || product.description_snippet == crate::model::UNAVAILABLE_DESCRIPTION
        || !written_body_link(connection, &product.booth_id)?
    {
        return Ok(false);
    }
    let version: i64 = connection
        .query_row(
            "SELECT classifier_version FROM products WHERE booth_id = ?1",
            params![product.booth_id],
            |row| row.get(0),
        )
        .sql()?;
    if version >= i64::from(crate::classifier::CLASSIFIER_VERSION) {
        return Ok(false);
    }
    connection
        .execute(
            "UPDATE products SET classifier_version = ?1
             WHERE booth_id = ?2 AND category_locked = 0
               AND description_snippet != ?3",
            params![
                crate::classifier::CLASSIFIER_VERSION,
                product.booth_id,
                crate::model::UNAVAILABLE_DESCRIPTION
            ],
        )
        .sql()?;
    crate::sync_meta::touch_product(connection, &product.booth_id)?;
    Ok(true)
}

pub(super) fn write_catalog_body(
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
             user_confirmed = 1, extracted_at = COALESCE(extracted_at, ?2), updated_at = ?2,
             classifier_version = ?3
             WHERE booth_id = ?4 AND category_locked = 0",
            params![
                to_json(&[display.to_string()])?,
                now,
                crate::classifier::CLASSIFIER_VERSION,
                booth_id
            ],
        )
        .sql()?;
    replace_links(connection, booth_id, &[avatar_id.to_string()], "catalog")?;
    connection
        .execute(
            "UPDATE avatars SET owned = 1 WHERE id = ?1",
            params![avatar_id],
        )
        .sql()?;
    crate::sync_meta::touch_product(connection, booth_id)?;
    crate::sync_meta::touch_avatar(connection, avatar_id)
}

pub(super) struct OwnedTarget {
    pub(super) avatar_id: String,
    pub(super) created: bool,
    pub(super) display: String,
}

pub(super) enum OwnedName {
    Ready(OwnedTarget),
    Ask {
        suggested: String,
        candidates: Vec<String>,
    },
}

pub(super) fn decide_owned_name(
    connection: &Connection,
    product: &Product,
) -> Result<OwnedName, String> {
    if let Some((id, display)) = catalog_body(connection, &product.booth_id)? {
        return Ok(OwnedName::Ready(OwnedTarget {
            avatar_id: id,
            created: false,
            display,
        }));
    }
    let avatars = load_avatars(connection)?;
    let mut labels = usable_owned_labels(&product.compatible_avatars);
    if labels.is_empty() {
        labels = usable_owned_labels(&product.unresolved_names);
    }
    if !labels.is_empty() {
        return Ok(assign_placeholder(
            decide_from_labels(&labels, &avatars),
            &product.booth_id,
        ));
    }
    if let Some(quoted) = single_acceptable_quote(&product.title) {
        return Ok(assign_placeholder(
            decide_from_labels(std::slice::from_ref(&quoted), &avatars),
            &product.booth_id,
        ));
    }
    let suggested = acceptable_title_name(&product.title).unwrap_or_default();
    Ok(OwnedName::Ask {
        suggested,
        candidates: Vec::new(),
    })
}

pub(super) fn decide_from_labels(labels: &[String], avatars: &[crate::model::Avatar]) -> OwnedName {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for label in labels {
        if let Some(avatar) = crate::resolve::resolve_name(label, avatars) {
            if !resolved.iter().any(|id: &String| id == &avatar.id) {
                resolved.push(avatar.id.clone());
            }
        } else if !unresolved.iter().any(|existing: &String| existing == label) {
            unresolved.push(label.clone());
        }
    }
    if resolved.len() == 1 && unresolved.is_empty() {
        let avatar_id = resolved.pop().expect("one id");
        let display = avatars
            .iter()
            .find(|avatar| avatar.id == avatar_id)
            .map(|avatar| avatar.display_name.clone())
            .unwrap_or_default();
        return OwnedName::Ready(OwnedTarget {
            avatar_id,
            created: false,
            display,
        });
    }
    if resolved.is_empty() && unresolved.len() == 1 {
        let display = unresolved.pop().expect("one label");
        return OwnedName::Ready(OwnedTarget {
            avatar_id: String::new(),
            created: true,
            display,
        });
    }
    let mut candidates = Vec::new();
    for id in &resolved {
        if let Some(avatar) = avatars.iter().find(|avatar| &avatar.id == id) {
            push_unique(&mut candidates, &avatar.display_name);
        }
    }
    for label in &unresolved {
        push_unique(&mut candidates, label);
    }
    OwnedName::Ask {
        suggested: String::new(),
        candidates,
    }
}

pub(super) fn assign_placeholder(name: OwnedName, booth_id: &str) -> OwnedName {
    match name {
        OwnedName::Ready(mut target) if target.created && target.avatar_id.is_empty() => {
            target.avatar_id = format!("owned-{booth_id}");
            OwnedName::Ready(target)
        }
        other => other,
    }
}

pub(super) fn resolve_supplied_name(
    connection: &Connection,
    booth_id: &str,
    name: &str,
) -> Result<OwnedTarget, String> {
    let display = clean_owned_name(name)?;
    let avatars = load_avatars(connection)?;
    if let Some(avatar) = crate::resolve::resolve_name(&display, &avatars)
        .or_else(|| crate::resolve::resolve_name(name.trim(), &avatars))
    {
        return Ok(OwnedTarget {
            avatar_id: avatar.id.clone(),
            created: false,
            display: avatar.display_name.clone(),
        });
    }
    Ok(OwnedTarget {
        avatar_id: format!("owned-{booth_id}"),
        created: true,
        display,
    })
}

pub(super) fn clean_owned_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("アバター名を入力してください".into());
    }
    if trimmed.chars().count() > 80 {
        return Err("アバター名が長すぎます".into());
    }
    let display = crate::normalize::preferred_display_name(trimmed)
        .ok_or_else(|| "アバター名として使えません".to_string())?;
    if display == "名称未設定" || crate::normalize::is_blocked_avatar_label(&display) {
        return Err("アバター名として使えません".into());
    }
    Ok(display)
}

pub(super) fn usable_owned_labels(names: &[String]) -> Vec<String> {
    let mut labels = Vec::new();
    for name in names {
        let trimmed = name.trim();
        if trimmed.is_empty()
            || trimmed == "名称未設定"
            || crate::normalize::is_blocked_avatar_label(trimmed)
        {
            continue;
        }
        if let Some(display) = crate::normalize::preferred_display_name(trimmed) {
            push_unique(&mut labels, &display);
        }
    }
    labels
}

pub(super) fn single_acceptable_quote(title: &str) -> Option<String> {
    let segments = crate::normalize::quoted_segments(title);
    if segments.len() != 1 {
        return None;
    }
    let name = crate::normalize::preferred_display_name(&segments[0])?;
    if name == "名称未設定" || crate::normalize::is_blocked_avatar_label(&name) {
        None
    } else {
        Some(name)
    }
}

pub(super) fn acceptable_title_name(title: &str) -> Option<String> {
    let name = crate::normalize::suggested_display_name(title);
    if name.is_empty() || name == "名称未設定" || crate::normalize::is_blocked_avatar_label(&name)
    {
        None
    } else {
        Some(name)
    }
}

pub(super) fn push_unique(labels: &mut Vec<String>, label: &str) {
    if !label.is_empty() && !labels.iter().any(|existing| existing == label) {
        labels.push(label.to_string());
    }
}

/// A pinned avatar body whose link was written by owned-body registration.
pub(super) fn manual_owned_body(
    connection: &Connection,
    product: &Product,
) -> Result<Option<String>, String> {
    if !classification_pinned(product)
        || product.category != Some(Category::Avatar)
        || !product.links_locked
    {
        return Ok(None);
    }
    connection
        .query_row(
            "SELECT avatar_id FROM product_avatars
             WHERE product_id = ?1 AND source IN ('owned', 'catalog')
             ORDER BY avatar_id LIMIT 1",
            params![product.booth_id],
            |row| row.get(0),
        )
        .optional()
        .sql()
}

pub(super) fn release_manual_owned_body(
    connection: &mut Connection,
    product: &Product,
    avatar_id: &str,
) -> Result<ClassificationSnapshot, String> {
    let mut snapshot = classification_snapshot(product);
    snapshot.links_locked = Some(product.links_locked);
    snapshot.links = Some(snapshot_links(connection, &product.booth_id)?);
    remember_extracted_names(&mut snapshot, product);
    snapshot.owned_avatars.push(OwnedAvatarRestore {
        id: avatar_id.to_string(),
        display_name: avatar_display(connection, avatar_id)?,
        aliases: avatar_aliases(connection, avatar_id)?,
        owned: true,
        remove_if_unused: false,
    });
    let category = product
        .genre
        .as_deref()
        .and_then(crate::genre::category_for_genre)
        .map(|category| category.as_str().to_string());
    let tx = connection.transaction().sql()?;
    tx.execute(
        "UPDATE products SET category = ?1, category_locked = 0, genre_locked = 0, links_locked = 0,
         updated_at = ?2 WHERE booth_id = ?3",
        params![category, now_iso(), product.booth_id],
    )
    .sql()?;
    delete_links(&tx, &product.booth_id)?;
    if body_count(&tx, avatar_id)? == 0 {
        tx.execute(
            "UPDATE avatars SET owned = 0 WHERE id = ?1",
            params![avatar_id],
        )
        .sql()?;
    }
    crate::sync_meta::touch_product(&tx, &product.booth_id)?;
    crate::sync_meta::touch_avatar(&tx, avatar_id)?;
    tx.commit().sql()?;
    Ok(snapshot)
}

pub(super) fn apply_owned_restore(
    connection: &Connection,
    avatar: &OwnedAvatarRestore,
) -> Result<(), String> {
    if avatar.owned {
        if !avatar_exists(connection, &avatar.id)? {
            if !avatar.id.starts_with("owned-") {
                return Err(format!("アバター {} が対応表にありません", avatar.id));
            }
            insert_placeholder(
                connection,
                &avatar.id,
                &avatar.display_name,
                &avatar.aliases,
            )?;
        }
        connection
            .execute(
                "UPDATE avatars SET owned = 1 WHERE id = ?1",
                params![avatar.id],
            )
            .sql()?;
        return Ok(());
    }
    if body_count(connection, &avatar.id)? > 0 {
        return Ok(());
    }
    if avatar_exists(connection, &avatar.id)? {
        connection
            .execute(
                "UPDATE avatars SET owned = 0 WHERE id = ?1",
                params![avatar.id],
            )
            .sql()?;
    }
    if avatar.remove_if_unused && avatar.id.starts_with("owned-") {
        let links: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM product_avatars WHERE avatar_id = ?1",
                params![avatar.id],
                |row| row.get(0),
            )
            .sql()?;
        let builtin: i64 = connection
            .query_row(
                "SELECT is_builtin FROM avatars WHERE id = ?1",
                params![avatar.id],
                |row| row.get(0),
            )
            .optional()
            .sql()?
            .unwrap_or(0);
        if links == 0 && builtin == 0 {
            connection
                .execute("DELETE FROM avatars WHERE id = ?1", params![avatar.id])
                .sql()?;
        }
    }
    Ok(())
}

pub(super) fn snapshot_links(
    connection: &Connection,
    booth_id: &str,
) -> Result<Vec<SnapshotLink>, String> {
    let mut statement = connection
        .prepare(
            "SELECT avatar_id, source FROM product_avatars WHERE product_id = ?1 ORDER BY avatar_id",
        )
        .sql()?;
    let links = statement
        .query_map(params![booth_id], |row| {
            Ok(SnapshotLink {
                avatar_id: row.get(0)?,
                source: row.get(1)?,
            })
        })
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    Ok(links)
}

pub(super) fn avatar_is_owned(connection: &Connection, id: &str) -> Result<bool, String> {
    let owned: i64 = connection
        .query_row(
            "SELECT owned FROM avatars WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(owned != 0)
}

pub(super) fn body_count(connection: &Connection, avatar_id: &str) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM product_avatars
             JOIN products ON products.booth_id = product_avatars.product_id
             WHERE product_avatars.avatar_id = ?1 AND products.category = 'avatar'",
            params![avatar_id],
            |row| row.get(0),
        )
        .sql()
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

pub(super) fn save_owned_body(
    connection: &Connection,
    product: &Product,
    avatar_id: &str,
) -> Result<(), String> {
    let booth_id = &product.booth_id;
    connection
        .execute(
            "UPDATE avatars SET owned = 1 WHERE id = ?1",
            params![avatar_id],
        )
        .sql()?;
    let display = avatar_display(connection, avatar_id)?;
    let now = now_iso();
    connection
        .execute(
            "UPDATE products SET category = 'avatar', compatible_avatars_json = ?1,
             unresolved_names_json = '[]', is_generic = 0, confidence = 1, needs_review = 0,
             user_confirmed = 1, extracted_at = COALESCE(extracted_at, ?2), updated_at = ?2,
             classifier_version = ?3
             WHERE booth_id = ?4 AND category_locked = 0",
            params![
                to_json(&[display])?,
                now,
                crate::classifier::CLASSIFIER_VERSION,
                booth_id
            ],
        )
        .sql()?;
    if !product.links_locked {
        replace_links(connection, booth_id, &[avatar_id.to_string()], "owned")?;
    }
    crate::sync_meta::touch_product(connection, booth_id)?;
    crate::sync_meta::touch_avatar(connection, avatar_id)
}
