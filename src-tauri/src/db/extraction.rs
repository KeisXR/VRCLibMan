//! Applying a classification to a product and resolving names again.
use super::*;

pub(super) fn apply_listed_compatibility(
    connection: &mut Connection,
    booth_id: &str,
) -> Result<(), String> {
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
        // The list adds links. Title keywords that find nothing must not undo a settled category.
        match (
            crate::compat::category_for_listed_product(
                &existing.title,
                &existing.description_snippet,
            ),
            existing.category,
        ) {
            (Category::Other, Some(current)) => current,
            (listed, _) => listed,
        }
    };
    let threshold = settings(connection)?.confidence_threshold;
    apply_extraction_inner(
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
        true,
    )
}

/// Applies one extraction and logs it under `run`.
pub(crate) fn apply_extraction_in_run(
    connection: &mut Connection,
    run: &mut ClassificationRun,
    booth_id: &str,
    extraction: &Extraction,
    threshold: f64,
) -> Result<(), String> {
    observe(connection, run, booth_id, |connection| {
        apply_extraction_inner(connection, booth_id, extraction, threshold, true)
    })
}

/// `apply_extraction_in_run` with a run of its own, for a call that has no entry point.
#[cfg(test)]
pub(crate) fn apply_extraction(
    connection: &mut Connection,
    booth_id: &str,
    extraction: &Extraction,
    threshold: f64,
) -> Result<(), String> {
    let mut run = ClassificationRun::new(KIND_IMPLICIT, CLASSIFIER_NONE);
    apply_extraction_in_run(connection, &mut run, booth_id, extraction, threshold)
}

pub(super) fn apply_extraction_inner(
    connection: &mut Connection,
    booth_id: &str,
    extraction: &Extraction,
    threshold: f64,
    title_fill: bool,
) -> Result<(), String> {
    if product_excluded(connection, booth_id)? {
        return Ok(());
    }
    let existing = require_product(connection, booth_id)?;
    if classification_pinned(&existing) {
        return Ok(());
    }
    let avatars = load_avatars(connection)?;
    let category = decide_category(
        &existing,
        extraction.category,
        extraction.confidence,
        title_fill,
    );
    // Download file and variation names stand in when nothing outside the text named an
    // avatar. Bodies and worlds do not take them.
    let suggested = if extraction.compatible_avatars.is_empty()
        && category.is_some_and(file_names_apply)
    {
        crate::compat::avatar_names_in_files(&product_file_labels(connection, booth_id)?, &avatars)
    } else {
        extraction.compatible_avatars.clone()
    };
    let names = crate::compat::names_for_extraction(
        &existing.title,
        &existing.description_snippet,
        &suggested,
        &avatars,
    );
    let tags = booth_tags_of(connection, booth_id)?;
    let genre = if existing.genre_locked {
        existing.genre.clone()
    } else {
        category.map(|category| {
            // Tags win only when the genre belongs to this category. A hair tag
            // must not turn a settled outfit into a hair genre, and a clothing
            // tag must not turn a tool into an outfit.
            let from_tags = crate::genre::genre_from_tags(category, &tags)
                .filter(|genre| crate::genre::fits(category, genre));
            let preferred = crate::genre::prefer_listing(
                category,
                &existing.title,
                &existing.description_snippet,
            );
            let from_title = if title_fill {
                crate::genre::classify_title_gap(
                    category,
                    &existing.title,
                    &existing.description_snippet,
                )
                .or_else(|| {
                    crate::genre::classify_text(
                        category,
                        &existing.title,
                        &existing.description_snippet,
                    )
                })
            } else {
                crate::genre::classify_text(
                    category,
                    &existing.title,
                    &existing.description_snippet,
                )
            };
            // A product-type phrase wins over tags. A BOOTH settlement has already
            // consulted tags; keep its selected genre, including a coarse-tool text override.
            let selected = if title_fill {
                preferred
                    .clone()
                    .or(from_tags)
                    .or(from_title)
                    .or_else(|| extraction.genre.clone())
            } else {
                preferred
                    .or_else(|| extraction.genre.clone())
                    .or(from_tags)
                    .or(from_title)
            };
            crate::genre::validate(category, selected.as_deref())
        })
    };
    let says_generic = if category == Some(Category::World) {
        false
    } else {
        extraction.is_generic
    };
    let is_generic = generic_without_avatars(category, &names, says_generic);
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
        for draft in &output.created_avatars {
            crate::sync_meta::touch_avatar(&tx, &draft.id)?;
        }
        for (avatar_id, _) in &output.readings {
            crate::sync_meta::touch_avatar(&tx, avatar_id)?;
        }
    }
    crate::sync_meta::touch_product(&tx, booth_id)?;
    tx.commit().sql()?;
    // Resolution records an owned-<booth id> placeholder and source "extracted"
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

pub(super) fn generic_without_avatars(
    category: Option<Category>,
    names: &[String],
    says_generic: bool,
) -> bool {
    if category == Some(Category::Accessory)
        && names
            .iter()
            .all(|name| crate::normalize::is_blocked_avatar_label(name))
    {
        return true;
    }
    // Texture and gimmick with no avatar, and none listed for one, are generic.
    // The generic phrase in the text does not decide this.
    if matches!(category, Some(Category::Texture | Category::Gimmick)) {
        return names.is_empty();
    }
    says_generic
}

pub(super) fn jp_readings(products: &[Product]) -> HashMap<String, String> {
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

pub(super) fn fold_romaji_names(
    names: &[String],
    readings: &HashMap<String, String>,
) -> Vec<String> {
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

pub(super) fn established_avatar_sources(
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

pub(super) fn review_with_established_placeholders(
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

pub(super) fn created_names_are_grounded(
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

/// Re-resolves every product and logs the changes under `run`.
pub(crate) fn reresolve_all_in_run(
    connection: &mut Connection,
    run: &mut ClassificationRun,
) -> Result<u32, String> {
    observe_all(connection, run, reresolve_products)
}

/// `reresolve_all_in_run` with a run of its own, for a call that has no entry point.
#[cfg(test)]
pub(crate) fn reresolve_all(connection: &mut Connection) -> Result<u32, String> {
    let mut run = ClassificationRun::new(KIND_IMPLICIT, CLASSIFIER_NONE);
    reresolve_all_in_run(connection, &mut run)
}

/// The catalog applied again, from the settings maintenance button.
pub(crate) fn reapply_catalog(connection: &mut Connection) -> Result<u32, String> {
    let mut run = ClassificationRun::new(KIND_CATALOG, CLASSIFIER_CATALOG);
    reresolve_all_in_run(connection, &mut run)
}

pub(super) fn reresolve_after_edit(connection: &mut Connection) -> Result<u32, String> {
    let mut run = ClassificationRun::new(KIND_EDIT, CLASSIFIER_CATALOG);
    reresolve_all_in_run(connection, &mut run)
}

pub(super) fn reresolve_products(connection: &mut Connection) -> Result<u32, String> {
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
    crate::sync_meta::reconcile_all(&tx)?;
    tx.commit().sql()?;
    Ok(processed)
}

pub(super) fn classification_pinned(product: &Product) -> bool {
    product.category_locked && product.genre_locked
}

pub(super) fn classification_snapshot(product: &Product) -> ClassificationSnapshot {
    ClassificationSnapshot {
        booth_id: product.booth_id.clone(),
        category: product
            .category
            .map(|category| category.as_str().to_string()),
        genre: product.genre.clone(),
        category_locked: product.category_locked,
        genre_locked: product.genre_locked,
        user_confirmed: product.user_confirmed,
        needs_review: product.needs_review,
        links_locked: None,
        links: None,
        owned_avatars: Vec::new(),
        compatible_avatars: None,
        unresolved_names: None,
        is_generic: None,
    }
}

pub(super) fn remember_extracted_names(snapshot: &mut ClassificationSnapshot, product: &Product) {
    snapshot.compatible_avatars = Some(product.compatible_avatars.clone());
    snapshot.unresolved_names = Some(product.unresolved_names.clone());
    snapshot.is_generic = Some(product.is_generic);
}
