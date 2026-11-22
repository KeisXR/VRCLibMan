//! Variations and download rows: free files, staging, and files received by the browser.
use super::*;

pub(super) fn load_variations(connection: &Connection) -> Result<Vec<VariationFact>, String> {
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

pub(super) fn load_download_rows(connection: &Connection) -> Result<Vec<DownloadFact>, String> {
    let mut statement = connection
        .prepare(
            "SELECT booth_id, variation, filename, size_bytes, downloaded, local_path, downloadable_id
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
                downloadable_id: row
                    .get::<_, Option<String>>(6)?
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(super) fn load_product_flags(connection: &Connection) -> Result<Vec<ProductFlag>, String> {
    // 購入履歴に、贈ったギフトでない注文として載っている商品。カードは「無料配布のファイルを探す」の
    // 代わりに「購入したファイルを読み込む」を出す。
    let mut statement = connection
        .prepare(
            "SELECT p.booth_id, p.is_free, p.is_adult, p.unpublished,
                    EXISTS (SELECT 1 FROM purchase_items i JOIN purchase_orders o ON o.order_id = i.order_id
                            WHERE i.booth_id = p.booth_id AND COALESCE(o.gift_override, o.gift_detected) = 0)
             FROM products p",
        )
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
                purchased: row.get::<_, i64>(4)? != 0,
            })
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(super) fn booth_id_in_path(path: &str, ids: &HashSet<String>) -> Option<String> {
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

pub(super) fn sync_scanned_downloads(connection: &Connection) -> Result<(), String> {
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
        received_gift: None,
        booth_category: None,
        booth_tags: None,
    };
    upsert(connection, &incoming)?;
    Ok(booth_id)
}

/// この商品のファイル行の数。
pub(crate) fn product_file_count(connection: &Connection, booth_id: &str) -> Result<usize, String> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM product_files WHERE booth_id = ?1",
            params![booth_id],
            |row| row.get(0),
        )
        .sql()?;
    Ok(count.max(0) as usize)
}

/// The product must exist and not be excluded before the public read starts.
pub(crate) fn free_files_target(connection: &Connection, booth_id: &str) -> Result<(), String> {
    if product_by_id(connection, booth_id)?.is_none() {
        return Err("商品が見つかりません".into());
    }
    if product_excluded(connection, booth_id)? {
        return Err("この商品は除外されています".into());
    }
    Ok(())
}

/// Rows from「ファイルを取得」. Title, shop, description, category, and tags stay as they are.
/// Returns how many rows the product has after the merge.
pub(crate) fn store_free_files(
    connection: &mut Connection,
    booth_id: &str,
    files: &[IncomingDownload],
    is_free: Option<bool>,
    is_adult: Option<bool>,
) -> Result<usize, String> {
    store_file_rows(
        connection,
        booth_id,
        files,
        &[],
        is_free,
        if is_adult == Some(true) {
            Some(true)
        } else {
            None
        },
    )
}

/// 購入した商品のファイル行とバリエーション。BOOTH のライブラリから読んだ 1 商品分
/// （`browser::read_purchased_files`）。題・ショップ・説明・カテゴリ・タグ・無料の印は書かない。
/// ライブラリに無い商品と除外中の商品は拒む。保存後の商品のファイル行の数を返す。
pub(crate) fn store_purchased_files(
    connection: &mut Connection,
    product: &IncomingProduct,
) -> Result<usize, String> {
    store_file_rows(
        connection,
        &product.booth_id,
        &product.downloads,
        &product.variations,
        None,
        if product.is_adult == Some(true) {
            Some(true)
        } else {
            None
        },
    )
}

fn store_file_rows(
    connection: &mut Connection,
    booth_id: &str,
    files: &[IncomingDownload],
    variations: &[IncomingVariation],
    is_free: Option<bool>,
    is_adult: Option<bool>,
) -> Result<usize, String> {
    free_files_target(connection, booth_id)?;
    let mut before_files = std::collections::HashMap::new();
    if let Ok(keys) = crate::asset_updates::existing_file_keys(connection, booth_id) {
        before_files.insert(booth_id.to_string(), keys);
    }
    let incoming = IncomingProduct {
        booth_id: booth_id.to_string(),
        title: String::new(),
        shop: String::new(),
        description_snippet: String::new(),
        url: String::new(),
        thumbnail_url: None,
        variations: variations.to_vec(),
        downloads: files.to_vec(),
        is_free,
        is_adult,
        unpublished: None,
        received_gift: None,
        booth_category: None,
        booth_tags: None,
    };
    let transaction = connection.transaction().sql()?;
    upsert(&transaction, &incoming)?;
    let count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM product_files WHERE booth_id = ?1",
            params![booth_id],
            |row| row.get(0),
        )
        .sql()?;
    transaction.commit().sql()?;
    let _ = crate::asset_updates::record_imported_files(
        connection,
        &before_files,
        std::slice::from_ref(&incoming),
        true,
        true,
    );
    Ok(count.max(0) as usize)
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
    if let Some(claimed) = crate::library_files::safe_filename(&notice.filename)
        && filename != claimed
    {
        return Err("ファイル名がパスと一致しません".into());
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
            received_gift: None,
            booth_category: None,
            booth_tags: None,
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
    record_extension_download(connection, booth_id, variation, path, size, None)
}

pub(crate) fn record_extension_download(
    connection: &Connection,
    booth_id: &str,
    variation: &str,
    path: &std::path::Path,
    size: Option<u64>,
    downloadable_id: Option<&str>,
) -> Result<(), String> {
    let disk_size = std::fs::metadata(path).ok().map(|meta| meta.len());
    let size = disk_size.or(size);
    let actual_filename = crate::library_files::safe_filename(
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(""),
    )
    .ok_or_else(|| "ファイル名を読めません".to_string())?;
    let variation = crate::library_files::clean_variation(variation).unwrap_or_default();
    let filename = if let Some(id) = downloadable_id {
        let mut statement = connection.prepare(
            "SELECT filename FROM product_files WHERE booth_id=?1 AND variation=?2 AND downloadable_id=?3"
        ).sql()?;
        let names = statement
            .query_map(params![booth_id, variation, id], |row| {
                row.get::<_, String>(0)
            })
            .sql()?
            .collect::<rusqlite::Result<Vec<_>>>()
            .sql()?;
        if names.len() != 1 {
            return Err("どのファイルか確かめられません".into());
        }
        names[0].clone()
    } else {
        actual_filename.clone()
    };
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
    if downloadable_id.is_some() {
        // Startup recovery may have recorded this same disk file before the exact
        // variation arrived. Drop only that duplicate fact, never a physical file.
        connection.execute(
            "DELETE FROM product_files WHERE booth_id=?1 AND local_path=?2 AND downloadable_id IS NULL AND (variation!=?3 OR filename!=?4)",
            params![booth_id,path.display().to_string(),variation,filename],
        ).sql()?;
    }
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
                actual_filename,
                bytes,
                booth_id,
                now_iso()
            ],
        )
        .sql()?;
    crate::sync_meta::touch_file_links(connection, booth_id)?;
    let stored_id: Option<String> = connection
        .query_row(
            "SELECT downloadable_id FROM product_files WHERE booth_id = ?1 AND variation = ?2 AND filename = ?3",
            params![booth_id, variation, filename],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
        .or_else(|| downloadable_id.map(str::to_string));
    let _ = crate::asset_updates::clear_new_file_on_download(
        connection,
        booth_id,
        stored_id.as_deref(),
        &filename,
    );
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
pub(crate) const BROWSER_SAME_FILE_MESSAGE: &str = "このファイルはダウンロード中です";
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
        .or_else(|| crate::library_files::filename_is_number(&filename).then(|| filename.clone()))
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
                    params![
                        new_filename,
                        downloadable_id,
                        booth_id,
                        variation,
                        old_filename
                    ],
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
    let placed = crate::library_files::place_into_library(
        &ready.source,
        ready.library.as_deref(),
        &ready.booth_id,
    )?;
    record_placed_download(
        connection,
        &ready.booth_id,
        &ready.variation,
        &placed,
        notice.size,
    )?;
    Ok(placed)
}

/// 画面から「開く」を頼まれたパスが、スキャン済みのファイルか、
/// 商品ファイルとして記録した手元の保存先とちょうど一致するときだけ真。
/// 画面から任意のパスを開けないようにするための境界である。
pub(crate) fn is_known_local_path(connection: &Connection, path: &str) -> Result<bool, String> {
    if path.trim().is_empty() {
        return Ok(false);
    }
    let found: i64 = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM local_files WHERE path = ?1)
                 OR EXISTS(SELECT 1 FROM product_files WHERE local_path = ?1)",
            [path],
            |row| row.get(0),
        )
        .sql()?;
    Ok(found != 0)
}

/// File tab: titles for folder labels. Only what is already in the local DB; nothing is fetched.
/// Empty titles are left out, so those folders show their plain name.
pub(crate) fn product_labels(
    connection: &Connection,
    booth_ids: &[String],
) -> Result<HashMap<String, crate::file_browser::EntryLabel>, String> {
    let mut labels = HashMap::new();
    if booth_ids.is_empty() {
        return Ok(labels);
    }
    let mut statement = connection
        .prepare("SELECT title, IFNULL(is_adult, 0) FROM products WHERE booth_id = ?1")
        .sql()?;
    for booth_id in booth_ids {
        if labels.contains_key(booth_id) {
            continue;
        }
        let found = statement
            .query_row([booth_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
            })
            .optional()
            .sql()?;
        if let Some((title, adult)) = found {
            let title = title.trim();
            if !title.is_empty() {
                labels.insert(
                    booth_id.clone(),
                    crate::file_browser::EntryLabel {
                        title: title.to_string(),
                        adult,
                    },
                );
            }
        }
    }
    Ok(labels)
}

/// File tab: files whose product is known, as `(path, booth_id)`. Scan links
/// (`local_files.linked_product_id`) and recorded downloads (`product_files.local_path`).
pub(crate) fn linked_file_products(
    connection: &Connection,
) -> Result<Vec<(String, String)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT path, linked_product_id FROM local_files
              WHERE linked_product_id IS NOT NULL AND is_dir = 0
             UNION ALL
             SELECT local_path, booth_id FROM product_files
              WHERE local_path IS NOT NULL AND local_path <> ''",
        )
        .sql()?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .sql()?;
    rows.collect::<Result<Vec<_>, _>>().sql()
}

pub(crate) struct SweepReport {
    pub moved: u32,
    pub left: Vec<crate::model::StagedLeftover>,
}

pub(crate) fn route_staged_download(
    connection: &mut Connection,
    source: &Path,
) -> Result<(), String> {
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
    if crate::library_files::file_looks_like_login_page(&plan.source) {
        return Err(BROWSER_LOGIN_MESSAGE.into());
    }
    let placed = crate::library_files::place_into_library(
        &plan.source,
        plan.library.as_deref(),
        &plan.booth_id,
    )?;
    crate::extension_delivery::remember(connection, &plan.source, &plan.booth_id, &placed)?;
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

pub(super) fn booth_id_from_url(url: &str) -> Option<String> {
    let start = url.find("/items/")?;
    let rest = &url[start + "/items/".len()..];
    let id: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if crate::library_files::valid_booth_id(&id) {
        Some(id)
    } else {
        None
    }
}
