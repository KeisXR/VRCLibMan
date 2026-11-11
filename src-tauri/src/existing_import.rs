use crate::{db, library_files, scan, AppState};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::{HashMap, HashSet}, path::{Component, Path, PathBuf}};
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    booth_id: String, title: String, variation: String, filename: String,
    size_bytes: Option<u64>, registered: bool,
    #[serde(skip)] local_path: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry { path: String, filename: String, size: u64, choices: Vec<Choice>, status: String }
#[derive(Serialize)]
pub struct Preview { entries: Vec<Entry>, truncated: bool }
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection { path: String, booth_id: String, variation: String, filename: String, size: u64 }
#[derive(Serialize, Default)]
pub struct Report { registered: usize, errors: Vec<String> }
fn sql(e: rusqlite::Error) -> String { format!("登録情報を処理できません: {e}") }

pub fn catalog(c: &Connection) -> Result<Vec<Choice>, String> {
    let mut stmt = c.prepare("SELECT f.booth_id,p.title,f.variation,f.filename,f.size_bytes,f.local_path FROM product_files f JOIN products p ON p.booth_id=f.booth_id WHERE f.filename!='' AND f.booth_id NOT IN (SELECT booth_id FROM product_exclusions)").map_err(sql)?;
    let rows = stmt.query_map([], |r| {
        let size: Option<i64> = r.get(4)?;
        Ok(Choice { booth_id:r.get(0)?,title:r.get(1)?,variation:r.get(2)?,filename:r.get(3)?,size_bytes:size.and_then(|v|u64::try_from(v).ok()),local_path:r.get(5)?,registered:false })
    }).map_err(sql)?.collect::<Result<Vec<_>,_>>().map_err(sql)?;
    Ok(rows)
}
// Only remove the browser's space + parenthesized numeric suffix. Version text stays intact.
fn browser_name(name: &str) -> Option<String> {
    let path = Path::new(name);
    let stem = path.file_stem()?.to_str()?;
    let (base, suffix) = stem.rsplit_once(" (")?;
    let digits = suffix.strip_suffix(')')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) { return None; }
    Some(format!("{}.{}", base, path.extension()?.to_str()?))
}
pub fn preview(rows: Vec<Choice>, root: &Path) -> Result<Preview, String> {
    let root = checked_root(root)?;
    let outcome = scan::scan_import_packages(&root)?;
    let mut names: HashMap<String, Vec<Choice>> = HashMap::new();
    for mut row in rows {
        row.registered = row.local_path.is_some();
        names.entry(row.filename.clone()).or_default().push(row);
    }
    let mut entries = Vec::new();
    for e in outcome.entries.into_iter().filter(|e| !e.is_dir) {
        let exact = names.get(&e.filename);
        let choices = exact.or_else(|| browser_name(&e.filename).and_then(|n|names.get(&n))).cloned().unwrap_or_default();
        let status = if choices.is_empty() { "unmatched" } else if choices.iter().all(|c|c.registered) { "registered" } else if exact.is_some() && choices.len()==1 && compatible(&choices[0],e.size) { "matched" } else { "review" };
        entries.push(Entry {path:e.path,filename:e.filename,size:e.size,choices,status:status.into()});
    }
    entries.sort_by(|a,b|a.path.cmp(&b.path));
    Ok(Preview { entries, truncated:outcome.truncated })
}
fn compatible(c: &Choice, size: u64) -> bool { !c.registered && c.size_bytes.is_none_or(|n|n==size) }
fn checked_root(root: &Path) -> Result<PathBuf,String> {
    if !root.is_absolute() || root.components().any(|c|matches!(c,Component::ParentDir)) { return Err("親ディレクトリを含まない絶対パスを選んでください".into()); }
    scan::validate_scan_root(root)?;
    root.canonicalize().map_err(|_|"フォルダを確認できません".into())
}
fn source(root: &Path, s: &Selection) -> Result<PathBuf,String> {
    let path=Path::new(&s.path);
    if !scan::is_import_package(path) || !scan::is_import_package(Path::new(&s.filename)) {
        return Err("取り込める形式は ZIP と Unitypackage だけです".into());
    }
    let relative=path.strip_prefix(root).map_err(|_|"取り込み元の外にあるファイルです")?;
    let mut current=root.to_path_buf();
    for part in relative.components() {
        if !matches!(part,Component::Normal(_)) { return Err("ファイルの場所が不正です".into()); }
        current.push(part);
        if std::fs::symlink_metadata(&current).map_err(|_|"ファイルを確認できません")?.file_type().is_symlink() { return Err("リンクは取り込めません".into()); }
    }
    let meta=std::fs::metadata(&current).map_err(|_|"ファイルを確認できません")?;
    if !meta.is_file() || meta.len()!=s.size { return Err("ファイルが変更されました。再度調べてください".into()); }
    let disk_name=current.file_name().and_then(|v|v.to_str()).unwrap_or("");
    if disk_name!=s.filename && browser_name(disk_name).as_deref()!=Some(s.filename.as_str()) { return Err("配布ファイル名と一致しません".into()); }
    Ok(current)
}
fn validate(rows: &[Choice], s: &Selection) -> Result<(),String> {
    if !rows.iter().any(|c|c.booth_id==s.booth_id && c.variation==s.variation && c.filename==s.filename && c.size_bytes.is_none_or(|n|n==s.size)) { return Err("候補またはサイズが変わりました。再度調べてください".into()); }
    Ok(())
}
fn record(c: &mut Connection,s: &Selection,path: &Path) -> Result<(),String> {
    let tx=c.transaction().map_err(sql)?;
    validate(&catalog(&tx)?,s)?;
    let path=library_files::without_verbatim_prefix(path).display().to_string();
    let linked: Option<(Option<String>, Option<String>)>=tx.query_row("SELECT linked_product_id, link_source FROM local_files WHERE path=?1",[&path],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
    if linked.is_some_and(|(id, source)|id.is_some_and(|id|id!=s.booth_id) && source.as_deref()!=Some("auto")) { return Err("別の商品への登録を保持しました".into()); }
    let existing: Option<String>=tx.query_row("SELECT local_path FROM product_files WHERE booth_id=?1 AND variation=?2 AND filename=?3",params![s.booth_id,s.variation,s.filename],|r|r.get(0)).map_err(sql)?;
    // Do not replace a different registered path, even when its disk is temporarily unavailable.
    if existing.as_deref().is_some_and(|p|p!=path) { return Err("登録済みの保存先を保持しました".into()); }
    tx.execute("UPDATE product_files SET downloaded=1,local_path=?1 WHERE booth_id=?2 AND variation=?3 AND filename=?4",params![path,s.booth_id,s.variation,s.filename]).map_err(sql)?;
    let filename=Path::new(&path).file_name().and_then(|n|n.to_str()).ok_or("ファイル名を確認できません")?;
    tx.execute("INSERT INTO local_files(path,filename,size,linked_product_id,candidate_product_id,match_score,link_source,scanned_at) VALUES(?1,?2,?3,?4,?4,1,'manual',datetime('now')) ON CONFLICT(path) DO UPDATE SET filename=excluded.filename,size=excluded.size,linked_product_id=excluded.linked_product_id,candidate_product_id=excluded.candidate_product_id,match_score=1,link_source='manual'",params![path,filename,s.size as i64,s.booth_id]).map_err(sql)?;
    crate::sync_meta::touch_file_links(&tx,&s.booth_id)?;
    tx.commit().map_err(sql)
}
pub fn register(app: &AppHandle,root: &Path,selections: Vec<Selection>,copy: bool) -> Result<Report,String> {
    static BUSY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _busy = BUSY.try_lock().map_err(|_| "別の取り込みを実行中です")?;
    let root=checked_root(root)?;
    let state=app.state::<AppState>();
    let library={let c=state.db.lock().map_err(|_|"登録情報を読めません")?; db::settings(&c)?.library_folder};
    if copy && library.is_empty() { return Err("設定でライブラリの保存先を選んでください".into()); }
    let mut report=Report::default(); let mut targets=HashSet::new(); let mut paths=HashSet::new();
    for s in selections {
        let result=(|| {
            if !targets.insert((s.booth_id.clone(),s.variation.clone(),s.filename.clone())) || !paths.insert(s.path.clone()) { return Err("同じ商品ファイルには1件だけ選択してください".into()); }
            let source=source(&root,&s)?;
            {let c=state.db.lock().map_err(|_|"登録情報を読めません")?;
                let rows=catalog(&c)?;
                validate(&rows,&s)?;
                if rows.iter().any(|row|row.booth_id==s.booth_id && row.variation==s.variation && row.filename==s.filename && row.local_path.is_some()) {
                    return Err("登録済みの保存先を保持しました".into());
                }
            }
            let dest=if copy {
                // Existing helper writes through a temporary file and never overwrites.
                if library_files::push_missing_file(&source,Path::new(&library),&s.booth_id,&s.filename)? != library_files::CopyOutcome::Copied { return Err("コピー先に同名のファイルがあります。既存ファイルは保持しました".into()); }
                library_files::library_destination(Path::new(&library),&s.booth_id,&s.filename)?
            } else {source};
            let mut c=state.db.lock().map_err(|_|"登録情報を読めません")?;
            if copy && db::settings(&c)?.library_folder!=library { return Err("保存先が変更されました。コピー済みファイルを確認してください".into()); }
            record(&mut c,&s,&dest)
        })();
        match result {Ok(())=>report.registered+=1,Err(e)=>report.errors.push(format!("{}: {e}",s.filename))}
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir,Connection) {
        let dir=tempfile::tempdir().unwrap();
        let c=db::open(&dir.path().join("test.sqlite")).unwrap();
        c.execute_batch("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES('123456','衣装','t','t'); INSERT INTO product_files(booth_id,filename,size_bytes) VALUES('123456','outfit_v1.zip',4);").unwrap();
        std::fs::write(dir.path().join("outfit_v1.zip"),b"test").unwrap();
        (dir,c)
    }
    fn selection(root: &Path) -> Selection {Selection{path:root.join("outfit_v1.zip").display().to_string(),booth_id:"123456".into(),variation:"".into(),filename:"outfit_v1.zip".into(),size:4}}
    #[test]
    fn non_packages_cannot_be_submitted_directly() {
        let (dir, _c) = fixture();
        for name in ["texture.psd", "model.fbx", "archive.7z", "archive.rar", "body.vrm", "pack.zip.part"] {
            std::fs::write(dir.path().join(name), b"test").unwrap();
            let mut s = selection(dir.path());
            s.path = dir.path().join(name).display().to_string();
            s.filename = name.into();
            assert!(source(dir.path(), &s).is_err());
        }
    }

    #[test]
    fn versions_are_preserved_and_browser_suffix_is_only_a_candidate() {
        assert_eq!(browser_name("outfit_v1 (2).zip").as_deref(),Some("outfit_v1.zip"));
        assert_eq!(browser_name("outfit_v1.zip"),None);
        let (dir,c)=fixture();
        std::fs::rename(dir.path().join("outfit_v1.zip"),dir.path().join("outfit_v1 (2).zip")).unwrap();
        let p=preview(catalog(&c).unwrap(),dir.path()).unwrap();
        assert_eq!(p.entries[0].status,"review");
        assert_eq!(p.entries[0].choices.len(),1);
    }
    #[test]
    fn ambiguous_names_require_a_choice_and_size_mismatch_is_rejected() {
        let (dir,c)=fixture();
        c.execute_batch("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES('123457','別の商品','t','t'); INSERT INTO product_files(booth_id,filename) VALUES('123457','outfit_v1.zip');").unwrap();
        let p=preview(catalog(&c).unwrap(),dir.path()).unwrap();
        assert_eq!(p.entries[0].status,"review"); assert_eq!(p.entries[0].choices.len(),2);
        assert!(validate(&catalog(&c).unwrap(),&selection(dir.path())).is_ok());
        let mut s=selection(dir.path()); s.size=5;
        assert!(validate(&catalog(&c).unwrap(),&s).is_err());
    }
    #[test]
    fn registration_preserves_source_and_manual_links() {
        let (dir,mut c)=fixture(); let s=selection(dir.path());
        let p=source(dir.path(),&s).unwrap();
        record(&mut c,&s,&p).unwrap();
        assert!(p.is_file());
        assert_eq!(preview(catalog(&c).unwrap(),dir.path()).unwrap().entries[0].status,"registered");
        c.execute_batch("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES('123457','別の商品','t','t'); UPDATE local_files SET linked_product_id='123457';").unwrap();
        assert!(record(&mut c,&s,&p).is_err());
    }
    #[test]
    fn explicit_registration_replaces_an_automatic_link() {
        let (dir,mut c)=fixture(); let s=selection(dir.path());
        let p=source(dir.path(),&s).unwrap();
        c.execute_batch("INSERT INTO products(booth_id,title,imported_at,updated_at) VALUES('123457','別の商品','t','t');").unwrap();
        c.execute("INSERT INTO local_files(path,filename,linked_product_id,link_source,scanned_at) VALUES(?1,'outfit_v1.zip','123457','auto','t')",[p.display().to_string()]).unwrap();
        record(&mut c,&s,&p).unwrap();
        let link: (String,String)=c.query_row("SELECT linked_product_id,link_source FROM local_files WHERE path=?1",[p.display().to_string()],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(link, ("123456".into(),"manual".into()));
    }
    #[test]
    fn copy_never_overwrites_and_source_stays_present() {
        let (dir,_c)=fixture(); let library=tempfile::tempdir().unwrap(); let s=selection(dir.path());
        let path=source(dir.path(),&s).unwrap();
        assert_eq!(library_files::push_missing_file(&path,library.path(),&s.booth_id,&s.filename).unwrap(), library_files::CopyOutcome::Copied);
        let dest=library_files::library_destination(library.path(),&s.booth_id,&s.filename).unwrap();
        std::fs::write(&dest,b"existing").unwrap();
        assert_ne!(library_files::push_missing_file(&path,library.path(),&s.booth_id,&s.filename).unwrap(), library_files::CopyOutcome::Copied);
        assert_eq!(std::fs::read(dest).unwrap(),b"existing"); assert!(path.is_file());
    }
    #[test]
    fn excluded_products_and_changed_source_are_rejected() {
        let (dir,c)=fixture();
        c.execute_batch("INSERT INTO product_exclusions(booth_id,excluded_at) VALUES('123456','t');").unwrap();
        assert!(catalog(&c).unwrap().is_empty());
        let s=selection(dir.path()); std::fs::write(&s.path,b"changed").unwrap();
        assert!(source(dir.path(),&s).is_err());
    }
}
