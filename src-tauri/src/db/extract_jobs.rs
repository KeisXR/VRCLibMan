//! Which products to classify next, and settling them from BOOTH categories and details.
use super::*;

pub(crate) struct ExtractJob {
    pub booth_id: String,
    pub title: String,
    pub description: String,
    pub booth_category_id: Option<i64>,
    pub booth_category_name: String,
    pub booth_category_parent: String,
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

pub(super) const EXTRACT_JOB_COLUMNS: &str = "booth_id, title, description_snippet, booth_category_id, booth_category_name, booth_category_parent";

/// 「未分類を分類」の対象。除外した商品と、カテゴリとジャンルの両方を手で固定した商品は入らない。
pub(super) const UNCLASSIFIED_WHERE: &str = "(extracted_at IS NULL OR category IS NULL)
                       AND NOT (category_locked = 1 AND genre_locked = 1)
                       AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)";

/// 「未分類を分類」で分類する商品が 1 件でもあるか。取り込みのあとの自動の分類（`auto_classify`）が見る。
pub(crate) fn has_unclassified_work(connection: &Connection) -> Result<bool, String> {
    connection
        .query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM products WHERE {UNCLASSIFIED_WHERE})"),
            [],
            |row| row.get(0),
        )
        .sql()
}

pub(super) fn map_extract_job(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExtractJob> {
    Ok(ExtractJob {
        booth_id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        booth_category_id: row.get(3)?,
        booth_category_name: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        booth_category_parent: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
    })
}

pub(crate) fn products_for_extract(
    connection: &Connection,
    target: ExtractTarget,
) -> Result<Vec<ExtractJob>, String> {
    let rows = match target {
        ExtractTarget::All => {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {EXTRACT_JOB_COLUMNS} FROM products
                     WHERE booth_id NOT IN (SELECT booth_id FROM product_exclusions)
                       AND NOT (category_locked = 1 AND genre_locked = 1)
                     ORDER BY updated_at DESC"
                ))
                .sql()?;
            statement
                .query_map([], map_extract_job)
                .sql()?
                .collect::<Result<Vec<_>, _>>()
                .sql()?
        }
        ExtractTarget::Unclassified => {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {EXTRACT_JOB_COLUMNS} FROM products
                     WHERE {UNCLASSIFIED_WHERE}
                     ORDER BY imported_at ASC"
                ))
                .sql()?;
            statement
                .query_map([], map_extract_job)
                .sql()?
                .collect::<Result<Vec<_>, _>>()
                .sql()?
        }
        ExtractTarget::Stale => {
            let mut statement = connection
                .prepare(&format!(
                    "SELECT {EXTRACT_JOB_COLUMNS} FROM products
                     WHERE extracted_at IS NOT NULL
                       AND category IS NOT NULL
                       AND classifier_version < ?1
                       AND description_snippet != ?2
                       AND NOT (category_locked = 1 AND genre_locked = 1)
                       AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
                     ORDER BY updated_at DESC"
                ))
                .sql()?;
            statement
                .query_map(
                    params![
                        crate::classifier::CLASSIFIER_VERSION,
                        crate::model::UNAVAILABLE_DESCRIPTION
                    ],
                    map_extract_job,
                )
                .sql()?
                .collect::<Result<Vec<_>, _>>()
                .sql()?
        }
    };
    Ok(rows)
}

pub(super) struct BoothMeta {
    pub(super) id: Option<i64>,
    pub(super) name: String,
    pub(super) tags: Vec<String>,
}

pub(super) fn booth_tags_of(
    connection: &Connection,
    booth_id: &str,
) -> Result<Vec<String>, String> {
    Ok(booth_meta(connection, booth_id)?.tags)
}

pub(super) fn booth_meta(connection: &Connection, booth_id: &str) -> Result<BoothMeta, String> {
    connection
        .query_row(
            "SELECT booth_category_id, booth_category_name, booth_tags FROM products WHERE booth_id = ?1",
            params![booth_id],
            |row| {
                let tags: Option<String> = row.get(2)?;
                Ok(BoothMeta {
                    id: row.get(0)?,
                    name: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    tags: parse_list(tags.as_deref().unwrap_or("[]")),
                })
            },
        )
        .sql()
}

/// Category id or name, plus tags when the category is ambiguous, decides the row.
/// A locked category that disagrees is left to the title rules. Both locks are already excluded.
pub(crate) fn apply_booth_settlement(
    connection: &mut Connection,
    booth_id: &str,
) -> Result<bool, String> {
    if product_excluded(connection, booth_id)? {
        return Ok(false);
    }
    let existing = require_product(connection, booth_id)?;
    if classification_pinned(&existing) {
        return Ok(false);
    }
    if existing.description_snippet == crate::model::UNAVAILABLE_DESCRIPTION {
        return Ok(false);
    }
    let meta = booth_meta(connection, booth_id)?;
    let Some(mut settlement) = crate::booth_category::settle(meta.id, &meta.name, &meta.tags)
    else {
        return Ok(false);
    };
    // 201 / 215 are a coarse tool bucket. Placement and avatar-gimmick text
    // move the row. A specific category such as outfit still ignores the title.
    if crate::booth_category::is_coarse_tool(meta.id, &meta.name)
        && let Some((category, genre)) =
            crate::classifier::coarse_tool_override(&existing.title, &existing.description_snippet)
    {
        settlement.category = category;
        settlement.genre = Some(genre);
    }
    if existing.category_locked && existing.category != Some(settlement.category) {
        return Ok(false);
    }
    let threshold = settings(connection)?.confidence_threshold;
    apply_extraction_inner(
        connection,
        booth_id,
        &Extraction {
            category: settlement.category,
            genre: settlement.genre,
            compatible_avatars: Vec::new(),
            is_generic: false,
            confidence: 0.95,
        },
        threshold,
        false,
    )?;
    Ok(true)
}

pub(crate) fn products_needing_booth_detail(
    connection: &Connection,
) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id FROM products
             WHERE booth_detail_at IS NULL
               AND IFNULL(is_adult, 0) = 0
               AND booth_id NOT IN (SELECT booth_id FROM product_exclusions)
             ORDER BY imported_at ASC",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| row.get(0))
        .sql()?
        .collect::<Result<Vec<_>, _>>()
        .sql()?;
    Ok(rows)
}

/// `true` means stop the batch. RetryLater and a transport error leave the row unchecked.
/// A changed description clears the classification, so that reset goes in the log.
pub(crate) fn store_booth_detail(
    connection: &mut Connection,
    booth_id: &str,
    outcome: &crate::booth_detail::DetailOutcome,
) -> Result<bool, String> {
    let mut run = ClassificationRun::new(KIND_BOOTH_DETAIL, CLASSIFIER_NONE);
    observe(connection, &mut run, booth_id, |connection| {
        store_booth_detail_unlogged(connection, booth_id, outcome)
    })
}

pub(super) fn store_booth_detail_unlogged(
    connection: &mut Connection,
    booth_id: &str,
    outcome: &crate::booth_detail::DetailOutcome,
) -> Result<bool, String> {
    use crate::booth_detail::DetailOutcome;
    if product_by_id(connection, booth_id)?.is_none() {
        return Ok(false);
    }
    match outcome {
        DetailOutcome::RetryLater => Ok(true),
        DetailOutcome::Again => Ok(false),
        DetailOutcome::Adult => {
            let _ = merge_flag(connection, booth_id, "is_adult", Some(true))?;
            let _ = stamp_booth_detail(connection, booth_id)?;
            Ok(false)
        }
        DetailOutcome::AgeGate => {
            let _ = stamp_booth_detail(connection, booth_id)?;
            Ok(false)
        }
        DetailOutcome::Missing => {
            let product = require_product(connection, booth_id)?;
            if product.description_snippet.trim().is_empty() {
                connection
                    .execute(
                        "UPDATE products SET description_snippet = ?1 WHERE booth_id = ?2",
                        params![crate::model::UNAVAILABLE_DESCRIPTION, booth_id],
                    )
                    .sql()?;
                note_unavailable(connection, booth_id)?;
            }
            let _ = stamp_booth_detail(connection, booth_id)?;
            Ok(false)
        }
        DetailOutcome::Public(item) => {
            let booth_category = if item.category_id.is_some() || !item.category_name.is_empty() {
                Some(crate::model::BoothCategoryInput {
                    id: item.category_id,
                    name: item.category_name.clone(),
                    parent: item.category_parent.clone(),
                })
            } else {
                None
            };
            let incoming = IncomingProduct {
                booth_id: booth_id.to_string(),
                title: String::new(),
                shop: item.shop.clone(),
                description_snippet: item.description.clone(),
                url: String::new(),
                thumbnail_url: None,
                variations: Vec::new(),
                downloads: Vec::new(),
                is_free: item.is_free,
                is_adult: item.is_adult,
                unpublished: None,
                received_gift: None,
                booth_category,
                booth_tags: Some(item.tags.clone()),
            };
            upsert(connection, &incoming)?;
            Ok(false)
        }
    }
}

pub(crate) fn classification_status(
    connection: &Connection,
) -> Result<crate::model::ClassificationStatus, String> {
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
