//! Files found by a folder scan and their links to products.
use super::*;

pub(crate) fn link_file(
    connection: &Connection,
    path: &str,
    product_id: Option<&str>,
) -> Result<(), String> {
    let found: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT path, linked_product_id FROM local_files WHERE path = ?1",
            params![path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .sql()?;
    let Some((_path, previous)) = found else {
        return Err("ファイルがスキャン結果にありません".into());
    };
    if let Some(product_id) = product_id {
        let product = require_product(connection, product_id)?;
        connection
            .execute(
                "UPDATE local_files SET linked_product_id = ?1, candidate_product_id = ?1,
                 match_score = 1, link_source = 'manual' WHERE path = ?2",
                params![product.booth_id, path],
            )
            .sql()?;
        crate::sync_meta::touch_file_links(connection, &product.booth_id)?;
        if let Some(prev) = previous.as_deref()
            && prev != product.booth_id
        {
            crate::sync_meta::touch_file_links(connection, prev)?;
        }
    } else {
        connection
            .execute(
                "UPDATE local_files SET linked_product_id = NULL, candidate_product_id = NULL,
                 match_score = 0, link_source = NULL WHERE path = ?1",
                params![path],
            )
            .sql()?;
        if let Some(prev) = previous.as_deref() {
            crate::sync_meta::touch_file_links(connection, prev)?;
        }
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
        return Err(
            "調査中にライブラリのフォルダが変更されたため、以前の結果は反映しませんでした".into(),
        );
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
    if !outcome.truncated {
        tx.execute(
            "DELETE FROM local_files WHERE (link_source IS NULL OR link_source != 'manual')
             AND COALESCE(linked_product_id, '') NOT IN (SELECT booth_id FROM product_exclusions)
             AND COALESCE(candidate_product_id, '') NOT IN (SELECT booth_id FROM product_exclusions)",
            [],
        )
        .sql()?;
    }
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
        if entry
            .labels
            .iter()
            .any(|label| excluded.iter().any(|id| contains_booth_id(label, id)))
        {
            continue;
        }
        if outcome.truncated {
            // An incomplete scan may only replace rows it actually visited.
            // Manual and excluded links keep the same protections as a full scan.
            tx.execute(
                "DELETE FROM local_files WHERE path = ?1
                 AND (link_source IS NULL OR link_source != 'manual')",
                [&entry.path],
            )
            .sql()?;
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
    crate::sync_meta::reconcile_file_links(&tx)?;
    tx.commit().sql()?;
    Ok(crate::model::ScanReport {
        indexed: outcome.entries.len() as u32,
        linked,
        truncated: outcome.truncated,
        root: root.to_string(),
    })
}
