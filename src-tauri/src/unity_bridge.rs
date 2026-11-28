//! Unity のエディタ拡張（docs/unity-projects.md の段階 4）からの問い合わせ。拡張は Unity の中に
//! VRCLibMan の窓を出し、いま開いているプロジェクトに入っている商品・まだ入っていない対応アセットを並べ、
//! 押した商品のパッケージを Unity の Import 画面に渡す。
//!
//! - 受けるのは受信口の `/unity/*` だけ。`/mcp/` と同じく、`Origin` のある要求（ウェブページと
//!   ブラウザ拡張）は断り、設定でオンのとき（`unity_bridge_enabled`、既定オフ）だけ、専用のコード
//!   （`unity_bridge_token`）の Bearer で通す。コードはオンのときだけアプリデータの `unity-token` に置く。
//! - 判定・検査・取り出しはアプリの規則をそのまま使う（`unity_index`・`package_check`・
//!   `prepare_packages`）。拡張は表示と `AssetDatabase.ImportPackage` だけを持つ。
//! - プロジェクトのフォルダには書き込まない。調べるのは一覧にあるプロジェクトだけ。
use crate::db;
use crate::mcp_api::{self, constant_eq, generate_token, token_ok, write_private};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use tauri::AppHandle;

/// アプリデータに置く、エディタ拡張が読む認証コードのファイル名。
pub(crate) const TOKEN_FILE: &str = "unity-token";
const ENABLED_KEY: &str = "unity_bridge_enabled";
const TOKEN_KEY: &str = "unity_bridge_token";
pub(crate) const BODY_LIMIT: usize = 16 * 1024;
/// アプリのローカルデータに写すエディタ拡張のフォルダ（ALCOM のユーザーパッケージに足す）。
pub(crate) const PACKAGE_FOLDER: &str = "unity-package";
pub(crate) const PACKAGE_NAME: &str = "app.vrclibman.unity";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// いまのプロジェクトについて前に調べた結果と、まだ入っていない対応アセット。調べ直さない。
    Project,
    /// プロジェクトを調べ直してから `Project` と同じものを返す。
    Scan,
    /// 商品の手元のファイルと、中の Unity パッケージ。
    Files,
    /// 渡す前の確かめ（足りないもの・要らない同梱版・もう入っている）。
    Check,
    /// 選んだパッケージを取り出して、Unity が読むパスを返す。
    Prepare,
    /// ライブラリ全体を探す（MCP の `search_library` と同じ規則）。このプロジェクトに入っているかを添える。
    Search,
    /// 商品の小さいサムネイル（PNG）。ほかの問い合わせを待たせないよう、受信口は別に答える。
    Thumb,
}

/// 問い合わせの答え。サムネイルだけ画像で返す。
pub(crate) enum BridgeReply {
    Json(Value),
    Png(Vec<u8>),
}

pub(crate) fn route(path: &str) -> Option<Route> {
    match path {
        "/unity/project" => Some(Route::Project),
        "/unity/scan" => Some(Route::Scan),
        "/unity/files" => Some(Route::Files),
        "/unity/check" => Some(Route::Check),
        "/unity/prepare" => Some(Route::Prepare),
        "/unity/search" => Some(Route::Search),
        "/unity/thumb" => Some(Route::Thumb),
        _ => None,
    }
}

impl Route {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Route::Project => "project",
            Route::Scan => "scan",
            Route::Files => "files",
            Route::Check => "check",
            Route::Prepare => "prepare",
            Route::Search => "search",
            Route::Thumb => "thumb",
        }
    }
}

// ---- 設定と認証コード ----

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnityBridgeStatus {
    pub enabled: bool,
    /// ALCOM のユーザーパッケージに足すフォルダ。オフのとき・用意できないときは空。
    pub folder: String,
    /// 用意したエディタ拡張の版（アプリと同じ）。
    pub version: String,
    /// 用意できなかったときの理由。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(crate) fn enabled(connection: &Connection) -> Result<bool, String> {
    Ok(db::setting(connection, ENABLED_KEY)?.as_deref() == Some("1"))
}

/// 保存した認証コード。無ければ作る。
fn token(connection: &Connection) -> Result<String, String> {
    if let Some(existing) = db::setting(connection, TOKEN_KEY)?
        && token_ok(&existing)
    {
        return Ok(existing);
    }
    let token = generate_token();
    db::set_setting(connection, TOKEN_KEY, &token)?;
    Ok(token)
}

pub(crate) fn stored_token(connection: &Connection) -> String {
    db::setting(connection, TOKEN_KEY)
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// 有効なときだけ認証コードのファイルを置き、無効なら消す。起動時と設定の変更のたびに呼ぶ。
pub(crate) fn sync_token_file(connection: &Connection, data_dir: &Path) -> Result<(), String> {
    let path = data_dir.join(TOKEN_FILE);
    if !enabled(connection)? {
        return match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("Unity の連携のファイルを消せません: {err}"))
            }
            _ => Ok(()),
        };
    }
    let token = token(connection)?;
    write_private(&path, &token).map_err(|err| format!("Unity の連携のファイルを書けません: {err}"))
}

pub(crate) fn save(connection: &mut Connection, data_dir: &Path, on: bool) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|err| format!("設定を保存できません: {err}"))?;
    db::set_setting(&transaction, ENABLED_KEY, if on { "1" } else { "0" })?;
    if on {
        token(&transaction)?;
    }
    transaction
        .commit()
        .map_err(|err| format!("設定を保存できません: {err}"))?;
    sync_token_file(connection, data_dir)
}

/// 認証コードを作り直す。開いている Unity の窓は、次の問い合わせで新しいファイルを読む。
pub(crate) fn regenerate(connection: &Connection, data_dir: &Path) -> Result<(), String> {
    db::set_setting(connection, TOKEN_KEY, &generate_token())?;
    sync_token_file(connection, data_dir)
}

// ---- 同梱したエディタ拡張の用意 ----

/// 前回写したファイルの一覧（新しい版に無いものだけを消すため）。
const FILE_LIST: &str = ".vrclibman-files";

fn bundled_package(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join(PACKAGE_FOLDER).join(PACKAGE_NAME));
    if let Some(dir) = bundled.filter(|dir| dir.join("package.json").is_file()) {
        return Some(dir);
    }
    // tauri dev ではリソースを写さないことがあるので、ソースのフォルダを使う。
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(PACKAGE_FOLDER)
        .join(PACKAGE_NAME);
    (cfg!(debug_assertions) && source.join("package.json").is_file()).then_some(source)
}

/// ALCOM のユーザーパッケージに足すフォルダ（アプリのローカルデータの `unity-package/app.vrclibman.unity`）。
pub(crate) fn package_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .map(|dir| dir.join(PACKAGE_FOLDER).join(PACKAGE_NAME))
        .map_err(|_| "アプリのデータフォルダが見つかりません".to_string())
}

pub(crate) fn package_version(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("name")?.as_str()? != PACKAGE_NAME {
        return None;
    }
    value.get("version")?.as_str().map(str::to_string)
}

/// `source` のパッケージを `target` へ写す。`package.json` を最後に置き換えるので、途中で失敗しても
/// 新しい版を名乗らない。前回写したファイルのうち新しい版に無いものだけを消し、ほかのファイルには触れない。
pub(crate) fn install_package(
    source: &Path,
    target: &Path,
    license: Option<&Path>,
) -> Result<(), String> {
    use crate::extension_package::{safe_relative, write_replacing};
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    if package_version(source).is_none() {
        return Err("同梱したエディタ拡張の版を読めません".into());
    }
    std::fs::create_dir_all(target).map_err(|_| "エディタ拡張のフォルダを作れません")?;
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(source)
        .follow_links(false)
        .min_depth(1)
    {
        let entry = entry.map_err(|_| "同梱したエディタ拡張を読めません")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| "同梱したエディタ拡張を読めません")?
            .to_path_buf();
        let hidden = relative
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'));
        if !hidden {
            files.push(relative);
        }
    }
    files.sort();
    let manifest = PathBuf::from("package.json");
    files.retain(|file| file != &manifest);
    let mut written = BTreeSet::new();
    for file in &files {
        let bytes =
            std::fs::read(source.join(file)).map_err(|_| "同梱したエディタ拡張を読めません")?;
        write_replacing(&target.join(file), &bytes)?;
        written.insert(file.clone());
    }
    if let Some(license) = license
        && let Ok(bytes) = std::fs::read(license)
    {
        write_replacing(&target.join("LICENSE"), &bytes)?;
        written.insert(PathBuf::from("LICENSE"));
    }
    let manifest_bytes =
        std::fs::read(source.join(&manifest)).map_err(|_| "同梱したエディタ拡張を読めません")?;
    write_replacing(&target.join(&manifest), &manifest_bytes)?;
    written.insert(manifest);
    let list_path = target.join(FILE_LIST);
    let previous = std::fs::read_to_string(&list_path).unwrap_or_default();
    for line in previous.lines() {
        let old = PathBuf::from(line.replace('\\', "/"));
        if !safe_relative(&old) || written.contains(&old) {
            continue;
        }
        let path = target.join(&old);
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_file()) {
            let _ = std::fs::remove_file(&path);
        }
    }
    let list: Vec<String> = written
        .iter()
        .map(|file| file.to_string_lossy().replace('\\', "/"))
        .collect();
    write_replacing(&list_path, format!("{}\n", list.join("\n")).as_bytes())
}

/// オンのとき、同梱した版を決まったフォルダへ用意する（版が違えば置き換える）。起動時と設定の変更時に呼ぶ。
/// オフにしてもフォルダは消さない（ALCOM が参照しているかもしれないため）。
pub(crate) fn status(app: &AppHandle, on: bool) -> UnityBridgeStatus {
    let bundled = bundled_package(app);
    let version = bundled
        .as_deref()
        .and_then(package_version)
        .unwrap_or_default();
    if !on {
        return UnityBridgeStatus {
            enabled: false,
            folder: String::new(),
            version,
            error: None,
        };
    }
    let prepared = (|| {
        let source =
            bundled.ok_or("同梱したエディタ拡張が見つかりません。アプリを入れ直してください")?;
        let target = package_dir(app)?;
        if package_version(&target).as_deref() != Some(version.as_str()) {
            let license = crate::extension_package::license_file(app);
            install_package(&source, &target, license.as_deref())?;
        }
        Ok::<_, String>(target.display().to_string())
    })();
    match prepared {
        Ok(folder) => UnityBridgeStatus {
            enabled: true,
            folder,
            version,
            error: None,
        },
        Err(error) => UnityBridgeStatus {
            enabled: true,
            folder: String::new(),
            version,
            error: Some(error),
        },
    }
}

// ---- 受信口での判定 ----

/// 受信口に `/unity/` が届いたときの判定。ブラウザは POST に Origin を必ず付けるので、
/// Origin のある要求はウェブページかブラウザ拡張であり、ここでは受けない。
pub(crate) fn gate(
    method: &str,
    path: &str,
    origin: Option<&str>,
    authorization: Option<&str>,
    enabled: bool,
    expected: &str,
    locked: bool,
) -> Result<Route, (u16, &'static str)> {
    if origin.is_some() {
        return Err((403, "Unity の連携は、ブラウザからは使えません"));
    }
    if !method.eq_ignore_ascii_case("POST") {
        return Err((405, "この操作には対応していません"));
    }
    if !enabled {
        return Err((
            403,
            "VRCLibMan の設定で「Unity の中から使う」をオンにしてください",
        ));
    }
    if locked {
        return Err((
            429,
            "接続の試行が多すぎます。30 秒待ってからもう一度お試しください",
        ));
    }
    let provided = authorization
        .map(str::trim)
        .and_then(|value| {
            value
                .strip_prefix("Bearer ")
                .or_else(|| value.strip_prefix("bearer "))
        })
        .map(str::trim)
        .unwrap_or("");
    if !token_ok(expected) || !constant_eq(provided.as_bytes(), expected.as_bytes()) {
        return Err((
            401,
            "認証コードが違います。VRCLibMan の設定で「Unity の中から使う」を確かめてください",
        ));
    }
    route(path.split('?').next().unwrap_or("")).ok_or((
        404,
        "この操作には対応していません。VRCLibMan とエディタ拡張の版をそろえてください",
    ))
}

// ---- 問い合わせ ----

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectBody {
    /// Unity のプロジェクトのフォルダ（`Application.dataPath` の親）。
    project: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilesBody {
    booth_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchBody {
    project: String,
    #[serde(default)]
    query: String,
    /// 空なら絞らない。`asset`・`world`・`avatar`。
    #[serde(default)]
    kind: String,
}

/// 検索で返す数の上限（MCP と同じ）。
const SEARCH_LIMIT: usize = 50;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageBody {
    project: String,
    /// 手元のファイル（`/unity/files` が返したもの）。ライブラリかリモートの中だけ受ける。
    path: String,
    #[serde(default)]
    entries: Vec<String>,
    #[serde(default)]
    booth_id: Option<String>,
}

fn bad_request(message: &str) -> (u16, String) {
    (400, message.to_string())
}

fn internal(error: String) -> (u16, String) {
    (500, error)
}

/// プロジェクトを一覧から探す。一覧に無いプロジェクトは調べない（任意のフォルダを歩かない）。
fn find_project(
    app: &AppHandle,
    folder: &str,
) -> Result<crate::unity_projects::UnityProject, (u16, String)> {
    if folder.trim().is_empty() {
        return Err(bad_request("プロジェクトのフォルダを読めません"));
    }
    let projects = crate::commands::unity_project_list_blocking(app)
        .map_err(internal)?
        .projects;
    let id = crate::unity_projects::project_id(folder, crate::unity::Os::current());
    projects
        .into_iter()
        .find(|project| project.id == id)
        .ok_or_else(|| {
            (
                404,
                "このプロジェクトは VRCLibMan のプロジェクトの一覧にありません。ALCOM に登録するか、VRCLibMan の「プロジェクト」で足してください"
                    .to_string(),
            )
        })
}

/// 版の知らせの言い方。画面（`src/unityProjects.ts` の `versionLines`）と同じ文。
pub(crate) fn version_text(note: &crate::unity_index::VersionNote) -> String {
    use crate::unity_index::VersionNote;
    match note {
        VersionNote::Older { newer } => {
            format!("古い版が入っています。新しいファイル（{newer}）があります")
        }
        VersionNote::Differs { files, versions } if *versions >= 2 => format!(
            "手元のどの版とも違うテクスチャ・モデルなどが {files} 個あります。改変したか、手元に無い版です"
        ),
        VersionNote::Differs { files, .. } => format!(
            "手元のファイルと違うテクスチャ・モデルなどが {files} 個あります。古い版か、改変した可能性があります"
        ),
    }
}

fn verdict_name(verdict: crate::unity_index::Verdict) -> &'static str {
    use crate::unity_index::Verdict;
    match verdict {
        Verdict::Installed => "installed",
        Verdict::Partial => "partial",
        Verdict::Missing => "missing",
        Verdict::Unknown => "unknown",
        Verdict::Bundled => "bundled",
    }
}

/// いまのプロジェクトについて言えること。調べていない・古い・途中までのときは「入っていない」を言わない。
fn project_reply(
    app: &AppHandle,
    project: &crate::unity_projects::UnityProject,
) -> Result<Value, (u16, String)> {
    let contents = crate::unity_index_jobs::read(app, |connection| {
        crate::unity_index_store::project_contents(connection, &project.id)
    })
    .map_err(internal)?;
    let view = crate::commands::project_contents_view(app, contents).map_err(internal)?;
    let products: Vec<Value> = view
        .products
        .iter()
        .filter(|product| {
            matches!(
                product.verdict,
                crate::unity_index::Verdict::Installed | crate::unity_index::Verdict::Partial
            )
        })
        .map(|product| {
            json!({
                "boothId": product.booth_id,
                "title": product.title,
                "verdict": verdict_name(product.verdict),
                "note": product.version.as_ref().map(version_text).unwrap_or_default(),
                "adjusted": product.adjusted,
            })
        })
        .collect();
    let suggestions = crate::commands::project_suggestions(app, project).map_err(internal)?;
    let missing: Vec<Value> = suggestions
        .items
        .iter()
        .map(|item| {
            json!({
                "boothId": item.booth_id,
                "title": item.title,
                "files": item.local_file_count,
            })
        })
        .collect();
    Ok(json!({
        "project": {
            "name": project.name,
            "version": project.version,
            "kind": project.kind,
            "avatars": project.avatars.iter().map(|avatar| avatar.name.clone()).collect::<Vec<_>>(),
        },
        "scannedAt": view.scanned_at.unwrap_or(0),
        "complete": view.complete,
        "stale": view.stale,
        "products": products,
        "missing": missing,
        "missingKnown": suggestions.scanned,
    }))
}

/// `finish_files` の結果のうち、手元にあって中に Unity パッケージがあるファイルだけ。Unity の窓が
/// `JsonUtility` で読めるよう、省略や null の無い形にする。
pub(crate) fn importable_files(listed: &Value) -> Value {
    let files: Vec<Value> = listed
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| {
            let path = file.get("path")?.as_str()?;
            let packages: Vec<Value> = file
                .get("unityPackages")?
                .as_array()?
                .iter()
                .map(|package| {
                    json!({
                        "entry": package.get("entry").and_then(Value::as_str).unwrap_or(""),
                        "name": package.get("name").and_then(Value::as_str).unwrap_or(""),
                        "size": package.get("size").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect();
            if packages.is_empty() {
                return None;
            }
            Some(json!({
                "filename": file.get("filename").and_then(Value::as_str).unwrap_or(""),
                "variation": file.get("variation").and_then(Value::as_str).unwrap_or(""),
                "path": path,
                "packages": packages,
            }))
        })
        .collect();
    json!({
        "title": listed.get("title").and_then(Value::as_str).unwrap_or(""),
        "files": files,
    })
}

// ---- サムネイル ----

/// Unity の窓に渡すサムネイルの一辺（ピクセル）。
const THUMB_SIZE: u32 = 96;
/// 覚えておくサムネイルの数。超えたら全部捨てて覚え直す。
const THUMB_CACHE: usize = 500;

static THUMBS: std::sync::Mutex<Option<std::collections::HashMap<String, Vec<u8>>>> =
    std::sync::Mutex::new(None);

/// 商品の小さいサムネイルを PNG で返す。保存してあればそれを、無ければアプリの画面と同じく
/// BOOTH の画像サーバーから Cookie なしで取る（保存はしない）。Unity は webp を読めないので、
/// どれも縮めて PNG にする。年齢制限の商品と除外した商品は返さない。
fn thumbnail(app: &AppHandle, booth_id: &str) -> Result<Vec<u8>, (u16, String)> {
    use tauri::Manager;
    if !mcp_api::valid_id(booth_id) {
        return Err(bad_request("商品IDを読めません"));
    }
    if let Some(bytes) = THUMBS
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref().and_then(|map| map.get(booth_id).cloned()))
    {
        return Ok(bytes);
    }
    let state = app.state::<crate::AppState>();
    let (saved, remote) = {
        let connection = state
            .db
            .lock()
            .map_err(|_| internal("データベースをロックできません".into()))?;
        let row: Option<(i64, Option<String>, Option<String>, i64)> = connection
            .query_row(
                "SELECT IFNULL(is_adult, 0), thumbnail_path, thumbnail_url,
                        EXISTS (SELECT 1 FROM product_exclusions e WHERE e.booth_id = products.booth_id)
                 FROM products WHERE booth_id = ?1",
                [booth_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .ok();
        let Some((adult, saved, remote, excluded)) = row else {
            return Err((404, "この商品はライブラリにありません".into()));
        };
        if adult != 0 || excluded != 0 {
            return Err((404, "サムネイルはありません".into()));
        }
        (saved, remote)
    };
    let saved_bytes = saved
        .map(std::path::PathBuf::from)
        .filter(|path| path.starts_with(&state.thumbs_dir))
        .and_then(|path| std::fs::read(path).ok());
    let bytes = match saved_bytes {
        Some(bytes) => bytes,
        None => {
            let url = remote
                .filter(|url| !url.trim().is_empty())
                .ok_or_else(|| (404, "サムネイルはありません".to_string()))?;
            let client = crate::thumb::download_client().map_err(internal)?;
            tauri::async_runtime::block_on(crate::thumb::fetch_card_image(&client, &url))
                .map_err(|error| (502, error))?
                .0
        }
    };
    let png = small_png(&bytes).ok_or_else(|| (415, "サムネイルを読めません".to_string()))?;
    if let Ok(mut cache) = THUMBS.lock() {
        let map = cache.get_or_insert_with(std::collections::HashMap::new);
        if map.len() >= THUMB_CACHE {
            map.clear();
        }
        map.insert(booth_id.to_string(), png.clone());
    }
    Ok(png)
}

/// 画像（png・jpeg・webp）を一辺 `THUMB_SIZE` 以下に縮めて PNG にする。
pub(crate) fn small_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let small = image.thumbnail(THUMB_SIZE, THUMB_SIZE);
    let mut out = std::io::Cursor::new(Vec::new());
    small.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// MCP の検索の結果を、Unity の窓が `JsonUtility` で読める形にする（null と省略を無くす）。
/// `installed` は、このプロジェクトに入っている商品の判定（`installed`・`partial`）。
pub(crate) fn search_items(
    found: &Value,
    installed: &std::collections::HashMap<String, &str>,
) -> Value {
    let text = |item: &Value, key: &str| {
        item.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let flag = |item: &Value, key: &str| item.get(key).and_then(Value::as_bool).unwrap_or(false);
    let items: Vec<Value> = found
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| {
            let booth_id = text(item, "boothId");
            let avatars: Vec<String> = item
                .get("compatibleAvatars")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            json!({
                "boothId": booth_id,
                "title": text(item, "title"),
                "shop": text(item, "shop"),
                "category": text(item, "category"),
                "genre": text(item, "genre"),
                "avatars": avatars,
                "generic": flag(item, "generic"),
                "hasFiles": flag(item, "hasLocalFiles"),
                "adult": flag(item, "adult"),
                "installed": installed.get(&booth_id).copied().unwrap_or(""),
            })
        })
        .collect();
    json!({
        "total": found.get("total").and_then(Value::as_u64).unwrap_or(0),
        "items": items,
    })
}

/// 受信口がデータベースのロックの外で呼ぶ。プロジェクトの一覧・索引・zip を読む。
pub(crate) fn handle(
    app: &AppHandle,
    route: Route,
    body: &str,
) -> Result<BridgeReply, (u16, String)> {
    if route == Route::Thumb {
        let body: FilesBody =
            serde_json::from_str(body).map_err(|_| bad_request("商品IDを読めません"))?;
        return thumbnail(app, body.booth_id.trim()).map(BridgeReply::Png);
    }
    handle_json(app, route, body).map(BridgeReply::Json)
}

fn handle_json(app: &AppHandle, route: Route, body: &str) -> Result<Value, (u16, String)> {
    match route {
        Route::Thumb => Err(bad_request("この操作には対応していません")),
        Route::Project => {
            let body: ProjectBody = serde_json::from_str(body)
                .map_err(|_| bad_request("プロジェクトのフォルダを読めません"))?;
            let project = find_project(app, &body.project)?;
            project_reply(app, &project)
        }
        Route::Scan => {
            let body: ProjectBody = serde_json::from_str(body)
                .map_err(|_| bad_request("プロジェクトのフォルダを読めません"))?;
            let project = find_project(app, &body.project)?;
            if project.state != "ok" {
                return Err((409, "プロジェクトのフォルダを読めません".to_string()));
            }
            let vpm: Vec<(String, String)> = project
                .packages
                .iter()
                .map(|package| (package.name.clone(), package.version.clone()))
                .collect();
            crate::unity_index_jobs::scan_project(app, &project.id, Path::new(&project.path), &vpm)
                .map_err(internal)?;
            project_reply(app, &project)
        }
        Route::Files => {
            let body: FilesBody =
                serde_json::from_str(body).map_err(|_| bad_request("商品IDを読めません"))?;
            let id = body.booth_id.trim().to_string();
            if !mcp_api::valid_id(&id) {
                return Err(bad_request("商品IDを読めません"));
            }
            let reply = {
                use tauri::Manager;
                let state = app.state::<crate::AppState>();
                let connection = state
                    .db
                    .lock()
                    .map_err(|_| internal("データベースをロックできません".into()))?;
                // 自分の PC の Unity の窓なので、年齢制限の商品もアプリの画面と同じく並べる。
                mcp_api::product_files(&connection, id, true).map_err(internal)?
            };
            match reply {
                mcp_api::Reply::Files(pending) => {
                    Ok(importable_files(&mcp_api::finish_files(pending)))
                }
                mcp_api::Reply::Json { status, body } => Err((
                    status,
                    body.get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("ファイルを読めません")
                        .to_string(),
                )),
                _ => Err(internal("ファイルを読めません".into())),
            }
        }
        Route::Check => {
            let body: PackageBody =
                serde_json::from_str(body).map_err(|_| bad_request("指定を読めません"))?;
            let project = find_project(app, &body.project)?;
            let check = crate::commands::package_check(
                app,
                &project,
                &body.path,
                &body.entries,
                body.booth_id.as_deref(),
            )
            .map_err(internal)?;
            // 言うことが無いときも同じ形で返す（Unity の `JsonUtility` は null を読めない）。
            Ok(check
                .and_then(|check| serde_json::to_value(check).ok())
                .unwrap_or_else(|| json!({ "installed": false, "missing": [], "bundled": [] })))
        }
        Route::Search => {
            let body: SearchBody =
                serde_json::from_str(body).map_err(|_| bad_request("検索の指定を読めません"))?;
            let query = body.query.trim();
            if query.is_empty() {
                return Err(bad_request("探す言葉を入れてください"));
            }
            let kind = match body.kind.trim() {
                "" => None,
                kind @ ("asset" | "world" | "avatar") => Some(kind),
                _ => return Err(bad_request("種類を読めません")),
            };
            // 打つたびに呼ばれるので、プロジェクトの一覧は読み直さない。フォルダから id を決め、
            // 索引の結果を読むだけ（プロジェクトのフォルダは歩かない）。
            if body.project.trim().is_empty() {
                return Err(bad_request("プロジェクトのフォルダを読めません"));
            }
            let project_id =
                crate::unity_projects::project_id(&body.project, crate::unity::Os::current());
            let reply = {
                use tauri::Manager;
                let state = app.state::<crate::AppState>();
                let connection = state
                    .db
                    .lock()
                    .map_err(|_| internal("データベースをロックできません".into()))?;
                // 自分の PC の Unity の窓なので、年齢制限の商品もアプリの画面と同じく並べる。
                mcp_api::search_library(&connection, query, kind, SEARCH_LIMIT, true)
                    .map_err(internal)?
            };
            let found = match reply {
                mcp_api::Reply::Json { status: 200, body } => body,
                mcp_api::Reply::Json { status, body } => {
                    return Err((
                        status,
                        body.get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("探せませんでした")
                            .to_string(),
                    ));
                }
                _ => return Err(internal("探せませんでした".into())),
            };
            // このプロジェクトに入っているか（前に調べた結果。古い・調べていないときは何も言わない）。
            let contents = crate::unity_index_jobs::read(app, |connection| {
                crate::unity_index_store::project_contents(connection, &project_id)
            })
            .map_err(internal)?;
            let mut installed = std::collections::HashMap::new();
            if contents.scanned_at.is_some() && !contents.stale {
                for product in &contents.products {
                    if matches!(
                        product.verdict,
                        crate::unity_index::Verdict::Installed
                            | crate::unity_index::Verdict::Partial
                    ) {
                        installed.insert(product.booth_id.clone(), verdict_name(product.verdict));
                    }
                }
            }
            Ok(search_items(&found, &installed))
        }
        Route::Prepare => {
            let body: PackageBody =
                serde_json::from_str(body).map_err(|_| bad_request("指定を読めません"))?;
            let project = find_project(app, &body.project)?;
            if body
                .booth_id
                .as_deref()
                .is_some_and(|id| !mcp_api::valid_id(id))
            {
                return Err(bad_request("商品IDを読めません"));
            }
            let prepared = crate::commands::prepare_packages(
                app,
                &body.path,
                &body.entries,
                body.booth_id.as_deref(),
            )
            .map_err(|error| (409, error))?;
            // Import 画面で取り込まれたかもしれないので、このプロジェクトの結果を古いとする。
            crate::unity_index_jobs::mark_project_stale(app, project.id.clone());
            Ok(json!({ "packages": prepared }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn gate_takes_only_the_unity_code_without_an_origin() {
        let bearer = format!("Bearer {CODE}");
        let ok = |path: &str| gate("POST", path, None, Some(&bearer), true, CODE, false);
        assert_eq!(ok("/unity/project"), Ok(Route::Project));
        assert_eq!(ok("/unity/prepare?x=1"), Ok(Route::Prepare));
        assert_eq!(ok("/unity/other").unwrap_err().0, 404);
        // ウェブページとブラウザ拡張（Origin がある）は、コードが合っていても断る。
        assert_eq!(
            gate(
                "POST",
                "/unity/project",
                Some("https://example.com"),
                Some(&bearer),
                true,
                CODE,
                false
            )
            .unwrap_err()
            .0,
            403
        );
        assert_eq!(
            gate(
                "GET",
                "/unity/project",
                None,
                Some(&bearer),
                true,
                CODE,
                false
            )
            .unwrap_err()
            .0,
            405
        );
        // オフ・コード違い・コード未作成・試行が多すぎる。
        assert_eq!(
            gate(
                "POST",
                "/unity/project",
                None,
                Some(&bearer),
                false,
                CODE,
                false
            )
            .unwrap_err()
            .0,
            403
        );
        assert_eq!(
            gate(
                "POST",
                "/unity/project",
                None,
                Some("Bearer nope"),
                true,
                CODE,
                false
            )
            .unwrap_err()
            .0,
            401
        );
        assert_eq!(
            gate(
                "POST",
                "/unity/project",
                None,
                Some(&bearer),
                true,
                "",
                false
            )
            .unwrap_err()
            .0,
            401
        );
        assert_eq!(
            gate(
                "POST",
                "/unity/project",
                None,
                Some(&bearer),
                true,
                CODE,
                true
            )
            .unwrap_err()
            .0,
            429
        );
    }

    #[test]
    fn token_file_follows_the_switch() {
        let dir = tempfile::tempdir().unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        save(&mut connection, dir.path(), true).unwrap();
        let written = std::fs::read_to_string(dir.path().join(TOKEN_FILE)).unwrap();
        assert!(token_ok(&written));
        assert_eq!(written, stored_token(&connection));
        // MCP のコードとは別のもの。
        assert_ne!(
            dir.path().join(TOKEN_FILE),
            dir.path().join(mcp_api::TOKEN_FILE)
        );
        regenerate(&connection, dir.path()).unwrap();
        let renewed = std::fs::read_to_string(dir.path().join(TOKEN_FILE)).unwrap();
        assert_ne!(written, renewed);
        save(&mut connection, dir.path(), false).unwrap();
        assert!(!dir.path().join(TOKEN_FILE).exists());
    }

    #[test]
    fn importable_files_keep_only_local_packages_without_nulls() {
        let listed = json!({
            "boothId": "1234",
            "title": "衣装",
            "files": [
                { "filename": "a.zip", "variation": "", "sizeBytes": null, "state": "local",
                  "path": "L/1234/a.zip",
                  "unityPackages": [{ "entry": "x/a.unitypackage", "name": "a.unitypackage", "size": 10 }] },
                { "filename": "b.zip", "variation": "", "sizeBytes": 5, "state": "notDownloaded" },
                { "filename": "readme.pdf", "variation": "", "sizeBytes": 5, "state": "local", "path": "L/1234/readme.pdf" },
                { "filename": "c.zip", "variation": "", "sizeBytes": 5, "state": "local", "path": "L/1234/c.zip", "unityPackages": [] }
            ]
        });
        let files = importable_files(&listed);
        assert_eq!(files["title"], "衣装");
        let list = files["files"].as_array().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["path"], "L/1234/a.zip");
        assert_eq!(list[0]["packages"][0]["entry"], "x/a.unitypackage");
        assert!(!files.to_string().contains("null"));
    }

    #[test]
    fn the_bundled_package_is_copied_and_updated_in_place() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("bundle");
        let target = root.path().join("data").join(PACKAGE_NAME);
        let license = root.path().join("LICENSE");
        std::fs::write(&license, "MPL").unwrap();
        let bundle = |version: &str, files: &[(&str, &str)]| {
            std::fs::create_dir_all(source.join("Editor")).unwrap();
            std::fs::write(
                source.join("package.json"),
                format!(r#"{{"name":"{PACKAGE_NAME}","version":"{version}"}}"#),
            )
            .unwrap();
            for (name, text) in files {
                std::fs::write(source.join(name), text).unwrap();
            }
        };
        bundle("0.9.2", &[("Editor/A.cs", "old"), ("Editor/Gone.cs", "x")]);
        install_package(&source, &target, Some(&license)).unwrap();
        assert_eq!(package_version(&target).as_deref(), Some("0.9.2"));
        assert!(target.join("LICENSE").is_file());
        assert!(target.join("Editor/Gone.cs").is_file());
        // 利用者が置いたファイルは消さず、前回写して新しい版に無いファイルだけを消す。
        std::fs::write(target.join("notes.txt"), "mine").unwrap();
        std::fs::remove_file(source.join("Editor/Gone.cs")).unwrap();
        bundle("0.9.3", &[("Editor/A.cs", "new")]);
        install_package(&source, &target, Some(&license)).unwrap();
        assert_eq!(package_version(&target).as_deref(), Some("0.9.3"));
        assert_eq!(
            std::fs::read_to_string(target.join("Editor/A.cs")).unwrap(),
            "new"
        );
        assert!(!target.join("Editor/Gone.cs").exists());
        assert!(target.join("notes.txt").is_file());
        // ほかのパッケージの package.json は、この拡張の版として読まない。
        std::fs::write(
            source.join("package.json"),
            r#"{"name":"other","version":"1.0.0"}"#,
        )
        .unwrap();
        assert!(install_package(&source, &target, None).is_err());
    }

    #[test]
    fn the_package_in_the_repository_matches_the_app_version() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(PACKAGE_FOLDER)
            .join(PACKAGE_NAME);
        assert_eq!(
            package_version(&dir).as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        // Unity は、パッケージのファイルに .meta が無いと読み込みのたびに作ろうとする。
        for entry in walkdir::WalkDir::new(&dir).min_depth(1) {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".meta") || name.starts_with('.') {
                continue;
            }
            let meta = entry.path().with_file_name(format!("{name}.meta"));
            assert!(meta.is_file(), "{name} has no .meta");
        }
        // LICENSE は用意するときに写すので、.meta だけを置いている。
        assert!(dir.join("LICENSE.meta").is_file());
    }

    #[test]
    fn search_items_have_no_nulls_and_mark_what_is_installed() {
        let found = json!({
            "total": 3,
            "returned": 2,
            "items": [
                { "boothId": "1111", "title": "髪", "shop": "店", "category": "hair", "genre": null,
                  "compatibleAvatars": ["しなの"], "generic": false, "hasLocalFiles": true },
                { "boothId": "2222", "title": "R", "shop": null, "category": null, "genre": null,
                  "compatibleAvatars": [], "generic": true, "hasLocalFiles": false, "adult": true }
            ]
        });
        let installed = std::collections::HashMap::from([("1111".to_string(), "installed")]);
        let items = search_items(&found, &installed);
        assert_eq!(items["total"], 3);
        assert_eq!(items["items"][0]["installed"], "installed");
        assert_eq!(items["items"][0]["avatars"][0], "しなの");
        assert_eq!(items["items"][0]["hasFiles"], true);
        assert_eq!(items["items"][1]["installed"], "");
        assert_eq!(items["items"][1]["adult"], true);
        assert!(!items.to_string().contains("null"));
        assert_eq!(route("/unity/search"), Some(Route::Search));
    }

    #[test]
    fn thumbnails_become_small_pngs() {
        let mut big = image::RgbImage::new(300, 200);
        big.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(big)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        let png = small_png(jpeg.get_ref()).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let decoded = image::load_from_memory(&png).unwrap();
        assert!(decoded.width() <= THUMB_SIZE && decoded.height() <= THUMB_SIZE);
        assert_eq!(decoded.width(), THUMB_SIZE);
        assert!(small_png(b"not an image").is_none());
        assert_eq!(route("/unity/thumb"), Some(Route::Thumb));
    }

    #[test]
    fn version_text_matches_the_screen() {
        use crate::unity_index::VersionNote;
        assert!(
            version_text(&VersionNote::Older {
                newer: "v2.zip".into()
            })
            .contains("v2.zip")
        );
        assert!(
            version_text(&VersionNote::Differs {
                files: 2,
                versions: 1
            })
            .contains("古い版か、改変した可能性")
        );
        assert!(
            version_text(&VersionNote::Differs {
                files: 1,
                versions: 3
            })
            .contains("どの版とも違う")
        );
    }
}
