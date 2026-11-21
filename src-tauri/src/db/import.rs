//! Importing products: the export formats, upserts, and merging what BOOTH sent.
use super::*;

pub(crate) fn import_export_json(
    connection: &mut Connection,
    json: &str,
) -> Result<ImportReport, String> {
    import_export_json_with_mode(connection, json, false)
}

pub(crate) fn import_export_json_with_mode(
    connection: &mut Connection,
    json: &str,
    authoritative_files: bool,
) -> Result<ImportReport, String> {
    let parsed = crate::import_data::parse_export(json)?;
    if parsed.products.is_empty() {
        return Err(parsed
            .warnings
            .first()
            .cloned()
            .unwrap_or_else(|| "商品が1件もありません".into()));
    }
    import_products_with_mode(connection, &parsed, authoritative_files)
}

/// One import is one run of the log. The whole pass is compared, because a changed description
/// clears a row before the listed-compatibility pass and dictionary settling write it again.
pub(crate) fn import_products(
    connection: &mut Connection,
    parsed: &ParsedExport,
) -> Result<ImportReport, String> {
    import_products_with_mode(connection, parsed, false)
}

pub(crate) fn import_products_with_mode(
    connection: &mut Connection,
    parsed: &ParsedExport,
    authoritative_files: bool,
) -> Result<ImportReport, String> {
    let mut run = ClassificationRun::new(KIND_IMPORT, CLASSIFIER_CATALOG);
    observe_all(connection, &mut run, |connection| {
        import_products_unlogged(connection, parsed, authoritative_files)
    })
}

pub(super) fn import_products_unlogged(
    connection: &mut Connection,
    parsed: &ParsedExport,
    authoritative_files: bool,
) -> Result<ImportReport, String> {
    let mut before_files = std::collections::HashMap::new();
    for product in &parsed.products {
        if !product.downloads.is_empty()
            && let Ok(keys) =
                crate::asset_updates::existing_file_keys(connection, &product.booth_id)
        {
            before_files.insert(product.booth_id.clone(), keys);
        }
    }
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
    let _ = crate::asset_updates::record_imported_files(
        connection,
        &before_files,
        &parsed.products,
        authoritative_files,
        false,
    );
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

pub(super) enum Upsert {
    Inserted,
    Updated,
    Unchanged,
}

pub(super) fn upsert(
    connection: &Connection,
    incoming: &IncomingProduct,
) -> Result<Upsert, String> {
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
            crate::sync_meta::touch_product(connection, &existing.booth_id)?;
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
            crate::sync_meta::touch_product(connection, &existing.booth_id)?;
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

pub(super) fn finish_upsert(
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

pub(super) fn merge_library_facts(
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
    if incoming.received_gift == Some(true) {
        changed |= merge_flag(connection, &incoming.booth_id, "received_gift", Some(true))?;
    }
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
    changed |= merge_booth_meta(connection, incoming)?;
    Ok(changed)
}

pub(super) fn merge_booth_meta(
    connection: &Connection,
    incoming: &IncomingProduct,
) -> Result<bool, String> {
    let mut changed = false;
    if incoming.is_adult == Some(true) {
        changed |= stamp_booth_detail(connection, &incoming.booth_id)?;
        return Ok(changed);
    }
    if let Some(category) = &incoming.booth_category {
        let id = category.id.filter(|id| *id > 0);
        let name: String = category.name.trim().chars().take(80).collect();
        let parent: String = category.parent.trim().chars().take(80).collect();
        let updated = connection
            .execute(
                "UPDATE products SET
                    booth_category_id = ?1,
                    booth_category_name = ?2,
                    booth_category_parent = ?3
                 WHERE booth_id = ?4
                   AND (
                     booth_category_id IS NOT ?1
                     OR IFNULL(booth_category_name, '') IS NOT ?2
                     OR IFNULL(booth_category_parent, '') IS NOT ?3
                   )",
                params![id, name, parent, incoming.booth_id],
            )
            .sql()?;
        changed |= updated > 0;
        changed |= stamp_booth_detail(connection, &incoming.booth_id)?;
    }
    if let Some(tags) = &incoming.booth_tags {
        let cleaned = clean_booth_tags(tags);
        let json = to_json(&cleaned)?;
        let updated = connection
            .execute(
                "UPDATE products SET booth_tags = ?1
                 WHERE booth_id = ?2 AND IFNULL(booth_tags, '') IS NOT ?1",
                params![json, incoming.booth_id],
            )
            .sql()?;
        changed |= updated > 0;
        changed |= stamp_booth_detail(connection, &incoming.booth_id)?;
    }
    Ok(changed)
}

pub(super) fn clean_booth_tags(tags: &[String]) -> Vec<String> {
    let mut cleaned = Vec::new();
    for tag in tags {
        let name: String = tag.trim().chars().take(40).collect();
        if name.is_empty() || cleaned.iter().any(|existing| existing == &name) {
            continue;
        }
        cleaned.push(name);
        if cleaned.len() == 40 {
            break;
        }
    }
    cleaned
}

pub(super) fn stamp_booth_detail(connection: &Connection, booth_id: &str) -> Result<bool, String> {
    let updated = connection
        .execute(
            "UPDATE products SET booth_detail_at = ?1 WHERE booth_id = ?2 AND booth_detail_at IS NULL",
            params![now_iso(), booth_id],
        )
        .sql()?;
    Ok(updated > 0)
}

pub(super) fn merge_flag(
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
        "received_gift" => {
            "UPDATE products SET received_gift = ?1 WHERE booth_id = ?2 AND (received_gift IS NOT ?1)"
        }
        _ => return Err("不明な項目です".into()),
    };
    let changed = connection
        .execute(sql, params![if value { 1 } else { 0 }, booth_id])
        .sql()?;
    Ok(changed > 0)
}

pub(super) fn merge_variation(
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

pub(super) fn clean_downloadable_id(value: &str) -> String {
    let id = value.trim();
    if (1..=12).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit()) {
        id.to_string()
    } else {
        String::new()
    }
}

pub(super) fn filename_is_placeholder(filename: &str, downloadable_id: &str) -> bool {
    let name = filename.trim();
    if name.is_empty() || (!downloadable_id.is_empty() && name == downloadable_id) {
        return true;
    }
    let bytes = name.as_bytes();
    (4..=12).contains(&bytes.len()) && bytes.iter().all(|byte| byte.is_ascii_digit())
}

pub(super) fn merge_download(
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
