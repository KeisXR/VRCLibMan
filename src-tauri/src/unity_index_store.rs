//! 導入済みの判定の索引（`unity-index.sqlite`）。docs/unity-projects.md の段階 2。
//!
//! - `library.sqlite` とは別のファイルで、消しても作り直せる写し。データ移行に含めない。
//!   版が合わなければ消して作り直す（移行を書かない）。
//! - ライブラリの unitypackage（zip の中を含む）を 1 件ずつ読み、GUID を残す。読むのは専用のワーカー 1 本か、
//!   プロジェクトを調べるコマンドで、どちらも `PASS` のロックを取ってから行う。
//! - プロジェクトの GUID の集合は残さない。パッケージごとの一致数だけを残す。

use crate::unity_index::{
    self, Counts, Guid, MAX_PROJECT_FILES, PROJECT_TIMEOUT, PackageIndex, STABLE_EXTENSIONS,
    Verdict,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

pub(crate) const FILE_NAME: &str = "unity-index.sqlite";
/// 表の形を変えたら上げる。合わない索引は消して作り直す。
pub(crate) const INDEX_VERSION: i64 = 1;
/// 共有のファイルの大きさを確かめる時間の上限。
const STAT_TIMEOUT: Duration = Duration::from_secs(2);

/// 索引を書く仕事は一度に 1 つ（ワーカーとコマンドが同じ索引を書く）。
pub(crate) static PASS: Mutex<()> = Mutex::new(());

const TABLES: &str = "
CREATE TABLE IF NOT EXISTS packages (
    id INTEGER PRIMARY KEY,
    source_path TEXT NOT NULL,
    entry TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified INTEGER NOT NULL,
    booth_id TEXT,
    file_count INTEGER NOT NULL,
    complete INTEGER NOT NULL,
    failed INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source_path, entry)
);
CREATE TABLE IF NOT EXISTS package_guids (
    guid BLOB NOT NULL,
    package_id INTEGER NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
    asset_size INTEGER,
    ext TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (guid, package_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS package_guids_by_package ON package_guids(package_id);
CREATE TABLE IF NOT EXISTS package_refs (
    package_id INTEGER NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
    guid BLOB NOT NULL,
    PRIMARY KEY (package_id, guid)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS project_packages (
    project_id TEXT NOT NULL,
    package_id INTEGER NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
    unique_total INTEGER NOT NULL,
    unique_matched INTEGER NOT NULL,
    vpm_owned INTEGER NOT NULL,
    vpm_owner TEXT,
    matched INTEGER NOT NULL,
    stable_matched INTEGER NOT NULL,
    stable_same_size INTEGER NOT NULL,
    PRIMARY KEY (project_id, package_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS project_scans (
    project_id TEXT PRIMARY KEY,
    scanned_at INTEGER NOT NULL,
    complete INTEGER NOT NULL,
    stale INTEGER NOT NULL DEFAULT 0,
    meta_files INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS guid_owners (
    guid BLOB PRIMARY KEY,
    package_name TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS vpm_scans (
    package_name TEXT NOT NULL,
    version TEXT NOT NULL,
    display_name TEXT,
    PRIMARY KEY (package_name, version)
) WITHOUT ROWID;
";

fn sql<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|err| format!("Unity の索引を読み書きできません: {err}"))
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn prepare(connection: &Connection) -> Result<(), String> {
    sql(connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;"))?;
    // 読むだけの呼び出し（プレビューの行など）でも開くので、表と版がそろっていれば何も書かない。
    // 調べている最中の索引に、開くたびに書き込みのロックを取りに行かないため。
    let version: i64 = sql(connection.query_row("PRAGMA user_version", [], |row| row.get(0)))?;
    if version == INDEX_VERSION {
        return Ok(());
    }
    sql(connection.execute_batch(TABLES))?;
    sql(connection.execute_batch(&format!("PRAGMA user_version = {INDEX_VERSION};")))
}

/// 索引を開く。版が合わない・壊れているときは消して作り直す。
pub(crate) fn open(path: &Path) -> Result<Connection, String> {
    let usable = Connection::open(path).ok().and_then(|connection| {
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .ok()?;
        (version == INDEX_VERSION || version == 0).then_some(connection)
    });
    let connection = match usable {
        Some(connection) => connection,
        None => {
            let _ = std::fs::remove_file(path);
            sql(Connection::open(path))?
        }
    };
    prepare(&connection)?;
    Ok(connection)
}

#[cfg(test)]
pub(crate) fn open_memory() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    prepare(&connection).unwrap();
    connection
}

/// 索引にするライブラリのファイル 1 つ。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Source {
    pub booth_id: String,
    pub path: PathBuf,
}

/// 大きさと更新日時（秒）。届かない共有では `None`。
fn stat(path: &Path) -> Option<(i64, i64)> {
    let owned = path.to_path_buf();
    crate::library_files::wait_for(STAT_TIMEOUT, move || {
        let meta = std::fs::metadata(&owned).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        Some((meta.len() as i64, modified))
    })
    .flatten()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// 読み直しが要るファイル。大きさか更新日時が変わったもの、まだ読んでいないもの。
pub(crate) fn pending_sources(
    connection: &Connection,
    sources: &[Source],
) -> Result<Vec<(Source, i64, i64)>, String> {
    let mut statement =
        sql(connection
            .prepare("SELECT size, modified FROM packages WHERE source_path = ?1 LIMIT 1"))?;
    let mut pending = Vec::new();
    for source in sources {
        let Some((size, modified)) = stat(&source.path) else {
            continue;
        };
        let known: Option<(i64, i64)> = sql(statement
            .query_row(params![path_text(&source.path)], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional())?;
        if known != Some((size, modified)) {
            pending.push((source.clone(), size, modified));
        }
    }
    Ok(pending)
}

/// 読んだ結果を書く。同じファイルの前の行は消して入れ直す。
pub(crate) fn store_source(
    connection: &mut Connection,
    source: &Source,
    size: i64,
    modified: i64,
    read: Result<Vec<unity_index::ReadPackage>, String>,
) -> Result<(), String> {
    let tx = sql(connection.transaction())?;
    let path = path_text(&source.path);
    sql(tx.execute("DELETE FROM packages WHERE source_path = ?1", params![path]))?;
    let packages = match read {
        Ok(packages) if !packages.is_empty() => packages,
        // zip に unitypackage が無い、または読めない。読み直さないよう、印だけ残す。
        other => {
            sql(tx.execute(
                "INSERT INTO packages (source_path, entry, size, modified, booth_id, file_count, complete, failed)
                 VALUES (?1, '', ?2, ?3, ?4, 0, 1, ?5)",
                params![path, size, modified, source.booth_id, i64::from(other.is_err())],
            ))?;
            return sql(tx.commit());
        }
    };
    for package in packages {
        let (index, failed) = match package.index {
            Ok(index) => (index, false),
            Err(_) => (PackageIndex::default(), true),
        };
        sql(tx.execute(
            "INSERT INTO packages (source_path, entry, size, modified, booth_id, file_count, complete, failed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                path,
                package.entry,
                size,
                modified,
                source.booth_id,
                index.file_count() as i64,
                i64::from(index.complete && !failed),
                i64::from(failed)
            ],
        ))?;
        let id = tx.last_insert_rowid();
        {
            let mut insert = sql(tx.prepare(
                "INSERT OR IGNORE INTO package_guids (guid, package_id, asset_size, ext) VALUES (?1, ?2, ?3, ?4)",
            ))?;
            for asset in &index.assets {
                sql(insert.execute(params![
                    asset.guid.as_slice(),
                    id,
                    asset.size.map(|size| size as i64),
                    asset.ext
                ]))?;
            }
            let mut refs = sql(
                tx.prepare("INSERT OR IGNORE INTO package_refs (package_id, guid) VALUES (?1, ?2)")
            )?;
            for guid in &index.refs {
                sql(refs.execute(params![id, guid.as_slice()]))?;
            }
        }
    }
    sql(tx.commit())
}

/// ライブラリの登録から消えたファイルの行を消す。`sources` にあるファイルは、届かなくても残す。
pub(crate) fn forget_missing(connection: &Connection, sources: &[Source]) -> Result<usize, String> {
    let keep: HashSet<String> = sources
        .iter()
        .map(|source| path_text(&source.path))
        .collect();
    let mut statement = sql(connection.prepare("SELECT DISTINCT source_path FROM packages"))?;
    let known: Vec<String> =
        sql(sql(statement.query_map([], |row| row.get(0)))?.collect::<Result<Vec<_>, _>>())?;
    let mut removed = 0;
    for path in known.into_iter().filter(|path| !keep.contains(path)) {
        removed +=
            sql(connection.execute("DELETE FROM packages WHERE source_path = ?1", params![path]))?;
    }
    Ok(removed)
}

/// 索引が変わったら、前に調べた結果は古いとする。VPM パッケージの持ち主（`guid_owners`）は
/// パッケージの GUID をすべて覚えているので、覚え直さない。
pub(crate) fn mark_index_changed(connection: &Connection) -> Result<(), String> {
    sql(connection.execute_batch("UPDATE project_scans SET stale = 1;"))
}

/// 読み直しが要るファイルを順に読み、索引を新しくする。読んだ数を返す。
/// `sources` はライブラリに登録されているファイルのすべて。届かないファイル（切れた共有）は読まず、
/// 索引からも消さない。索引から消すのは、`sources` に無くなったファイルだけ。
/// `progress` は（読んだ数、全体）で呼ぶ。`stop` が真になったら次のファイルの前で止める。
pub(crate) fn refresh(
    connection: &mut Connection,
    sources: &[Source],
    mut progress: impl FnMut(usize, usize),
    stop: impl Fn() -> bool,
) -> Result<usize, String> {
    let pending = pending_sources(connection, sources)?;
    let total = pending.len();
    let mut read = 0;
    for (source, size, modified) in pending {
        if stop() {
            break;
        }
        progress(read, total);
        let result = unity_index::read_source(&source.path);
        store_source(connection, &source, size, modified, result)?;
        read += 1;
    }
    let removed = forget_missing(connection, sources)?;
    if read > 0 || removed > 0 {
        mark_index_changed(connection)?;
    }
    Ok(read)
}

/// VPM パッケージの名前として歩いてよい形（`jp.lilxyzw.liltoon` など）。
fn vpm_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 214
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

struct IndexedGuid {
    package_id: i64,
    size: Option<i64>,
    ext: String,
}

/// プロジェクトを調べ、パッケージごとの一致数を残す。
pub(crate) fn scan_project(
    connection: &mut Connection,
    project_id: &str,
    project_path: &Path,
    vpm_packages: &[(String, String)],
) -> Result<(), String> {
    // 索引を読む。
    let mut guids: HashMap<Guid, Vec<IndexedGuid>> = HashMap::new();
    let mut package_booth: HashMap<i64, Option<String>> = HashMap::new();
    {
        let mut statement =
            sql(connection.prepare("SELECT id, booth_id FROM packages WHERE failed = 0"))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
        }))?;
        for row in rows {
            let (id, booth) = sql(row)?;
            package_booth.insert(id, booth);
        }
        let mut statement =
            sql(connection.prepare("SELECT guid, package_id, asset_size, ext FROM package_guids"))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, String>(3)?,
            ))
        }))?;
        for row in rows {
            let (guid, package_id, size, ext) = sql(row)?;
            let Ok(guid) = <Guid>::try_from(guid.as_slice()) else {
                continue;
            };
            if package_booth.contains_key(&package_id) {
                guids.entry(guid).or_default().push(IndexedGuid {
                    package_id,
                    size,
                    ext,
                });
            }
        }
    }

    // VPM パッケージは名前と版ごとに 1 回だけ歩き、その GUID をすべて覚える（1 プロジェクトで数千件）。
    // 索引にある GUID だけを覚えると、ライブラリにファイルが増えるたびに歩き直しになるため。
    for (name, version) in vpm_packages {
        let done: bool = sql(connection
            .query_row(
                "SELECT 1 FROM vpm_scans WHERE package_name = ?1 AND version = ?2",
                params![name, version],
                |_| Ok(()),
            )
            .optional())?
        .is_some();
        // 名前は vpm-manifest.json から来る。フォルダ名として安全な形だけを歩く
        // （区切り・`..`・ドライブ名の `C:` などで Packages の外へ出ない）。
        if done || !vpm_folder_name(name) {
            continue;
        }
        let folder = project_path.join("Packages").join(name);
        if !folder.is_dir() {
            continue;
        }
        let scan =
            unity_index::scan_metas(&folder, Instant::now() + PROJECT_TIMEOUT, MAX_PROJECT_FILES);
        let display = std::fs::read_to_string(folder.join("package.json"))
            .ok()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
            .and_then(|value| {
                value
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });
        let tx = sql(connection.transaction())?;
        {
            let mut insert = sql(tx.prepare(
                "INSERT OR IGNORE INTO guid_owners (guid, package_name) VALUES (?1, ?2)",
            ))?;
            for guid in scan.found.keys() {
                sql(insert.execute(params![guid.as_slice(), name]))?;
            }
        }
        if scan.complete {
            sql(tx.execute(
                "INSERT OR REPLACE INTO vpm_scans (package_name, version, display_name) VALUES (?1, ?2, ?3)",
                params![name, version, display],
            ))?;
        }
        sql(tx.commit())?;
    }
    let owners: HashMap<Guid, String> = {
        let mut statement = sql(connection.prepare("SELECT guid, package_name FROM guid_owners"))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?))
        }))?;
        let mut map = HashMap::new();
        for row in rows {
            let (guid, name) = sql(row)?;
            // 使うのは、ライブラリのパッケージにある GUID の分だけ。
            if let Ok(guid) = <Guid>::try_from(guid.as_slice())
                && guids.contains_key(&guid)
            {
                map.insert(guid, name);
            }
        }
        map
    };

    // プロジェクトの Assets を歩く。
    let scan = unity_index::scan_metas(
        &project_path.join("Assets"),
        Instant::now() + PROJECT_TIMEOUT,
        MAX_PROJECT_FILES,
    );

    #[derive(Default)]
    struct Tally {
        unique: usize,
        unique_matched: usize,
        matched: usize,
        stable_matched: usize,
        stable_same_size: usize,
        owners: HashMap<String, usize>,
    }
    let mut tallies: HashMap<i64, Tally> = package_booth
        .keys()
        .map(|id| (*id, Tally::default()))
        .collect();
    for (guid, holders) in &guids {
        let booths: HashSet<&Option<String>> = holders
            .iter()
            .map(|holder| &package_booth[&holder.package_id])
            .collect();
        let owner = owners.get(guid);
        // VPM パッケージが持っているファイル（商品に少しだけ同梱されたシェーダーなど）は、
        // VPM 版を入れたプロジェクトでは Assets に無いので、固有のファイルに数えない。
        let unique = booths.len() == 1 && owner.is_none();
        let found = scan.found.get(guid);
        for holder in holders.iter().filter(|holder| holder.size.is_some()) {
            let tally = tallies.entry(holder.package_id).or_default();
            if let Some(owner) = owner {
                *tally.owners.entry(owner.clone()).or_default() += 1;
            }
            if unique {
                tally.unique += 1;
            }
            let Some(path) = found else { continue };
            tally.matched += 1;
            if unique {
                tally.unique_matched += 1;
            }
            if STABLE_EXTENSIONS.contains(&holder.ext.as_str()) {
                tally.stable_matched += 1;
                let on_disk = std::fs::metadata(path).map(|meta| meta.len() as i64).ok();
                if on_disk.is_some() && on_disk == holder.size {
                    tally.stable_same_size += 1;
                }
            }
        }
    }

    let tx = sql(connection.transaction())?;
    sql(tx.execute(
        "DELETE FROM project_packages WHERE project_id = ?1",
        params![project_id],
    ))?;
    {
        let mut insert = sql(tx.prepare(
            "INSERT INTO project_packages (project_id, package_id, unique_total, unique_matched, vpm_owned, vpm_owner,
             matched, stable_matched, stable_same_size) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        ))?;
        for (package_id, tally) in &tallies {
            let owner = tally
                .owners
                .iter()
                .max_by_key(|(name, count)| (**count, std::cmp::Reverse((*name).clone())));
            sql(insert.execute(params![
                project_id,
                package_id,
                tally.unique as i64,
                tally.unique_matched as i64,
                owner.map(|(_, count)| *count as i64).unwrap_or(0),
                owner.map(|(name, _)| name.clone()),
                tally.matched as i64,
                tally.stable_matched as i64,
                tally.stable_same_size as i64
            ]))?;
        }
    }
    sql(tx.execute(
        "INSERT OR REPLACE INTO project_scans (project_id, scanned_at, complete, stale, meta_files) VALUES (?1, ?2, ?3, 0, ?4)",
        params![project_id, now_secs(), i64::from(scan.complete), scan.files as i64],
    ))?;
    sql(tx.commit())
}

/// 調べた結果の、パッケージ 1 つ。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackageInstall {
    /// zip の中のパスか、単体のファイル名。
    pub name: String,
    pub source_path: String,
    pub verdict: Verdict,
    pub unique_total: usize,
    pub unique_matched: usize,
    /// 同梱版のとき、その持ち主の VPM パッケージ。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled_with: Option<String>,
    /// 同じ商品の導入済みのパッケージが 2 つ以上あるとき、大きさがいちばん合うもの（入っている版の見込み）。
    pub closest: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProductInstall {
    pub booth_id: String,
    pub verdict: Verdict,
    pub packages: Vec<PackageInstall>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectContents {
    /// 調べた日時（UNIX 秒）。まだ調べていなければ `None`。
    pub scanned_at: Option<i64>,
    /// `.meta` を最後まで歩けた。途中までのときは「入っていない」と言わない。
    pub complete: bool,
    /// 調べたあとに索引が変わった・Unity に渡した。
    pub stale: bool,
    pub products: Vec<ProductInstall>,
}

fn file_label(source_path: &str, entry: &str) -> String {
    let text = if entry.is_empty() { source_path } else { entry };
    text.rsplit(['/', '\\']).next().unwrap_or(text).to_string()
}

/// 前に調べた結果を読む。調べ直しはしない。
pub(crate) fn project_contents(
    connection: &Connection,
    project_id: &str,
) -> Result<ProjectContents, String> {
    let scan: Option<(i64, bool, bool)> = sql(connection
        .query_row(
            "SELECT scanned_at, complete, stale FROM project_scans WHERE project_id = ?1",
            params![project_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, i64>(2)? != 0,
                ))
            },
        )
        .optional())?;
    let Some((scanned_at, complete, stale)) = scan else {
        return Ok(ProjectContents {
            scanned_at: None,
            complete: false,
            stale: false,
            products: Vec::new(),
        });
    };
    let mut statement = sql(connection.prepare(
        "SELECT p.booth_id, p.source_path, p.entry, p.file_count, r.unique_total, r.unique_matched,
                r.vpm_owned, r.vpm_owner, r.stable_matched, r.stable_same_size
         FROM project_packages r JOIN packages p ON p.id = r.package_id
         WHERE r.project_id = ?1 AND p.booth_id IS NOT NULL AND p.failed = 0 AND p.complete = 1
           AND p.file_count > 0
         ORDER BY p.booth_id, p.source_path, p.entry",
    ))?;
    struct Row {
        booth_id: String,
        install: PackageInstall,
        stable_ratio: f64,
    }
    let rows: Vec<Row> = sql(sql(statement.query_map(params![project_id], |row| {
        let files: i64 = row.get(3)?;
        let unique_total: i64 = row.get(4)?;
        let unique_matched: i64 = row.get(5)?;
        let vpm_owned: i64 = row.get(6)?;
        let owner: Option<String> = row.get(7)?;
        let stable_matched: i64 = row.get(8)?;
        let stable_same: i64 = row.get(9)?;
        let verdict = unity_index::verdict(Counts {
            unique: unique_total as usize,
            unique_matched: unique_matched as usize,
            vpm_owned: vpm_owned as usize,
            files: files as usize,
        });
        let source_path: String = row.get(1)?;
        let entry: String = row.get(2)?;
        Ok(Row {
            booth_id: row.get(0)?,
            stable_ratio: if stable_matched > 0 {
                stable_same as f64 / stable_matched as f64
            } else {
                0.0
            },
            install: PackageInstall {
                name: file_label(&source_path, &entry),
                source_path,
                verdict,
                unique_total: unique_total as usize,
                unique_matched: unique_matched as usize,
                bundled_with: if verdict == Verdict::Bundled {
                    owner
                } else {
                    None
                },
                closest: false,
            },
        })
    }))?
    .collect::<Result<Vec<_>, _>>())?;
    let mut products: Vec<ProductInstall> = Vec::new();
    let mut ratios: Vec<Vec<f64>> = Vec::new();
    for row in rows {
        match products.last_mut() {
            Some(product) if product.booth_id == row.booth_id => {
                product.packages.push(row.install);
                ratios.last_mut().unwrap().push(row.stable_ratio);
            }
            _ => {
                products.push(ProductInstall {
                    booth_id: row.booth_id,
                    verdict: Verdict::Unknown,
                    packages: vec![row.install],
                });
                ratios.push(vec![row.stable_ratio]);
            }
        }
    }
    for (product, ratios) in products.iter_mut().zip(ratios) {
        let verdicts: Vec<Verdict> = product
            .packages
            .iter()
            .map(|package| package.verdict)
            .collect();
        product.verdict = unity_index::product_verdict(&verdicts);
        let installed: Vec<usize> = (0..product.packages.len())
            .filter(|index| product.packages[*index].verdict == Verdict::Installed)
            .collect();
        if installed.len() >= 2
            && let Some(best) = installed
                .iter()
                .copied()
                .max_by(|a, b| ratios[*a].total_cmp(&ratios[*b]))
            && ratios[best] > 0.0
        {
            product.packages[best].closest = true;
        }
    }
    Ok(ProjectContents {
        scanned_at: Some(scanned_at),
        complete,
        stale,
        products,
    })
}

/// 商品が入っていると分かったプロジェクト（導入済みか一部）。調べていないプロジェクトは入らない。
pub(crate) fn product_projects(
    connection: &Connection,
    booth_id: &str,
) -> Result<Vec<(String, Verdict)>, String> {
    let mut statement =
        sql(connection.prepare("SELECT project_id FROM project_scans ORDER BY project_id"))?;
    let projects: Vec<String> =
        sql(sql(statement.query_map([], |row| row.get(0)))?.collect::<Result<Vec<_>, _>>())?;
    let mut found = Vec::new();
    for project in projects {
        let contents = project_contents(connection, &project)?;
        // 調べたあとに索引が変わった（パッケージの GUID が変わったかもしれない）結果は使わない。
        // 調べ直すまで、そのプロジェクトについては何も言わない。
        if contents.stale {
            continue;
        }
        if let Some(product) = contents
            .products
            .iter()
            .find(|product| product.booth_id == booth_id)
            && matches!(product.verdict, Verdict::Installed | Verdict::Partial)
        {
            found.push((project, product.verdict));
        }
    }
    Ok(found)
}

/// そのプロジェクトへ渡したので、前に調べた結果を古いとする。
pub(crate) fn mark_project_stale(connection: &Connection, project_id: &str) -> Result<(), String> {
    sql(connection.execute(
        "UPDATE project_scans SET stale = 1 WHERE project_id = ?1",
        params![project_id],
    ))
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unity_index::tests::{guid, package};

    fn write_meta(dir: &Path, name: &str, guid: &str, body: &[u8]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
        std::fs::write(
            dir.join(format!("{name}.meta")),
            format!("fileFormatVersion: 2\nguid: {guid}\n"),
        )
        .unwrap();
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        library: PathBuf,
        project: PathBuf,
        connection: Connection,
    }

    /// 商品 100（衣装、自分の 4 ファイル + 共有のショップフォルダ）、商品 200（lilToon の同梱版）、商品 300（別の衣装）。
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("library");
        std::fs::create_dir_all(library.join("100")).unwrap();
        std::fs::create_dir_all(library.join("200")).unwrap();
        std::fs::create_dir_all(library.join("300")).unwrap();
        std::fs::write(
            library.join("100").join("Outfit.unitypackage"),
            package(&[
                (guid(1), "Assets/Shop", None),
                (guid(2), "Assets/Shop/Outfit/a.fbx", Some(b"fbx-v1")),
                (guid(3), "Assets/Shop/Outfit/b.png", Some(b"png")),
                (
                    guid(4),
                    "Assets/Shop/Outfit/c.mat",
                    Some(
                        format!(
                            "%YAML 1.1\n m_Shader: {{fileID: 1, guid: {}, type: 3}}",
                            guid(70)
                        )
                        .as_bytes(),
                    ),
                ),
                (guid(5), "Assets/Shop/Outfit/d.prefab", Some(b"prefab")),
            ]),
        )
        .unwrap();
        std::fs::write(
            library.join("200").join("lilToon_1.4.0.unitypackage"),
            package(&[
                (
                    guid(70),
                    "Assets/lilToon/Shader/lts.shader",
                    Some(b"shader"),
                ),
                (guid(71), "Assets/lilToon/Editor/x.cs", Some(b"cs")),
            ]),
        )
        .unwrap();
        std::fs::write(
            library.join("300").join("Other.unitypackage"),
            package(&[
                (guid(1), "Assets/Shop", None),
                (guid(9), "Assets/Shop/Other/z.png", Some(b"z")),
            ]),
        )
        .unwrap();
        let project = dir.path().join("Project");
        let shop = project.join("Assets").join("Shop").join("Outfit");
        std::fs::create_dir_all(&shop).unwrap();
        std::fs::write(
            project.join("Assets").join("Shop.meta"),
            format!("guid: {}\n", guid(1)),
        )
        .unwrap();
        write_meta(&shop, "a.fbx", &guid(2), b"fbx-v1");
        write_meta(&shop, "b.png", &guid(3), b"png");
        write_meta(&shop, "c.mat", &guid(4), b"edited");
        // d.prefab は消した。
        let lil = project.join("Packages").join("jp.lilxyzw.liltoon");
        write_meta(&lil.join("Shader"), "lts.shader", &guid(70), b"shader");
        write_meta(&lil.join("Editor"), "x.cs", &guid(71), b"cs");
        std::fs::write(
            lil.join("package.json"),
            r#"{"name":"jp.lilxyzw.liltoon","displayName":"lilToon"}"#,
        )
        .unwrap();
        Fixture {
            _dir: dir,
            library,
            project,
            connection: open_memory(),
        }
    }

    fn sources(library: &Path) -> Vec<Source> {
        [
            ("100", "Outfit.unitypackage"),
            ("200", "lilToon_1.4.0.unitypackage"),
            ("300", "Other.unitypackage"),
        ]
        .iter()
        .map(|(id, name)| Source {
            booth_id: (*id).into(),
            path: library.join(id).join(name),
        })
        .collect()
    }

    #[test]
    fn refresh_reads_only_new_or_changed_files() {
        let mut f = fixture();
        let all = sources(&f.library);
        let mut seen = Vec::new();
        assert_eq!(
            refresh(
                &mut f.connection,
                &all,
                |done, total| seen.push((done, total)),
                || false
            )
            .unwrap(),
            3
        );
        assert_eq!(seen.first(), Some(&(0, 3)));
        assert_eq!(
            refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap(),
            0
        );
        // 書き換わったファイルだけ読み直す。
        std::fs::write(
            &all[2].path,
            package(&[(guid(9), "Assets/Shop/Other/z.png", Some(b"zz"))]),
        )
        .unwrap();
        let later = SystemTime::now() + Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&all[2].path)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(
            refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap(),
            1
        );
        // 登録に残っているが届かないファイル（切れた共有、消えたファイル）は、読まず、索引からも消さない。
        std::fs::remove_file(&all[1].path).unwrap();
        assert_eq!(
            refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap(),
            0
        );
        let count = |connection: &Connection| -> i64 {
            connection
                .query_row("SELECT count(*) FROM packages", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(count(&f.connection), 3);
        // ライブラリの登録から消えたものは消す。
        refresh(&mut f.connection, &all[..2], |_, _| {}, || false).unwrap();
        assert_eq!(count(&f.connection), 2);
        // 止められたら次のファイルの前で止める。
        let mut fresh = open_memory();
        assert_eq!(refresh(&mut fresh, &all, |_, _| {}, || true).unwrap(), 0);
    }

    #[test]
    fn vpm_folder_names_stay_inside_packages() {
        for name in [
            "jp.lilxyzw.liltoon",
            "com.vrchat.base",
            "nadena.dev.modular-avatar",
            "a_b",
        ] {
            assert!(vpm_folder_name(name), "{name}");
        }
        for name in [
            "", "..", "a/b", "a\\b", "C:", "a..b", ".hidden", "名前", "a b",
        ] {
            assert!(!vpm_folder_name(name), "{name}");
        }
    }

    #[test]
    fn new_library_files_do_not_forget_vpm_owners() {
        let mut f = fixture();
        // 先に衣装だけを索引にして調べる。lilToon の同梱版は、まだライブラリに無い。
        let all = sources(&f.library);
        let first = vec![all[0].clone()];
        refresh(&mut f.connection, &first, |_, _| {}, || false).unwrap();
        let vpm = vec![("jp.lilxyzw.liltoon".to_string(), "2.4.1".to_string())];
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        // あとから同梱版が増える。VPM パッケージを歩き直せなくても（フォルダを消す）、持ち主は分かる。
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        assert!(project_contents(&f.connection, "p1").unwrap().stale);
        std::fs::remove_dir_all(f.project.join("Packages")).unwrap();
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        let contents = project_contents(&f.connection, "p1").unwrap();
        let lil = contents
            .products
            .iter()
            .find(|p| p.booth_id == "200")
            .unwrap();
        assert_eq!(lil.verdict, Verdict::Bundled);
    }

    #[test]
    fn scanning_a_project_decides_installed_bundled_and_missing() {
        let mut f = fixture();
        let all = sources(&f.library);
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        let vpm = vec![("jp.lilxyzw.liltoon".to_string(), "2.4.1".to_string())];
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        let contents = project_contents(&f.connection, "p1").unwrap();
        assert!(contents.complete && !contents.stale && contents.scanned_at.is_some());
        let by_id = |id: &str| {
            contents
                .products
                .iter()
                .find(|p| p.booth_id == id)
                .unwrap()
                .clone()
        };
        let outfit = by_id("100");
        // 4 ファイル中 3 つ（d.prefab を消した）で 75%、「一部」。共有のショップフォルダは数えない。
        assert_eq!(outfit.verdict, Verdict::Partial);
        assert_eq!(
            (
                outfit.packages[0].unique_matched,
                outfit.packages[0].unique_total
            ),
            (3, 4)
        );
        let lil = by_id("200");
        assert_eq!(lil.verdict, Verdict::Bundled);
        assert_eq!(
            lil.packages[0].bundled_with.as_deref(),
            Some("jp.lilxyzw.liltoon")
        );
        assert_eq!(by_id("300").verdict, Verdict::Missing);
        // VPM パッケージは 1 回だけ歩く。
        let display: Option<String> = f
            .connection
            .query_row("SELECT display_name FROM vpm_scans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(display.as_deref(), Some("lilToon"));
        assert_eq!(
            product_projects(&f.connection, "100").unwrap(),
            [("p1".to_string(), outfit.verdict)]
        );
        assert!(product_projects(&f.connection, "300").unwrap().is_empty());
        mark_project_stale(&f.connection, "p1").unwrap();
        assert!(project_contents(&f.connection, "p1").unwrap().stale);
        // 古い結果からは、プレビューの行に「入っている」と出さない。調べ直せば戻る。
        assert!(product_projects(&f.connection, "100").unwrap().is_empty());
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        assert_eq!(product_projects(&f.connection, "100").unwrap().len(), 1);
        // 調べていないプロジェクトは空で、「入っていない」とは言わない。
        let none = project_contents(&f.connection, "other").unwrap();
        assert!(none.scanned_at.is_none() && none.products.is_empty());
    }

    #[test]
    fn two_versions_mark_the_closest_by_stable_file_sizes() {
        let mut f = fixture();
        let v2 = f.library.join("100").join("Outfit_v2.unitypackage");
        std::fs::write(
            &v2,
            package(&[
                (guid(2), "Assets/Shop/Outfit/a.fbx", Some(b"fbx-v2-longer")),
                (guid(3), "Assets/Shop/Outfit/b.png", Some(b"png-v2")),
                (guid(4), "Assets/Shop/Outfit/c.mat", Some(b"m")),
                (guid(5), "Assets/Shop/Outfit/d.prefab", Some(b"p")),
            ]),
        )
        .unwrap();
        let mut all = sources(&f.library);
        all.push(Source {
            booth_id: "100".into(),
            path: v2,
        });
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        // d.prefab も戻して、どちらの版も導入済みにする。
        std::fs::write(f.project.join("Assets/Shop/Outfit/d.prefab"), b"prefab").unwrap();
        std::fs::write(
            f.project.join("Assets/Shop/Outfit/d.prefab.meta"),
            format!("guid: {}\n", guid(5)),
        )
        .unwrap();
        scan_project(&mut f.connection, "p1", &f.project, &[]).unwrap();
        let contents = project_contents(&f.connection, "p1").unwrap();
        let outfit = contents
            .products
            .iter()
            .find(|p| p.booth_id == "100")
            .unwrap();
        assert_eq!(outfit.verdict, Verdict::Installed);
        let closest: Vec<&str> = outfit
            .packages
            .iter()
            .filter(|p| p.closest)
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(closest, ["Outfit.unitypackage"]);
    }

    /// この PC のライブラリとプロジェクトを読むだけの確認。索引は一時フォルダに作る。
    /// `VRCLIBMAN_REAL_LIBRARY`（商品 ID のフォルダが並ぶ場所）と `VRCLIBMAN_REAL_PROJECT` を指定して
    /// `cargo test -- --ignored real_library_index --nocapture` で動かす。名前とパスは出さない。
    #[test]
    #[ignore]
    fn real_library_index() {
        let library =
            PathBuf::from(std::env::var("VRCLIBMAN_REAL_LIBRARY").expect("VRCLIBMAN_REAL_LIBRARY"));
        let project =
            PathBuf::from(std::env::var("VRCLIBMAN_REAL_PROJECT").expect("VRCLIBMAN_REAL_PROJECT"));
        let mut found = Vec::new();
        for entry in walkdir::WalkDir::new(&library)
            .max_depth(2)
            .into_iter()
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if entry.file_type().is_file()
                && (name.ends_with(".zip") || name.ends_with(".unitypackage"))
            {
                let booth = entry
                    .path()
                    .parent()
                    .and_then(|p| p.file_name())
                    .unwrap()
                    .to_string_lossy()
                    .to_string();
                found.push(Source {
                    booth_id: booth,
                    path: entry.path().to_path_buf(),
                });
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut connection = open(&dir.path().join(FILE_NAME)).unwrap();
        let started = Instant::now();
        let read = refresh(&mut connection, &found, |_, _| {}, || false).unwrap();
        let indexed = started.elapsed();
        let manifest = std::fs::read_to_string(project.join("Packages").join("vpm-manifest.json"))
            .unwrap_or_default();
        let (_, packages) = crate::unity_projects::parse_vpm_manifest(&manifest);
        let vpm: Vec<(String, String)> =
            packages.into_iter().map(|p| (p.name, p.version)).collect();
        let started = Instant::now();
        scan_project(&mut connection, "real", &project, &vpm).unwrap();
        let scanned = started.elapsed();
        let started = Instant::now();
        scan_project(&mut connection, "real", &project, &vpm).unwrap();
        let rescanned = started.elapsed();
        let contents = project_contents(&connection, "real").unwrap();
        eprintln!(
            "sources={} read={read} indexed={indexed:?} scan={scanned:?} rescan={rescanned:?} complete={}",
            found.len(),
            contents.complete
        );
        for product in &contents.products {
            let packages: Vec<String> = product
                .packages
                .iter()
                .map(|p| {
                    format!(
                        "{:?} {}/{}{}{}",
                        p.verdict,
                        p.unique_matched,
                        p.unique_total,
                        p.bundled_with
                            .as_ref()
                            .map(|o| format!(" bundled:{o}"))
                            .unwrap_or_default(),
                        if p.closest { " closest" } else { "" }
                    )
                })
                .collect();
            eprintln!(
                "  {} {:?} [{}]",
                product.booth_id,
                product.verdict,
                packages.join("; ")
            );
        }
    }

    #[test]
    fn index_file_with_another_version_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch("CREATE TABLE old (x); PRAGMA user_version = 99;")
                .unwrap();
        }
        let connection = open(&path).unwrap();
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, INDEX_VERSION);
        let old: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE name = 'old'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert!(old.is_none());
    }
}
