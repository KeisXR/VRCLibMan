use crate::{library_files, scan};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    path: String,
    filename: String,
    size: u64,
    status: String,
    booth_id: Option<String>,
    title: Option<String>,
    variation: Option<String>,
    original_filename: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview { entries: Vec<Candidate>, truncated: bool }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection { path: String, booth_id: String, variation: String, filename: String, size: u64 }

fn db_error(e: rusqlite::Error) -> String { format!("データベースを読めません: {e}") }

pub fn preview(connection: &Connection, root: &Path) -> Result<Preview, String> {
    let outcome = scan::scan_folder(root)?;
    let mut statement = connection.prepare(
        "SELECT f.booth_id, p.title, f.variation, f.filename, f.size_bytes, f.local_path FROM product_files f JOIN products p ON p.booth_id=f.booth_id WHERE f.filename != '' AND f.booth_id NOT IN (SELECT booth_id FROM product_exclusions)"
    ).map_err(db_error)?;
    let rows = statement.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,Option<String>>(5)?))).map_err(db_error)?
        .collect::<Result<Vec<_>,_>>().map_err(db_error)?;
    let mut entries = Vec::new();
    for entry in outcome.entries.into_iter().filter(|e| !e.is_dir) {
        let matched: Vec<_> = rows.iter().filter(|r| r.3 == entry.filename).collect();
        let mut c = Candidate { path: entry.path, filename: entry.filename, size: entry.size, status: "unmatched".into(), booth_id: None, title: None, variation: None, original_filename: None };
        if matched.len() == 1 {
            let r = matched[0];
            c.booth_id = Some(r.0.clone()); c.title = Some(r.1.clone()); c.variation = Some(r.2.clone()); c.original_filename = Some(r.3.clone());
            c.status = if r.5.as_ref().is_some_and(|p| Path::new(p).is_file()) { "registered" } else if r.4.is_some_and(|size| size < 0 || size as u64 != c.size) { "review" } else { "matched" }.into();
        } else if !matched.is_empty() { c.status = "review".into(); }
        entries.push(c);
    }
    Ok(Preview { entries, truncated: outcome.truncated })
}

pub fn register(connection: &mut Connection, root: &Path, selections: Vec<Selection>) -> Result<u32, String> {
    let current = preview(connection, root)?;
    let tx = connection.transaction().map_err(db_error)?;
    let mut count = 0;
    let mut targets = std::collections::HashSet::new();
    for s in selections {
        if !targets.insert((s.booth_id.clone(), s.variation.clone(), s.filename.clone())) {
            return Err("同じ商品ファイルに複数のファイルが選択されています。1件だけ選んでください".into());
        }
        let Some(found) = current.entries.iter().find(|e| e.path == s.path && e.status == "matched" && e.size == s.size && e.booth_id.as_deref() == Some(s.booth_id.as_str()) && e.variation.as_deref() == Some(s.variation.as_str()) && e.original_filename.as_deref() == Some(s.filename.as_str())) else { return Err("ファイルまたは候補が変わりました。もう一度調べてください".into()); };
        let meta = std::fs::symlink_metadata(&found.path).map_err(|_| "ファイルを確認できません。もう一度調べてください")?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != found.size { return Err("ファイルが変更されました。もう一度調べてください".into()); }
        let path = Path::new(&found.path).canonicalize().map_err(|_| "ファイルを確認できません")?;
        let canonical_root = root.canonicalize().map_err(|_| "フォルダを確認できません")?;
        if !path.starts_with(canonical_root) { return Err("フォルダの外にあるファイルは登録できません".into()); }
        let path = library_files::without_verbatim_prefix(&path).display().to_string();
        let already_linked: Option<String> = tx.query_row(
            "SELECT linked_product_id FROM local_files WHERE path=?1 AND linked_product_id IS NOT NULL", [&path], |r| r.get(0)
        ).optional().map_err(db_error)?;
        if already_linked.as_deref().is_some_and(|id| id != s.booth_id.as_str()) {
            return Err("別の商品に登録されたファイルが含まれています".into());
        }
        let changed = tx.execute("UPDATE product_files SET downloaded=1, local_path=?1 WHERE booth_id=?2 AND variation=?3 AND filename=?4", params![path, s.booth_id, s.variation, s.filename]).map_err(db_error)?;
        if changed != 1 { return Err("商品情報が変更されました。もう一度調べてください".into()); }
        tx.execute("INSERT INTO local_files (path, filename, is_dir, size, linked_product_id, candidate_product_id, match_score, link_source, scanned_at) VALUES (?1, ?2, 0, ?3, ?4, ?4, 1, 'manual', datetime('now')) ON CONFLICT(path) DO UPDATE SET linked_product_id=excluded.linked_product_id, candidate_product_id=excluded.candidate_product_id, match_score=1, link_source='manual'", params![path, found.filename, found.size as i64, s.booth_id]).map_err(db_error)?;
        count += 1;
    }
    tx.commit().map_err(db_error)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let connection = crate::db::open(&dir.path().join("test.sqlite")).unwrap();
        connection.execute_batch("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES ('123456','衣装','t','t'); INSERT INTO product_files(booth_id,variation,filename,size_bytes) VALUES ('123456','','outfit_v1.zip',4);").unwrap();
        fs::write(dir.path().join("outfit_v1.zip"), b"test").unwrap();
        (dir, connection)
    }
    fn selection(path: &Path) -> Selection {
        Selection { path: path.join("outfit_v1.zip").display().to_string(), booth_id: "123456".into(), variation: "".into(), filename: "outfit_v1.zip".into(), size: 4 }
    }
    #[test]
    fn registers_exact_match_without_moving_file() {
        let (dir, mut db) = fixture();
        assert_eq!(preview(&db, dir.path()).unwrap().entries[0].status, "matched");
        assert_eq!(register(&mut db, dir.path(), vec![selection(dir.path())]).unwrap(), 1);
        assert!(dir.path().join("outfit_v1.zip").is_file());
        assert_eq!(preview(&db, dir.path()).unwrap().entries[0].status, "registered");
        let result: (i64, String) = db.query_row("SELECT downloaded, local_path FROM product_files WHERE booth_id='123456'", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(result.0, 1);
        assert!(Path::new(&result.1).is_file());
    }
    #[test]
    fn size_mismatch_and_ambiguous_names_cannot_be_registered() {
        let (dir, mut db) = fixture();
        fs::write(dir.path().join("outfit_v1.zip"), b"changed").unwrap();
        assert_eq!(preview(&db, dir.path()).unwrap().entries[0].status, "review");
        assert!(register(&mut db, dir.path(), vec![selection(dir.path())]).is_err());
        fs::write(dir.path().join("outfit_v1.zip"), b"test").unwrap();
        db.execute_batch("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES ('123457','別の商品','t','t'); INSERT INTO product_files(booth_id,filename) VALUES ('123457','outfit_v1.zip');").unwrap();
        assert_eq!(preview(&db, dir.path()).unwrap().entries[0].status, "review");
        assert!(register(&mut db, dir.path(), vec![selection(dir.path())]).is_err());
    }
}
