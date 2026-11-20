//! Manual classification: genre moves, undo, owned bodies, edits, and the feedback export.
use super::*;

pub(super) const DESCRIPTION_HEAD_CHARS: usize = 400;
pub(super) const CLASSIFICATION_FEEDBACK_FORMAT: &str = "vrclibman.classification-feedback";
pub(super) const CLASSIFICATION_FEEDBACK_PURPOSE: &str = "分類ルールの改善に渡すデータです。説明は先頭だけです。corrections は手動で移した履歴と、自動分類へ戻した操作、その取り消しです。過去の移動は復元していません。manualClassifications はいま手動で固定している分類です。トークンやファイルの場所は含みません。";

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ClassificationPlacement {
    category: Option<String>,
    genre: Option<String>,
}

pub(super) fn placement_of(product: &Product) -> ClassificationPlacement {
    ClassificationPlacement {
        category: product
            .category
            .map(|category| category.as_str().to_string()),
        genre: product.genre.clone(),
    }
}

pub(super) fn record_classification_move(
    connection: &Connection,
    booth_id: &str,
    action: &str,
    from: &ClassificationPlacement,
    to: Option<&ClassificationPlacement>,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO classification_moves (
                booth_id, at, action, from_category, from_genre, to_category, to_genre
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                booth_id,
                now_iso(),
                action,
                from.category,
                from.genre,
                to.and_then(|place| place.category.clone()),
                to.and_then(|place| place.genre.clone()),
            ],
        )
        .sql()?;
    Ok(())
}

/// Move one product onto an asset or world genre and pin it by locking category and genre together.
/// Returns the classification from before the move.
pub(crate) fn move_product_genre(
    connection: &mut Connection,
    booth_id: &str,
    kind: &str,
    genre: &str,
) -> Result<ClassificationSnapshot, String> {
    move_product_genre_with_avatars(connection, booth_id, kind, genre, None)
}

pub(crate) fn move_product_genre_with_avatars(
    connection: &mut Connection,
    booth_id: &str,
    kind: &str,
    genre: &str,
    avatar_ids: Option<Vec<String>>,
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
    let mut snapshot = classification_snapshot(&product);
    if avatar_ids.is_some() {
        snapshot.links = Some(snapshot_links(connection, booth_id)?);
        snapshot.links_locked = Some(product.links_locked);
        snapshot.is_generic = Some(product.is_generic);
    }
    let already = product.category == Some(category)
        && product.genre.as_deref() == Some(genre)
        && classification_pinned(&product);
    if !already || avatar_ids.is_some() {
        let from = placement_of(&product);
        let to = ClassificationPlacement {
            category: Some(category.as_str().to_string()),
            genre: Some(validated.to_string()),
        };
        update_product(
            connection,
            &ProductUpdate {
                purchased_editions: None,
                booth_id: booth_id.to_string(),
                category: Some(category.as_str().to_string()),
                genre: Some(genre.to_string()),
                reset_genre: false,
                is_generic: avatar_ids
                    .as_ref()
                    .filter(|ids| !ids.is_empty())
                    .map(|_| false),
                avatar_ids,
            },
        )?;
        record_classification_move(connection, booth_id, "move", &from, Some(&to))?;
    }
    Ok(snapshot)
}

pub(crate) fn restore_product_classification(
    connection: &mut Connection,
    snapshot: &ClassificationSnapshot,
) -> Result<(), String> {
    let product = require_product(connection, &snapshot.booth_id)?;
    if let Some(category) = snapshot.category.as_deref()
        && Category::parse(category).is_none()
    {
        return Err("カテゴリが不正です".into());
    }
    let from = placement_of(&product);
    let to = ClassificationPlacement {
        category: snapshot.category.clone(),
        genre: snapshot.genre.clone(),
    };
    let changed = from != to
        || product.category_locked != snapshot.category_locked
        || product.genre_locked != snapshot.genre_locked;
    let tx = connection.transaction().sql()?;
    tx.execute(
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
    if let Some(locked) = snapshot.links_locked {
        tx.execute(
            "UPDATE products SET links_locked = ?1 WHERE booth_id = ?2",
            params![locked, snapshot.booth_id],
        )
        .sql()?;
    }
    if let Some(links) = &snapshot.links {
        delete_links(&tx, &snapshot.booth_id)?;
        for link in links {
            tx.execute(
                "INSERT OR IGNORE INTO product_avatars (product_id, avatar_id, source) VALUES (?1, ?2, ?3)",
                params![snapshot.booth_id, link.avatar_id, link.source],
            )
            .sql()?;
        }
    }
    for avatar in &snapshot.owned_avatars {
        apply_owned_restore(&tx, avatar)?;
    }
    if let Some(names) = &snapshot.compatible_avatars {
        tx.execute(
            "UPDATE products SET compatible_avatars_json = ?1 WHERE booth_id = ?2",
            params![to_json(names)?, snapshot.booth_id],
        )
        .sql()?;
    }
    if let Some(names) = &snapshot.unresolved_names {
        tx.execute(
            "UPDATE products SET unresolved_names_json = ?1 WHERE booth_id = ?2",
            params![to_json(names)?, snapshot.booth_id],
        )
        .sql()?;
    }
    if let Some(generic) = snapshot.is_generic {
        tx.execute(
            "UPDATE products SET is_generic = ?1 WHERE booth_id = ?2",
            params![generic, snapshot.booth_id],
        )
        .sql()?;
    }
    // Genre-move undo is the history #72 records. An owned-body snapshot carries
    // links, and that registration stays out of classification_moves.
    if changed && snapshot.links.is_none() {
        record_classification_move(&tx, &snapshot.booth_id, "restore", &from, Some(&to))?;
    }
    crate::sync_meta::touch_product(&tx, &snapshot.booth_id)?;
    for avatar in &snapshot.owned_avatars {
        crate::sync_meta::touch_avatar(&tx, &avatar.id)?;
    }
    tx.commit().sql()?;
    Ok(())
}

/// Clear both locks so the next reclassification may replace this placement.
/// A product pinned as an owned avatar body is also unregistered: the category
/// returns to the one implied by its genre, and the body link is removed.
/// Returns the pinned snapshot so the unpin itself can be undone.
pub(crate) fn unpin_product_classification(
    connection: &mut Connection,
    booth_id: &str,
) -> Result<ClassificationSnapshot, String> {
    let product = require_product(connection, booth_id)?;
    if !classification_pinned(&product) {
        return Err("この商品は手動で固定されていません".into());
    }
    if let Some(avatar_id) = manual_owned_body(connection, &product)? {
        return release_manual_owned_body(connection, &product, &avatar_id);
    }
    let snapshot = classification_snapshot(&product);
    let from = placement_of(&product);
    connection
        .execute(
            "UPDATE products SET category_locked = 0, genre_locked = 0, updated_at = ?1 WHERE booth_id = ?2",
            params![now_iso(), booth_id],
        )
        .sql()?;
    record_classification_move(connection, booth_id, "unpin", &from, None)?;
    crate::sync_meta::touch_product(connection, booth_id)?;
    Ok(snapshot)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ClassificationFeedbackProduct {
    booth_id: String,
    title: String,
    shop: String,
    description_head: String,
    category: Option<String>,
    genre: Option<String>,
    classification_status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ClassificationCorrection {
    booth_id: String,
    at: String,
    action: String,
    from: ClassificationPlacement,
    to: Option<ClassificationPlacement>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ClassificationFeedbackFile {
    format: &'static str,
    version: u32,
    scope: String,
    description_head_chars: usize,
    purpose: &'static str,
    products: Vec<ClassificationFeedbackProduct>,
    corrections: Vec<ClassificationCorrection>,
    manual_classifications: Vec<ClassificationFeedbackProduct>,
}

pub(super) fn description_head(text: &str) -> String {
    text.chars().take(DESCRIPTION_HEAD_CHARS).collect()
}

pub(super) fn feedback_status(product: &Product) -> &'static str {
    if product.category.is_none() {
        "unclassified"
    } else if classification_pinned(product) {
        "manual"
    } else if product.needs_review {
        "needs_review"
    } else {
        "classified"
    }
}

pub(super) fn feedback_product(product: &Product) -> ClassificationFeedbackProduct {
    ClassificationFeedbackProduct {
        booth_id: product.booth_id.clone(),
        title: product.title.clone(),
        shop: product.shop.clone(),
        description_head: description_head(&product.description_snippet),
        category: product
            .category
            .map(|category| category.as_str().to_string()),
        genre: product.genre.clone(),
        classification_status: feedback_status(product).to_string(),
    }
}

pub(super) fn feedback_scope_matches(product: &Product, scope: &str) -> bool {
    let unclassified = product.category.is_none();
    let other = product.category == Some(Category::Other);
    match scope {
        "other" => other,
        "unclassified" => unclassified,
        "both" => other || unclassified,
        _ => false,
    }
}

pub(super) fn classification_corrections(
    connection: &Connection,
) -> Result<Vec<ClassificationCorrection>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id, at, action, from_category, from_genre, to_category, to_genre
             FROM classification_moves
             WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)
             ORDER BY id",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            let action: String = row.get(2)?;
            let to = if action == "unpin" {
                None
            } else {
                Some(ClassificationPlacement {
                    category: row.get(5)?,
                    genre: row.get(6)?,
                })
            };
            Ok(ClassificationCorrection {
                booth_id: row.get(0)?,
                at: row.get(1)?,
                action,
                from: ClassificationPlacement {
                    category: row.get(3)?,
                    genre: row.get(4)?,
                },
                to,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

/// JSON for improving classification rules. No URLs, thumbnails, tokens, or local paths.
pub(crate) fn classification_feedback_json(
    connection: &Connection,
    scope: &str,
) -> Result<String, String> {
    if !matches!(scope, "other" | "unclassified" | "both") {
        return Err("対象が不正です".into());
    }
    let excluded = excluded_product_ids(connection)?;
    let mut products: Vec<Product> = load_products(connection)?
        .into_iter()
        .filter(|product| {
            !excluded.contains(&product.booth_id) && feedback_scope_matches(product, scope)
        })
        .collect();
    products.sort_by(|left, right| left.booth_id.cmp(&right.booth_id));
    let mut manual: Vec<Product> = load_products(connection)?
        .into_iter()
        .filter(|product| !excluded.contains(&product.booth_id) && classification_pinned(product))
        .collect();
    manual.sort_by(|left, right| left.booth_id.cmp(&right.booth_id));
    let file = ClassificationFeedbackFile {
        format: CLASSIFICATION_FEEDBACK_FORMAT,
        version: 1,
        scope: scope.to_string(),
        description_head_chars: DESCRIPTION_HEAD_CHARS,
        purpose: CLASSIFICATION_FEEDBACK_PURPOSE,
        products: products.iter().map(feedback_product).collect(),
        corrections: classification_corrections(connection)?,
        manual_classifications: manual.iter().map(feedback_product).collect(),
    };
    let mut json =
        serde_json::to_string_pretty(&file).map_err(|_| "JSONにできません".to_string())?;
    json.push('\n');
    Ok(json)
}

pub(crate) fn save_classification_feedback(
    connection: &Connection,
    scope: &str,
    path: &std::path::Path,
) -> Result<(), String> {
    let json = classification_feedback_json(connection, scope)?;
    save_classification_feedback_body(path, &json)
}

pub(crate) fn save_classification_feedback_body(
    path: &std::path::Path,
    body: &str,
) -> Result<(), String> {
    let destination = checked_json_destination(path)?;
    std::fs::write(&destination, body).map_err(|_| "ファイルを保存できません".to_string())?;
    Ok(())
}

pub(crate) fn checked_json_destination(
    path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    checked_destination(path, ".json", "保存先は JSON ファイルにしてください")
}

pub(crate) fn checked_zip_destination(
    path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    checked_destination(path, ".zip", "保存先は ZIP ファイルにしてください")
}

/// The save path for a chosen file: a name with `suffix` in an existing folder, and not a symlink.
pub(super) fn checked_destination(
    path: &std::path::Path,
    suffix: &str,
    wrong_kind: &str,
) -> Result<std::path::PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            !name.is_empty()
                && *name != suffix
                && name.to_ascii_lowercase().ends_with(suffix)
                && !name.contains(['/', '\\', '\0'])
        })
        .ok_or(wrong_kind)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or("保存先が不正です")?;
    let parent = parent
        .canonicalize()
        .map_err(|_| "保存先のフォルダを確認できません".to_string())?;
    if !parent.is_dir() {
        return Err("保存先のフォルダを確認できません".into());
    }
    let destination = parent.join(name);
    if destination
        .symlink_metadata()
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err("シンボリックリンクには保存できません".into());
    }
    Ok(destination)
}

/// Register a product that is not already an avatar body as an owned avatar.
/// A single name from the extraction or the dictionary is used as-is.
/// Several names, or none, ask for a name and leave the product unchanged.
pub(crate) fn register_manual_owned_body(
    connection: &mut Connection,
    booth_id: &str,
    name: Option<&str>,
) -> Result<RegisterOwnedBodyResult, String> {
    if product_excluded(connection, booth_id)? {
        return Err("この商品は除外されています".into());
    }
    let product = require_product(connection, booth_id)?;
    if product.category == Some(Category::Avatar) {
        return Err("この商品はすでにアバター本体です".into());
    }
    let supplied = name.is_some();
    let target = if let Some(name) = name {
        resolve_supplied_name(connection, booth_id, name)?
    } else {
        match decide_owned_name(connection, &product)? {
            OwnedName::Ready(target) => target,
            OwnedName::Ask {
                suggested,
                candidates,
            } => {
                return Ok(RegisterOwnedBodyResult::NeedsName {
                    suggested_name: suggested,
                    candidates,
                });
            }
        }
    };
    let previous_links = snapshot_links(connection, booth_id)?;
    let existed = avatar_exists(connection, &target.avatar_id)?;
    let was_owned = existed && avatar_is_owned(connection, &target.avatar_id)?;
    let mut snapshot = classification_snapshot(&product);
    snapshot.links_locked = Some(product.links_locked);
    snapshot.links = Some(previous_links);
    remember_extracted_names(&mut snapshot, &product);
    if !was_owned {
        snapshot.owned_avatars.push(OwnedAvatarRestore {
            id: target.avatar_id.clone(),
            display_name: target.display.clone(),
            aliases: Vec::new(),
            owned: false,
            remove_if_unused: !existed,
        });
    }
    let tx = connection.transaction().sql()?;
    tx.execute(
        "UPDATE products SET category_locked = 0, links_locked = 0 WHERE booth_id = ?1",
        params![booth_id],
    )
    .sql()?;
    if target.created {
        if existed {
            tx.execute(
                "UPDATE avatars SET display_name = ?1 WHERE id = ?2 AND is_builtin = 0",
                params![target.display, target.avatar_id],
            )
            .sql()?;
        } else {
            insert_placeholder(&tx, &target.avatar_id, &target.display, &[])?;
        }
    }
    let unlocked = require_product(&tx, booth_id)?;
    let from_catalog =
        !supplied && catalog_body(&tx, booth_id)?.is_some_and(|(id, _)| id == target.avatar_id);
    if from_catalog {
        write_catalog_body(&tx, booth_id, &target.avatar_id, &target.display)?;
    } else {
        save_owned_body(&tx, &unlocked, &target.avatar_id)?;
    }
    tx.execute(
        "UPDATE products SET category_locked = 1, genre_locked = 1, links_locked = 1, updated_at = ?1
         WHERE booth_id = ?2",
        params![now_iso(), booth_id],
    )
    .sql()?;
    crate::sync_meta::touch_product(&tx, booth_id)?;
    crate::sync_meta::touch_avatar(&tx, &target.avatar_id)?;
    tx.commit().sql()?;
    let display_name = avatar_display(connection, &target.avatar_id)?;
    Ok(RegisterOwnedBodyResult::Registered {
        snapshot,
        avatar_id: target.avatar_id,
        display_name,
    })
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
    crate::sync_meta::touch_product(&tx, booth_id)?;
    tx.commit().sql()?;
    Ok(())
}

pub(crate) fn update_product(
    connection: &mut Connection,
    update: &ProductUpdate,
) -> Result<(), String> {
    let mut product = require_product(connection, &update.booth_id)?;
    let threshold = settings(connection)?.confidence_threshold;
    let fixed_genre = update.genre.as_deref().filter(|value| !value.is_empty());
    if update.reset_genre && fixed_genre.is_some() {
        return Err("ジャンルの固定と「自動」は同時に指定できません".into());
    }
    if let Some(category) = &update.category {
        product.category = Some(Category::parse(category).ok_or("カテゴリが不正です")?);
        product.category_locked = true;
    }
    if let Some(genre) = fixed_genre {
        let category = product
            .category
            .ok_or("ジャンルを選ぶ前にカテゴリを選んでください")?;
        product.genre = Some(crate::genre::validate(category, Some(genre)));
        product.genre_locked = true;
    } else if update.reset_genre {
        // 「自動」: the genre follows the category, tags and text again, and later
        // reclassification may change it. A locked category stays as it is.
        product.genre = match product.category {
            Some(category) => Some(automatic_genre(connection, &product, category)?),
            None => None,
        };
        product.genre_locked = false;
    } else if update.category.is_some() {
        product.genre = product
            .category
            .map(|category| crate::genre::validate(category, product.genre.as_deref()));
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
        replacement = Some(ids.clone());
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
    if let Some(choice) = &update.purchased_editions {
        crate::purchased_editions::save(&tx, &product.booth_id, choice)?;
    }
    crate::sync_meta::touch_product(&tx, &product.booth_id)?;
    tx.commit().sql()?;
    Ok(())
}

/// The genre the rules choose for this category without an extraction run: a product-type
/// phrase first, then tags that fit the category, then the keyword rules on the text.
pub(super) fn automatic_genre(
    connection: &Connection,
    product: &Product,
    category: Category,
) -> Result<String, String> {
    let tags = booth_tags_of(connection, &product.booth_id)?;
    let preferred =
        crate::genre::prefer_listing(category, &product.title, &product.description_snippet);
    let from_tags = crate::genre::genre_from_tags(category, &tags)
        .filter(|genre| crate::genre::fits(category, genre));
    let from_text =
        crate::genre::classify_text(category, &product.title, &product.description_snippet);
    Ok(crate::genre::validate(
        category,
        preferred.or(from_tags).or(from_text).as_deref(),
    ))
}
