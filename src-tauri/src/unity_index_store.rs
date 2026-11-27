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
/// 2・3: 段階 3 の `project_found_refs` を足し、`guid_owners` を VPM パッケージの版ごとにした。
pub(crate) const INDEX_VERSION: i64 = 3;
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
    guid BLOB NOT NULL,
    package_name TEXT NOT NULL,
    version TEXT NOT NULL,
    PRIMARY KEY (guid, package_name, version)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS vpm_scans (
    package_name TEXT NOT NULL,
    version TEXT NOT NULL,
    display_name TEXT,
    PRIMARY KEY (package_name, version)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS project_found_refs (
    project_id TEXT NOT NULL,
    package_id INTEGER NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
    guid BLOB NOT NULL,
    PRIMARY KEY (project_id, package_id, guid)
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
#[cfg(test)]
pub(crate) fn refresh(
    connection: &mut Connection,
    sources: &[Source],
    progress: impl FnMut(usize, usize),
    stop: impl Fn() -> bool,
) -> Result<usize, String> {
    refresh_counted(connection, sources, progress, stop).map(|(read, _)| read)
}

/// `refresh` と同じで、（読んだ数、消したファイルの行の数）を返す。どちらかが 0 でなければ索引が変わった。
pub(crate) fn refresh_counted(
    connection: &mut Connection,
    sources: &[Source],
    mut progress: impl FnMut(usize, usize),
    stop: impl Fn() -> bool,
) -> Result<(usize, usize), String> {
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
    Ok((read, removed))
}

/// VPM パッケージの名前として歩いてよい形（`jp.lilxyzw.liltoon` など）。
pub(crate) fn vpm_folder_name(name: &str) -> bool {
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
        // 持ち主は版ごとに覚える。版が変わるとファイルが増えたり無くなったりするので、古い版で覚えた
        // GUID を新しい版の持ち主にしないため。歩き終えた版は、その版の行を入れ直す。
        let tx = sql(connection.transaction())?;
        if scan.complete {
            sql(tx.execute(
                "DELETE FROM guid_owners WHERE package_name = ?1 AND version = ?2",
                params![name, version],
            ))?;
        }
        {
            let mut insert = sql(tx.prepare(
                "INSERT OR IGNORE INTO guid_owners (guid, package_name, version) VALUES (?1, ?2, ?3)",
            ))?;
            for guid in scan.found.keys() {
                sql(insert.execute(params![guid.as_slice(), name, version]))?;
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
    // 持ち主として使うのは、このプロジェクトに入っている VPM パッケージだけ。ほかのプロジェクトで覚えた
    // パッケージを持ち主にすると、そのパッケージの無いプロジェクトでも同梱版として判定から外れ、
    // 固有のファイルからも外れるため（VPM 版が Packages に無ければ、取り込んだものは Assets にある）。
    // 版も、このプロジェクトに入っているものだけ。
    let present: HashSet<(&str, &str)> = vpm_packages
        .iter()
        .map(|(name, version)| (name.as_str(), version.as_str()))
        .collect();
    let owners: HashMap<Guid, String> = {
        let mut statement =
            sql(connection.prepare("SELECT guid, package_name, version FROM guid_owners"))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }))?;
        let mut map = HashMap::new();
        for row in rows {
            let (guid, name, version) = sql(row)?;
            // 使うのは、ライブラリのパッケージにある GUID の分だけ。
            if let Ok(guid) = <Guid>::try_from(guid.as_slice())
                && guids.contains_key(&guid)
                && present.contains(&(name.as_str(), version.as_str()))
            {
                map.insert(guid, name);
            }
        }
        map
    };
    // 段階 3: パッケージの外への参照。
    let refs: HashMap<i64, Vec<Guid>> = {
        let mut statement = sql(connection.prepare("SELECT package_id, guid FROM package_refs"))?;
        let rows = sql(statement.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        }))?;
        let mut map: HashMap<i64, Vec<Guid>> = HashMap::new();
        for row in rows {
            let (package_id, guid) = sql(row)?;
            if let Ok(guid) = <Guid>::try_from(guid.as_slice())
                && package_booth.contains_key(&package_id)
            {
                map.entry(package_id).or_default().push(guid);
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

    // 段階 3: パッケージの外への参照のうち、このプロジェクトの Assets に**あった**ものを残す。無かったものは
    // `package_refs` からこれを引けば分かる。あったものは少なく（Assets に取り込んだほかの商品を指すときだけ）、
    // 無かったものを残すと、パッケージの数 × 参照の数 × プロジェクトの数の行になるため。
    // 持ち主は見せるときに引く（`package_needs`）。あとで別のプロジェクトを調べて持ち主が分かったときも、
    // 調べ直さずに言える。
    let found_refs: Vec<(i64, &Guid)> = refs
        .iter()
        .flat_map(|(package_id, guids)| guids.iter().map(move |guid| (*package_id, guid)))
        .filter(|(_, guid)| scan.found.contains_key(*guid))
        .collect();

    let tx = sql(connection.transaction())?;
    sql(tx.execute(
        "DELETE FROM project_packages WHERE project_id = ?1",
        params![project_id],
    ))?;
    sql(tx.execute(
        "DELETE FROM project_found_refs WHERE project_id = ?1",
        params![project_id],
    ))?;
    {
        let mut insert = sql(tx.prepare(
            "INSERT OR IGNORE INTO project_found_refs (project_id, package_id, guid) VALUES (?1, ?2, ?3)",
        ))?;
        for (package_id, guid) in &found_refs {
            sql(insert.execute(params![project_id, package_id, guid.as_slice()]))?;
        }
    }
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

/// この結果から「入っていない」と言えるか。調べていない・古い・途中までしか歩けなかったときは言えない。
pub(crate) fn absence_known(contents: &ProjectContents) -> bool {
    contents.scanned_at.is_some() && contents.complete && !contents.stale
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

/// 段階 3: パッケージが外に必要としていて、調べたときプロジェクトの Assets に無かったものの持ち主。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NeedOwner {
    /// VPM パッケージの名前（`jp.lilxyzw.liltoon`）。入っているかは見せるときに決める。
    Vpm(String),
    /// プロジェクトにその VPM パッケージは入っているが、入っている版を歩いたところ、そのファイルが無かった。
    VpmLacking(String),
    /// ライブラリのほかの商品の ID。
    Product(String),
}

/// 選んだファイル（zip の中の項目）の索引の行。パスは `same_path` で比べる。
fn package_ids(
    connection: &Connection,
    source: &Path,
    entries: &[String],
) -> Result<Vec<(i64, String, i64)>, String> {
    let mut statement = sql(connection.prepare(
        "SELECT id, source_path, file_count FROM packages WHERE entry = ?1 AND failed = 0",
    ))?;
    let mut found = Vec::new();
    for entry in entries {
        let rows = sql(statement.query_map(params![entry], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        }))?;
        for row in rows {
            let (id, path, files) = sql(row)?;
            if crate::library_files::same_path(Path::new(&path), source) {
                found.push((id, entry.clone(), files));
            }
        }
    }
    Ok(found)
}

/// 選んだパッケージが外に必要としていて、このプロジェクトに無かったもの（持ち主ごとの参照の数）。
/// 調べていない・古い・途中までしか歩けなかったときは `None`（足りないと言わない）。
/// `project_vpm` はいまのプロジェクトの `vpm-manifest.json` の名前と版。
pub(crate) fn package_needs(
    connection: &Connection,
    project_id: &str,
    source: &Path,
    entries: &[String],
    project_vpm: &[(String, String)],
) -> Result<Option<Vec<(NeedOwner, usize)>>, String> {
    let usable: Option<bool> = sql(connection
        .query_row(
            "SELECT complete = 1 AND stale = 0 FROM project_scans WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .optional())?;
    if usable != Some(true) {
        return Ok(None);
    }
    let mut missing = sql(connection.prepare(
        "SELECT r.guid, p.booth_id FROM package_refs r JOIN packages p ON p.id = r.package_id
         WHERE r.package_id = ?2 AND NOT EXISTS (
             SELECT 1 FROM project_found_refs f
             WHERE f.project_id = ?1 AND f.package_id = r.package_id AND f.guid = r.guid)",
    ))?;
    let mut vpm_owners = sql(connection.prepare(
        "SELECT package_name, version FROM guid_owners WHERE guid = ?1 ORDER BY package_name, version",
    ))?;
    let mut walked = sql(
        connection.prepare("SELECT 1 FROM vpm_scans WHERE package_name = ?1 AND version = ?2")
    )?;
    let mut holders = sql(connection.prepare(
        "SELECT DISTINCT p.booth_id FROM package_guids g JOIN packages p ON p.id = g.package_id
         WHERE g.guid = ?1 AND g.asset_size IS NOT NULL AND p.failed = 0 AND p.booth_id IS NOT NULL",
    ))?;
    let mut totals: Vec<(NeedOwner, usize)> = Vec::new();
    for (package_id, _, _) in package_ids(connection, source, entries)? {
        let rows: Vec<(Vec<u8>, Option<String>)> = sql(sql(missing
            .query_map(params![project_id, package_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            }))?
        .collect::<Result<Vec<_>, _>>())?;
        for (guid, own) in rows {
            let owners: Vec<(String, String)> = sql(sql(
                vpm_owners.query_map(params![guid], |row| Ok((row.get(0)?, row.get(1)?)))
            )?
            .collect::<Result<Vec<_>, _>>())?;
            let owner = if let Some((first, _)) = owners.first() {
                // 同じファイルを 2 つ以上のパッケージ（や版）が持つことがある。プロジェクトに入っているものを先に見る。
                let has_exact = owners.iter().find(|owner| project_vpm.contains(owner));
                let other_version = owners
                    .iter()
                    .find(|(name, _)| project_vpm.iter().any(|(known, _)| known == name));
                match (has_exact, other_version) {
                    // 入っている版がそのファイルを持つ。入っているので、見せるときに外れる。
                    (Some((name, _)), _) => NeedOwner::Vpm(name.clone()),
                    // 名前は入っているが、持っているのは別の版。入っている版を最後まで歩いていれば、
                    // その版には無いと分かる。歩いていない版については言わない。
                    (None, Some((name, _))) => {
                        let installed = project_vpm
                            .iter()
                            .find(|(known, _)| known == name)
                            .map(|(_, version)| version);
                        let lacking = match installed {
                            Some(version) => sql(walked.exists(params![name, version]))?,
                            None => false,
                        };
                        if lacking {
                            NeedOwner::VpmLacking(name.clone())
                        } else {
                            NeedOwner::Vpm(name.clone())
                        }
                    }
                    (None, None) => NeedOwner::Vpm(first.clone()),
                }
            } else {
                let booths: Vec<String> =
                    sql(sql(holders.query_map(params![guid], |row| row.get(0)))?
                        .collect::<Result<Vec<_>, _>>())?;
                // 持ち主がライブラリの商品 1 つだけで、渡す商品とは別のときだけ。同じ商品のほかのパッケージにも
                // あるもの、2 つ以上の商品にまたがるもの、持ち主の分からないものは言わない。
                match booths.as_slice() {
                    [booth] if Some(booth) != own.as_ref() => NeedOwner::Product(booth.clone()),
                    _ => continue,
                }
            };
            match totals.iter_mut().find(|(known, _)| *known == owner) {
                Some((_, count)) => *count += 1,
                None => totals.push((owner, 1)),
            }
        }
    }
    totals.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(Some(totals))
}

/// 選んだパッケージのうち、同梱版（ファイルの 80% 以上を 1 つの VPM パッケージが持つ）のもの。
/// 持ち主はどこかのプロジェクトで歩いた VPM パッケージから決まるので、渡す先を調べていなくても言える。
pub(crate) fn bundled_packages(
    connection: &Connection,
    source: &Path,
    entries: &[String],
) -> Result<Vec<(String, String)>, String> {
    let mut statement = sql(connection.prepare(
        "SELECT o.package_name, COUNT(DISTINCT g.guid) AS owned FROM package_guids g
         JOIN guid_owners o ON o.guid = g.guid
         WHERE g.package_id = ?1 AND g.asset_size IS NOT NULL
         GROUP BY o.package_name ORDER BY owned DESC, o.package_name LIMIT 1",
    ))?;
    let mut found = Vec::new();
    for (package_id, entry, files) in package_ids(connection, source, entries)? {
        let top: Option<(String, i64)> = sql(statement
            .query_row(params![package_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional())?;
        if let Some((name, owned)) = top
            && files > 0
            && owned as f64 >= unity_index::BUNDLED_RATIO * files as f64
        {
            found.push((entry, name));
        }
    }
    Ok(found)
}

/// 歩いた VPM パッケージの読みやすい名前（`package.json` の `displayName`）。
pub(crate) fn vpm_display_names(
    connection: &Connection,
) -> Result<HashMap<String, String>, String> {
    let mut statement = sql(connection.prepare(
        "SELECT package_name, display_name FROM vpm_scans WHERE display_name IS NOT NULL AND display_name <> ''
         ORDER BY package_name, version",
    ))?;
    let rows = sql(statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))))?;
    let mut names = HashMap::new();
    for row in rows {
        let (name, display): (String, String) = sql(row)?;
        names.insert(name, display);
    }
    Ok(names)
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
        // 消しただけのときも、消した数を返す（開いている画面に知らせるため）。
        let mut counted = open_memory();
        let kept = vec![all[0].clone(), all[2].clone()];
        refresh(&mut counted, &kept, |_, _| {}, || false).unwrap();
        assert_eq!(
            refresh_counted(&mut counted, &all[..1], |_, _| {}, || false).unwrap(),
            (0, 1)
        );
        // 止められたら次のファイルの前で止める。
        let mut fresh = open_memory();
        assert_eq!(refresh(&mut fresh, &all, |_, _| {}, || true).unwrap(), 0);
    }

    /// 段階 3: 商品 400（アバター本体）と、それを指す商品 500（衣装。同じ商品の共通パッケージも指す）。
    fn needs_fixture() -> (Fixture, Vec<Source>) {
        let f = fixture();
        std::fs::create_dir_all(f.library.join("400")).unwrap();
        std::fs::create_dir_all(f.library.join("500")).unwrap();
        std::fs::write(
            f.library.join("400").join("Body.unitypackage"),
            package(&[(guid(80), "Assets/Body/body.fbx", Some(b"fbx"))]),
        )
        .unwrap();
        let prefab = format!(
            "%YAML 1.1\n m_Script: {{fileID: 1, guid: {}, type: 3}}\n m_Mesh: {{fileID: 2, guid: {}, type: 3}}\n m_Common: {{guid: {}}}\n m_Unknown: {{guid: {}}}\n",
            guid(70),
            guid(80),
            guid(95),
            guid(99)
        );
        std::fs::write(
            f.library.join("500").join("Cloth.unitypackage"),
            package(&[(guid(90), "Assets/Cloth/c.prefab", Some(prefab.as_bytes()))]),
        )
        .unwrap();
        std::fs::write(
            f.library.join("500").join("ClothCommon.unitypackage"),
            package(&[(guid(95), "Assets/Cloth/common.png", Some(b"png"))]),
        )
        .unwrap();
        let mut all = sources(&f.library);
        for (id, name) in [
            ("400", "Body.unitypackage"),
            ("500", "Cloth.unitypackage"),
            ("500", "ClothCommon.unitypackage"),
        ] {
            all.push(Source {
                booth_id: id.into(),
                path: f.library.join(id).join(name),
            });
        }
        (f, all)
    }

    #[test]
    fn outside_references_are_counted_by_owner() {
        let (mut f, all) = needs_fixture();
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        let cloth = f.library.join("500").join("Cloth.unitypackage");
        let entries = vec![String::new()];
        // 調べていないプロジェクトについては言わない。
        assert_eq!(
            package_needs(&f.connection, "p1", &cloth, &entries, &[]).unwrap(),
            None
        );
        let vpm = vec![("jp.lilxyzw.liltoon".to_string(), "2.4.1".to_string())];
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        let lil = NeedOwner::Vpm("jp.lilxyzw.liltoon".into());
        // 本体（ほかの商品）と lilToon（VPM）。同じ商品の共通パッケージと、持ち主の分からない参照は言わない。
        assert_eq!(
            package_needs(&f.connection, "p1", &cloth, &entries, &[]).unwrap(),
            Some(vec![
                (lil.clone(), 1),
                (NeedOwner::Product("400".into()), 1)
            ])
        );
        // 衣装 100 のマテリアルは lilToon のシェーダーを指す（Assets に無く、Packages にある）。
        // 入っているかは見せるときに vpm-manifest.json で決めるので、ここでは持ち主だけを返す。
        let outfit = f.library.join("100").join("Outfit.unitypackage");
        assert_eq!(
            package_needs(&f.connection, "p1", &outfit, &entries, &[]).unwrap(),
            Some(vec![(lil.clone(), 1)])
        );
        // 本体を取り込んだあとに調べ直すと、本体は言わない。
        write_meta(
            &f.project.join("Assets").join("Body"),
            "body.fbx",
            &guid(80),
            b"fbx",
        );
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        assert_eq!(
            package_needs(&f.connection, "p1", &cloth, &entries, &[]).unwrap(),
            Some(vec![(lil, 1)])
        );
        // 渡したあと（古い結果）と、知らないファイルについては言わない。
        mark_project_stale(&f.connection, "p1").unwrap();
        assert_eq!(
            package_needs(&f.connection, "p1", &cloth, &entries, &[]).unwrap(),
            None
        );
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        assert_eq!(
            package_needs(
                &f.connection,
                "p1",
                &f.library.join("x.unitypackage"),
                &entries,
                &[]
            )
            .unwrap(),
            Some(Vec::new())
        );
    }

    #[test]
    fn a_file_missing_from_the_installed_vpm_version_is_reported() {
        let (mut f, all) = needs_fixture();
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        let outfit = f.library.join("100").join("Outfit.unitypackage");
        let entries = vec![String::new()];
        // p1 は lilToon 1.0（シェーダー guid(70) がある）。p2 は lilToon 2.0 で、そのシェーダーが無い。
        let old = vec![("jp.lilxyzw.liltoon".to_string(), "1.0".to_string())];
        scan_project(&mut f.connection, "p1", &f.project, &old).unwrap();
        let newer = f._dir.path().join("Newer");
        std::fs::create_dir_all(newer.join("Assets")).unwrap();
        let lil = newer.join("Packages").join("jp.lilxyzw.liltoon");
        write_meta(&lil.join("Editor"), "x.cs", &guid(71), b"cs");
        let new = vec![("jp.lilxyzw.liltoon".to_string(), "2.0".to_string())];
        scan_project(&mut f.connection, "p2", &newer, &new).unwrap();
        // p2 の入っている版（2.0）を歩いていて、そこに無いので、入っていても足りない。
        assert_eq!(
            package_needs(&f.connection, "p2", &outfit, &entries, &new).unwrap(),
            Some(vec![(
                NeedOwner::VpmLacking("jp.lilxyzw.liltoon".into()),
                1
            )])
        );
        // 1.0 の入っているプロジェクトでは足りている（入っているかは見せるときに決める）。
        assert_eq!(
            package_needs(&f.connection, "p1", &outfit, &entries, &old).unwrap(),
            Some(vec![(NeedOwner::Vpm("jp.lilxyzw.liltoon".into()), 1)])
        );
        // 歩いていない版については言わない。
        let unknown = vec![("jp.lilxyzw.liltoon".to_string(), "3.0".to_string())];
        assert_eq!(
            package_needs(&f.connection, "p2", &outfit, &entries, &unknown).unwrap(),
            Some(vec![(NeedOwner::Vpm("jp.lilxyzw.liltoon".into()), 1)])
        );
        // 版ごとに覚えるので、2.0 の p2 では lilToon の同梱版（guid(70) を持つ）も持ち主を数えない。
        let lil_bundle = project_contents(&f.connection, "p2")
            .unwrap()
            .products
            .into_iter()
            .find(|p| p.booth_id == "200")
            .unwrap();
        assert_ne!(lil_bundle.verdict, Verdict::Bundled);
    }

    #[test]
    fn a_file_owned_by_two_packages_counts_for_the_one_the_project_has() {
        let (mut f, all) = needs_fixture();
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        let outfit = f.library.join("100").join("Outfit.unitypackage");
        let entries = vec![String::new()];
        // シェーダー guid(70) を、lilToon と、名前の違う別のパッケージの両方が持つ。
        let fork = f.project.join("Packages").join("com.example.fork");
        write_meta(&fork.join("Shader"), "lts.shader", &guid(70), b"shader");
        let both = vec![
            ("com.example.fork".to_string(), "1.0".to_string()),
            ("jp.lilxyzw.liltoon".to_string(), "2.4.1".to_string()),
        ];
        scan_project(&mut f.connection, "p1", &f.project, &both).unwrap();
        // lilToon だけが入っているプロジェクトでは、lilToon を持ち主にする（名前の順で先の fork ではなく）。
        let only_lil = vec![both[1].clone()];
        assert_eq!(
            package_needs(&f.connection, "p1", &outfit, &entries, &only_lil).unwrap(),
            Some(vec![(NeedOwner::Vpm("jp.lilxyzw.liltoon".into()), 1)])
        );
        // どちらも入っていなければ、名前の順で先のもの。
        assert_eq!(
            package_needs(&f.connection, "p1", &outfit, &entries, &[]).unwrap(),
            Some(vec![(NeedOwner::Vpm("com.example.fork".into()), 1)])
        );
    }

    #[test]
    fn owners_learned_from_another_project_count_without_rescanning() {
        let (mut f, all) = needs_fixture();
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        let outfit = f.library.join("100").join("Outfit.unitypackage");
        let entries = vec![String::new()];
        // p1 は lilToon を入れていない（歩かない）ので、VPM の持ち主はまだ分からない。シェーダーを持つのは
        // ライブラリの商品 200（lilToon の同梱版）だけなので、その商品を言う。
        scan_project(&mut f.connection, "p1", &f.project, &[]).unwrap();
        assert_eq!(
            package_needs(&f.connection, "p1", &outfit, &entries, &[]).unwrap(),
            Some(vec![(NeedOwner::Product("200".into()), 1)])
        );
        // 別のプロジェクト p2 で lilToon を歩くと、p1 を調べ直さなくても持ち主が分かる。
        let vpm = vec![("jp.lilxyzw.liltoon".to_string(), "2.4.1".to_string())];
        scan_project(&mut f.connection, "p2", &f.project, &vpm).unwrap();
        assert_eq!(
            package_needs(&f.connection, "p1", &outfit, &entries, &[]).unwrap(),
            Some(vec![(NeedOwner::Vpm("jp.lilxyzw.liltoon".into()), 1)])
        );
    }

    #[test]
    fn bundled_packages_are_known_from_any_walked_project() {
        let (mut f, all) = needs_fixture();
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        let lil_path = f.library.join("200").join("lilToon_1.4.0.unitypackage");
        let entries = vec![String::new()];
        // まだどのプロジェクトでも lilToon を歩いていないので、同梱版とは分からない。
        assert!(
            bundled_packages(&f.connection, &lil_path, &entries)
                .unwrap()
                .is_empty()
        );
        let vpm = vec![("jp.lilxyzw.liltoon".to_string(), "2.4.1".to_string())];
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        assert_eq!(
            bundled_packages(&f.connection, &lil_path, &entries).unwrap(),
            vec![(String::new(), "jp.lilxyzw.liltoon".to_string())]
        );
        assert!(
            bundled_packages(
                &f.connection,
                &f.library.join("100").join("Outfit.unitypackage"),
                &entries
            )
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            vpm_display_names(&f.connection)
                .unwrap()
                .get("jp.lilxyzw.liltoon"),
            Some(&"lilToon".to_string())
        );
    }

    #[test]
    fn vpm_owners_count_only_in_projects_that_have_the_package() {
        let mut f = fixture();
        let all = sources(&f.library);
        refresh(&mut f.connection, &all, |_, _| {}, || false).unwrap();
        // p1 で lilToon を歩き、持ち主を覚える。
        let vpm = vec![("jp.lilxyzw.liltoon".to_string(), "2.4.1".to_string())];
        scan_project(&mut f.connection, "p1", &f.project, &vpm).unwrap();
        // lilToon の無いプロジェクト p2 に、同梱版を Assets へ取り込んだ。同梱版ではなく、ふつうの
        // パッケージとして「導入済み」になる。
        let other = f._dir.path().join("Other");
        write_meta(
            &other.join("Assets").join("lilToon"),
            "lts.shader",
            &guid(70),
            b"shader",
        );
        write_meta(
            &other.join("Assets").join("lilToon"),
            "x.cs",
            &guid(71),
            b"cs",
        );
        scan_project(&mut f.connection, "p2", &other, &[]).unwrap();
        let contents = project_contents(&f.connection, "p2").unwrap();
        let lil = contents
            .products
            .iter()
            .find(|p| p.booth_id == "200")
            .unwrap();
        assert_eq!(lil.verdict, Verdict::Installed);
        assert_eq!(
            project_contents(&f.connection, "p1")
                .unwrap()
                .products
                .iter()
                .find(|p| p.booth_id == "200")
                .unwrap()
                .verdict,
            Verdict::Bundled
        );
    }

    #[test]
    fn absence_is_known_only_from_a_fresh_complete_scan() {
        let contents = |scanned_at: Option<i64>, complete: bool, stale: bool| ProjectContents {
            scanned_at,
            complete,
            stale,
            products: Vec::new(),
        };
        assert!(absence_known(&contents(Some(1), true, false)));
        assert!(!absence_known(&contents(None, false, false)));
        assert!(!absence_known(&contents(Some(1), false, false)));
        assert!(!absence_known(&contents(Some(1), true, true)));
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

    /// 段階 3 の下調べ。`VRCLIBMAN_REAL_LIBRARY` を索引にし、ALCOM の全プロジェクトを調べたうえで、
    /// パッケージの外への参照の持ち主を数える。商品名・パスは出さない（商品 ID の末尾 4 桁だけ）。
    #[test]
    #[ignore]
    fn real_reference_owners() {
        let library =
            PathBuf::from(std::env::var("VRCLIBMAN_REAL_LIBRARY").expect("VRCLIBMAN_REAL_LIBRARY"));
        let settings = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap())
            .join("VRChatCreatorCompanion")
            .join("settings.json");
        let projects = crate::unity_projects::parse_user_projects(
            &std::fs::read_to_string(settings).unwrap_or_default(),
        );
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
        let dir = std::env::var("VRCLIBMAN_REAL_INDEX_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| tempfile::tempdir().unwrap().keep());
        let mut connection = open(&dir.join(FILE_NAME)).unwrap();
        refresh(&mut connection, &found, |_, _| {}, || false).unwrap();
        let mut manifests: Vec<(usize, HashSet<String>)> = Vec::new();
        for (index, path) in projects.iter().enumerate() {
            let path = PathBuf::from(path);
            let manifest = std::fs::read_to_string(path.join("Packages").join("vpm-manifest.json"))
                .unwrap_or_default();
            let (_, packages) = crate::unity_projects::parse_vpm_manifest(&manifest);
            let names: HashSet<String> = packages.iter().map(|p| p.name.clone()).collect();
            let vpm: Vec<(String, String)> =
                packages.into_iter().map(|p| (p.name, p.version)).collect();
            let started = Instant::now();
            let _ = scan_project(&mut connection, &format!("p{index}"), &path, &vpm);
            eprintln!("scanned p{index} in {:?}", started.elapsed());
            manifests.push((index, names));
        }
        let owners: HashMap<Vec<u8>, String> = {
            let mut s = connection
                .prepare("SELECT guid, package_name FROM guid_owners")
                .unwrap();
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .flatten()
                .collect()
        };
        let library_owner: HashMap<Vec<u8>, HashSet<String>> = {
            let mut s = connection
                .prepare("SELECT g.guid, p.booth_id FROM package_guids g JOIN packages p ON p.id = g.package_id WHERE g.asset_size IS NOT NULL")
                .unwrap();
            let mut map: HashMap<Vec<u8>, HashSet<String>> = HashMap::new();
            for (guid, booth) in s
                .query_map([], |r| {
                    Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
                })
                .unwrap()
                .flatten()
            {
                map.entry(guid).or_default().insert(booth);
            }
            map
        };
        let packages: Vec<(i64, String, i64)> = {
            let mut s = connection
                .prepare("SELECT id, booth_id, file_count FROM packages WHERE file_count > 0")
                .unwrap();
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .flatten()
                .collect()
        };
        let tail = |booth: &str| booth[booth.len().saturating_sub(4)..].to_string();
        let mut unknown_all: HashMap<String, usize> = HashMap::new();
        for (id, booth, files) in &packages {
            let mut s = connection
                .prepare("SELECT guid FROM package_refs WHERE package_id = ?1")
                .unwrap();
            let refs: Vec<Vec<u8>> = s
                .query_map(params![id], |r| r.get(0))
                .unwrap()
                .flatten()
                .collect();
            let mut vpm: HashMap<String, usize> = HashMap::new();
            let mut products: HashMap<String, usize> = HashMap::new();
            let mut unknown = 0;
            for guid in &refs {
                if let Some(owner) = owners.get(guid) {
                    *vpm.entry(owner.clone()).or_default() += 1;
                } else if let Some(booths) = library_owner.get(guid) {
                    let others: Vec<String> = booths
                        .iter()
                        .filter(|b| *b != booth)
                        .map(|b| tail(b))
                        .collect();
                    let key = if others.is_empty() {
                        "self(other package)".to_string()
                    } else {
                        others.join("+")
                    };
                    *products.entry(key).or_default() += 1;
                } else {
                    unknown += 1;
                    let text: String = guid.iter().map(|b| format!("{b:02x}")).collect();
                    *unknown_all.entry(text).or_default() += 1;
                }
            }
            let mut vpm: Vec<_> = vpm.into_iter().collect();
            vpm.sort();
            let mut products: Vec<_> = products.into_iter().collect();
            products.sort();
            // VPM の持ち主のうち、入っていないプロジェクトの数。
            let lacking: Vec<String> = vpm
                .iter()
                .map(|(name, _)| {
                    let without = manifests
                        .iter()
                        .filter(|(_, names)| !names.contains(name))
                        .count();
                    format!("{name}:{without}")
                })
                .collect();
            eprintln!(
                "pkg {} files={files} refs={} vpm={:?} products={:?} unknown={unknown} lacking={:?}",
                tail(booth),
                refs.len(),
                vpm,
                products,
                lacking
            );
        }
        let mut unknown_all: Vec<_> = unknown_all.into_iter().collect();
        unknown_all.sort_by(|a, b| b.1.cmp(&a.1));
        eprintln!("unknown guids (top 40, by packages):");
        for (guid, count) in unknown_all.iter().take(40) {
            eprintln!("  {guid} {count}");
        }
        eprintln!(
            "owners known: {} guids; projects: {}",
            owners.len(),
            manifests.len()
        );
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
