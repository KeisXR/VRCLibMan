//! AI エージェント連携（MCP）の問い合わせ。`VRCLibMan --mcp`（`mcp_stdio`）が受信口の `/mcp/*` へ
//! 送ってきたものに答える。読むだけで、データベースにもファイルにも書かない。
//! 返さないもの: 除外した商品、設定で許していない R-18 の商品、R-18 の説明、URL、金額、
//! トークン、ライブラリかリモートのフォルダの外のパス。並べ方と分け方は画面と同じ `view` を使う。
use crate::model::{Category, Product};
use crate::{db, file_browser, unity_import, view};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// アプリデータに置く、`--mcp` のプロセスが読む認証コードのファイル名。
pub(crate) const TOKEN_FILE: &str = "mcp-token";
const ENABLED_KEY: &str = "mcp_enabled";
const ADULT_KEY: &str = "mcp_include_adult";
/// AI エージェントからの 1 ファイルのダウンロードを許すか（既定オフ）。
const DOWNLOAD_KEY: &str = "mcp_allow_download";
const TOKEN_KEY: &str = "mcp_token";
const SNIPPET_CHARS: usize = 300;
const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 50;
/// 1 回の `product_files` で目次を読む zip の数。受信口は 1 本のスレッドなので長く止めない。
const MAX_LISTED_ARCHIVES: usize = 8;
pub(crate) const BODY_LIMIT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Search,
    Avatars,
    AvatarAssets,
    ProductFiles,
    /// 設定で許したときだけ。1 回の呼び出しで 1 ファイル。
    DownloadFile,
    /// `tools/list` のために、今使えるツールを返す。
    Capabilities,
}

pub(crate) fn route(path: &str) -> Option<Route> {
    match path {
        "/mcp/search_library" => Some(Route::Search),
        "/mcp/list_avatars" => Some(Route::Avatars),
        "/mcp/avatar_assets" => Some(Route::AvatarAssets),
        "/mcp/product_files" => Some(Route::ProductFiles),
        "/mcp/download_file" => Some(Route::DownloadFile),
        "/mcp/capabilities" => Some(Route::Capabilities),
        _ => None,
    }
}

impl Route {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Route::Search => "search_library",
            Route::Avatars => "list_avatars",
            Route::AvatarAssets => "avatar_assets",
            Route::ProductFiles => "product_files",
            Route::DownloadFile => "download_file",
            Route::Capabilities => "capabilities",
        }
    }
}

// ---- 設定と認証コード ----

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub enabled: bool,
    pub include_adult: bool,
    pub allow_download: bool,
    /// エージェントの MCP 設定に書く実行ファイル。分からないときは空。
    pub command: String,
}

fn flag(connection: &Connection, key: &str) -> Result<bool, String> {
    Ok(db::setting(connection, key)?.as_deref() == Some("1"))
}

pub(crate) fn enabled(connection: &Connection) -> Result<bool, String> {
    flag(connection, ENABLED_KEY)
}

pub(crate) fn include_adult(connection: &Connection) -> Result<bool, String> {
    flag(connection, ADULT_KEY)
}

pub(crate) fn allow_download(connection: &Connection) -> Result<bool, String> {
    flag(connection, DOWNLOAD_KEY)
}

pub(crate) fn status(connection: &Connection) -> Result<McpStatus, String> {
    Ok(McpStatus {
        enabled: enabled(connection)?,
        include_adult: include_adult(connection)?,
        allow_download: allow_download(connection)?,
        command: command_path(),
    })
}

/// エージェントが起動する実行ファイル。AppImage は展開先が起動のたびに変わるので、外側のファイルを使う。
pub(crate) fn command_path() -> String {
    #[cfg(target_os = "linux")]
    if let Some(image) = std::env::var_os("APPIMAGE").filter(|value| !value.is_empty()) {
        return std::path::PathBuf::from(image).display().to_string();
    }
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_default()
}

fn token_ok(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn generate_token() -> String {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).expect("OS entropy");
    buf.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 保存した認証コード。無ければ作る。
pub(crate) fn token(connection: &Connection) -> Result<String, String> {
    if let Some(existing) = db::setting(connection, TOKEN_KEY)?
        && token_ok(&existing)
    {
        return Ok(existing);
    }
    let token = generate_token();
    db::set_setting(connection, TOKEN_KEY, &token)?;
    Ok(token)
}

/// 有効なときだけ認証コードのファイルを置き、無効なら消す。起動時と設定の変更のたびに呼ぶ。
pub(crate) fn sync_token_file(connection: &Connection, data_dir: &Path) -> Result<(), String> {
    let path = data_dir.join(TOKEN_FILE);
    if !enabled(connection)? {
        return match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("AI エージェント連携のファイルを消せません: {err}"))
            }
            _ => Ok(()),
        };
    }
    let token = token(connection)?;
    write_private(&path, &token)
        .map_err(|err| format!("AI エージェント連携のファイルを書けません: {err}"))
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)
}

pub(crate) fn save(
    connection: &mut Connection,
    data_dir: &Path,
    enabled: bool,
    include_adult: bool,
    allow_download: bool,
) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|err| format!("設定を保存できません: {err}"))?;
    db::set_setting(&transaction, ENABLED_KEY, if enabled { "1" } else { "0" })?;
    db::set_setting(
        &transaction,
        ADULT_KEY,
        if include_adult { "1" } else { "0" },
    )?;
    db::set_setting(
        &transaction,
        DOWNLOAD_KEY,
        if allow_download { "1" } else { "0" },
    )?;
    if enabled {
        token(&transaction)?;
    }
    transaction
        .commit()
        .map_err(|err| format!("設定を保存できません: {err}"))?;
    sync_token_file(connection, data_dir)
}

/// 認証コードを作り直す。今つながっているエージェントは、次の問い合わせで新しいファイルを読む。
pub(crate) fn regenerate(connection: &Connection, data_dir: &Path) -> Result<(), String> {
    db::set_setting(connection, TOKEN_KEY, &generate_token())?;
    sync_token_file(connection, data_dir)
}

// ---- 受信口での判定 ----

/// 受信口に `/mcp/` が届いたときの判定。ブラウザは POST に Origin を必ず付けるので、
/// Origin のある要求はウェブページか拡張であり、ここでは受けない。
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
        return Err((403, "AI エージェント連携は、ブラウザからは使えません"));
    }
    if !method.eq_ignore_ascii_case("POST") {
        return Err((405, "この操作には対応していません"));
    }
    if !enabled {
        return Err((
            403,
            "VRCLibMan の設定で「AI エージェント連携」をオンにしてください",
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
            "認証コードが違います。VRCLibMan の設定で AI エージェント連携を確かめてください",
        ));
    }
    let path = path.split('?').next().unwrap_or("");
    route(path).ok_or((
        404,
        "この操作には対応していません。アプリを更新してください",
    ))
}

fn constant_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = u8::from(left.len() != right.len());
    for index in 0..left.len().max(right.len()) {
        diff |= left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0);
    }
    diff == 0
}

// ---- 問い合わせ ----

/// `handle` の結果。ファイルの一覧は、データベースのロックを外してからパスを確かめる。
#[derive(Debug)]
pub(crate) enum Reply {
    Json {
        status: u16,
        body: Value,
    },
    Files(PendingFiles),
    /// 受信口がデータベースのロックを外してから、`browser::download_one_file` で受け取る。
    Download(DownloadPlan),
}

/// AI エージェントから頼まれた 1 ファイル。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DownloadPlan {
    pub booth_id: String,
    pub variation: String,
    pub filename: String,
}

impl Reply {
    fn ok(body: Value) -> Self {
        Reply::Json { status: 200, body }
    }

    fn error(status: u16, message: &str) -> Self {
        Reply::Json {
            status,
            body: json!({ "error": message }),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PendingFiles {
    booth_id: String,
    title: String,
    library: String,
    remote: String,
    /// この商品のダウンロード行とファイルだけを残した写し。
    snapshot: view::Snapshot,
}

struct FileRow {
    filename: String,
    variation: String,
    size: Option<u64>,
    /// 実在を確かめた場所と、その種類（`local` か `remote`）。
    found: Option<(String, &'static str)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchBody {
    #[serde(default)]
    query: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    genre: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AvatarBody {
    avatar: String,
    #[serde(default)]
    genre: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct DownloadBody {
    booth_id: String,
    filename: String,
    #[serde(default)]
    variation: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct FilesBody {
    booth_id: String,
}

fn valid_id(id: &str) -> bool {
    (4..=12).contains(&id.len()) && id.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn handle(connection: &Connection, route: Route, body: &str) -> Result<Reply, String> {
    let adult_ok = include_adult(connection)?;
    let body = if body.trim().is_empty() { "{}" } else { body };
    match route {
        Route::Capabilities => Ok(Reply::ok(json!({
            "downloadFile": allow_download(connection)?,
        }))),
        Route::DownloadFile => {
            if !allow_download(connection)? {
                return Ok(Reply::error(
                    403,
                    "VRCLibMan の設定で「AI エージェントからのダウンロードを許可」がオフです",
                ));
            }
            let Ok(body) = serde_json::from_str::<DownloadBody>(body) else {
                return Ok(Reply::error(400, "ダウンロードの指定を読めません"));
            };
            plan_download(connection, body)
        }
        Route::Search => {
            let Ok(body) = serde_json::from_str::<SearchBody>(body) else {
                return Ok(Reply::error(400, "検索の指定を読めません"));
            };
            search(connection, body, adult_ok)
        }
        Route::Avatars => {
            let snapshot = db::load_snapshot(connection)?;
            let avatars: Vec<Value> = view::panel_avatars(&snapshot)
                .into_iter()
                .map(|avatar| {
                    let record = snapshot.avatars.iter().find(|row| row.id == avatar.id);
                    let base = record
                        .and_then(|row| row.base_body.as_deref())
                        .and_then(|id| snapshot.base_bodies.iter().find(|body| body.id == id))
                        .map(|body| body.display_name.clone());
                    json!({
                        "id": avatar.id,
                        "name": avatar.display_name,
                        "aliases": record.map(|row| row.aliases.clone()).unwrap_or_default(),
                        "owned": record.is_some_and(|row| row.owned),
                        "baseBody": base,
                        "itemCount": avatar.item_count,
                    })
                })
                .collect();
            Ok(Reply::ok(json!({ "avatars": avatars })))
        }
        Route::AvatarAssets => {
            let Ok(body) = serde_json::from_str::<AvatarBody>(body) else {
                return Ok(Reply::error(400, "アバターの指定を読めません"));
            };
            let snapshot = db::load_snapshot(connection)?;
            let Some(avatar) = find_avatar(&snapshot, &body.avatar) else {
                return Ok(Reply::error(
                    404,
                    "アバターが見つかりません。list_avatars で名前を確かめてください",
                ));
            };
            let Some(sections) = view::panel_avatar_sections(&snapshot, &avatar.id) else {
                return Ok(Reply::error(404, "アバターが見つかりません"));
            };
            let facts = Facts::new(&snapshot);
            let wanted = body
                .genre
                .as_deref()
                .map(str::trim)
                .filter(|g| !g.is_empty());
            let sections: Vec<Value> = sections
                .into_iter()
                .filter(|section| wanted.is_none_or(|g| section.key == g || section.label == g))
                .map(|section| {
                    let items: Vec<Value> = section
                        .items
                        .iter()
                        .filter(|item| adult_ok || !item.is_adult)
                        .filter_map(|item| facts.products.get(item.booth_id.as_str()))
                        .map(|product| {
                            let mut value = facts.item(product);
                            if let Some(via) = section
                                .items
                                .iter()
                                .find(|item| item.booth_id == product.booth_id)
                                .map(|item| item.via_base_body)
                            {
                                value["viaBaseBody"] = json!(via);
                            }
                            value
                        })
                        .collect();
                    json!({ "key": section.key, "label": section.label, "items": items })
                })
                .filter(|section| {
                    section["items"]
                        .as_array()
                        .is_some_and(|items| !items.is_empty())
                })
                .collect();
            Ok(Reply::ok(json!({
                "avatar": { "id": avatar.id, "name": avatar.display_name },
                "sections": sections,
            })))
        }
        Route::ProductFiles => {
            let Some(id) = serde_json::from_str::<FilesBody>(body)
                .ok()
                .map(|body| body.booth_id)
                .filter(|id| valid_id(id))
            else {
                return Ok(Reply::error(400, "商品IDを読めません"));
            };
            let snapshot = db::load_snapshot(connection)?;
            let facts = Facts::new(&snapshot);
            let Some(product) = facts.products.get(id.as_str()) else {
                return Ok(Reply::error(404, "この商品はライブラリにありません"));
            };
            if !adult_ok && facts.adult.contains(id.as_str()) {
                return Ok(Reply::error(404, "この商品はライブラリにありません"));
            }
            let settings = db::settings(connection)?;
            let title = product.title.clone();
            drop(facts);
            // 場所の判定は画面と同じ `library_files::fill_places`。この商品の行だけを残して渡す。
            let mut snapshot = snapshot;
            snapshot.download_rows.retain(|row| row.booth_id == id);
            snapshot
                .files
                .retain(|file| !file.is_dir && file.linked_product_id.as_deref() == Some(&id));
            Ok(Reply::Files(PendingFiles {
                booth_id: id,
                title,
                library: settings.library_folder,
                remote: settings.remote_library_folder,
                snapshot,
            }))
        }
    }
}

/// 頼まれた 1 ファイルを、ライブラリのファイル行から 1 つに決める。R-18 と除外した商品は、設定によらず受けない。
fn plan_download(connection: &Connection, body: DownloadBody) -> Result<Reply, String> {
    let id = body.booth_id.trim().to_string();
    if !valid_id(&id) {
        return Ok(Reply::error(400, "商品IDを読めません"));
    }
    let snapshot = db::load_snapshot(connection)?;
    let facts = Facts::new(&snapshot);
    if !facts.products.contains_key(id.as_str()) {
        return Ok(Reply::error(404, "この商品はライブラリにありません"));
    }
    if facts.adult.contains(id.as_str()) {
        return Ok(Reply::error(
            403,
            "年齢制限のある商品は、AI エージェントからはダウンロードできません。VRCLibMan の画面から取得してください",
        ));
    }
    let filename = body.filename.trim();
    let wanted = body.variation.as_deref().map(str::trim);
    let rows: Vec<_> = snapshot
        .download_rows
        .iter()
        .filter(|row| row.booth_id == id && row.filename == filename)
        .filter(|row| wanted.is_none_or(|variation| row.variation == variation))
        .collect();
    match rows.as_slice() {
        [] => Ok(Reply::error(
            404,
            "そのファイルはこの商品のダウンロード一覧にありません。product_files で名前を確かめてください",
        )),
        [row] => Ok(Reply::Download(DownloadPlan {
            booth_id: id,
            variation: row.variation.clone(),
            filename: row.filename.clone(),
        })),
        _ => Ok(Reply::error(
            400,
            "同じ名前のファイルが複数のバリエーションにあります。variation を指定してください",
        )),
    }
}

/// 受け取ったファイルの場所と、中の Unity パッケージ。
pub(crate) fn downloaded(plan: &DownloadPlan, path: &str) -> Value {
    let mut value = json!({
        "boothId": plan.booth_id,
        "filename": plan.filename,
        "variation": plan.variation,
        "path": path,
    });
    let file = Path::new(path);
    if has_extension(file, "unitypackage") || has_extension(file, "zip") {
        match unity_import::list_packages(file) {
            Ok(list) => value["unityPackages"] = json!(list.packages),
            Err(error) => value["unityPackagesError"] = json!(error),
        }
    }
    value
}

/// データベースのロックの外で呼ぶ。パスを確かめ、zip と .unitypackage の中の Unity パッケージを並べる。
pub(crate) fn finish_files(pending: PendingFiles) -> Value {
    use crate::model::FilePlace;
    let roots: Vec<PathBuf> = [
        file_browser::optional_root(&pending.library),
        file_browser::optional_root(&pending.remote),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut snapshot = pending.snapshot;
    crate::library_files::fill_places(&mut snapshot, &pending.remote);
    let mut rows: Vec<FileRow> = snapshot
        .placed_downloads
        .iter()
        .map(|placed| FileRow {
            filename: placed.filename.clone(),
            variation: placed.variation.clone(),
            size: placed.size_bytes,
            found: match placed.state {
                FilePlace::Local => placed.local_path.clone().map(|path| (path, "local")),
                FilePlace::Remote => placed.remote_path.clone().map(|path| (path, "remote")),
                _ => None,
            },
        })
        .collect();
    // ダウンロード行の無い、スキャンで結び付けたファイル。
    let placed_names: HashSet<&str> = snapshot
        .placed_downloads
        .iter()
        .map(|placed| placed.filename.as_str())
        .collect();
    rows.extend(
        snapshot
            .files
            .iter()
            .filter(|file| !placed_names.contains(file.filename.as_str()))
            .map(|file| FileRow {
                filename: file.filename.clone(),
                variation: String::new(),
                size: Some(file.size),
                found: Some((file.path.clone(), "local")),
            }),
    );
    let mut listed = 0usize;
    let files: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            let inside = row
                .found
                .as_ref()
                .and_then(|(raw, place)| inside_roots(raw, &roots).map(|path| (path, *place)));
            let state = match (&inside, &row.found) {
                (Some((_, place)), _) => *place,
                (None, Some(_)) => "outsideLibrary",
                (None, None) => "notDownloaded",
            };
            let mut value = json!({
                "filename": row.filename,
                "variation": row.variation,
                "sizeBytes": row.size,
                "state": state,
            });
            if let Some((path, _)) = inside {
                value["path"] = json!(path.display().to_string());
                let package = has_extension(&path, "unitypackage") || has_extension(&path, "zip");
                if package && listed < MAX_LISTED_ARCHIVES {
                    listed += 1;
                    match unity_import::list_packages(&path) {
                        Ok(list) => {
                            value["unityPackages"] = json!(list.packages);
                            if list.truncated {
                                value["unityPackagesTruncated"] = json!(true);
                            }
                        }
                        Err(error) => value["unityPackagesError"] = json!(error),
                    }
                }
            }
            value
        })
        .collect();
    json!({ "boothId": pending.booth_id, "title": pending.title, "files": files })
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case(extension))
}

/// ライブラリかリモートのフォルダの中にあるとき、解決したパスを返す。
fn inside_roots(raw: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if roots.is_empty() {
        return None;
    }
    let requested = file_browser::requested_path(raw, "").ok()?;
    if !requested.is_absolute() {
        return None;
    }
    let resolved = file_browser::resolve(&requested).ok()?;
    roots
        .iter()
        .any(|root| resolved.starts_with(root) && resolved != *root)
        .then_some(resolved)
}

/// 検索や一覧の 1 件に添える事実。
struct Facts<'a> {
    products: HashMap<&'a str, &'a Product>,
    adult: HashSet<&'a str>,
    avatars_of: HashMap<&'a str, Vec<&'a str>>,
    has_files: HashSet<&'a str>,
}

impl<'a> Facts<'a> {
    fn new(snapshot: &'a view::Snapshot) -> Self {
        let names: HashMap<&str, &str> = snapshot
            .avatars
            .iter()
            .map(|avatar| (avatar.id.as_str(), avatar.display_name.as_str()))
            .collect();
        let mut avatars_of: HashMap<&str, Vec<&str>> = HashMap::new();
        for link in &snapshot.links {
            if let Some(name) = names.get(link.avatar_id.as_str()) {
                avatars_of
                    .entry(link.product_id.as_str())
                    .or_default()
                    .push(name);
            }
        }
        let has_files = snapshot
            .download_rows
            .iter()
            .filter(|row| {
                row.local_path
                    .as_deref()
                    .is_some_and(|path| !path.is_empty())
            })
            .map(|row| row.booth_id.as_str())
            .chain(
                snapshot
                    .files
                    .iter()
                    .filter(|file| !file.is_dir)
                    .filter_map(|file| file.linked_product_id.as_deref()),
            )
            .collect();
        Facts {
            products: snapshot
                .products
                .iter()
                .map(|product| (product.booth_id.as_str(), product))
                .collect(),
            adult: snapshot
                .product_flags
                .iter()
                .filter(|flag| flag.is_adult == Some(true))
                .map(|flag| flag.booth_id.as_str())
                .collect(),
            avatars_of,
            has_files,
        }
    }

    fn item(&self, product: &Product) -> Value {
        let adult = self.adult.contains(product.booth_id.as_str());
        let genre = product.genre.as_deref().map(genre_label);
        let mut value = json!({
            "boothId": product.booth_id,
            "title": product.title,
            "shop": product.shop,
            "category": product.category.map(Category::as_str),
            "genre": genre,
            "compatibleAvatars": self.avatars_of.get(product.booth_id.as_str()).cloned().unwrap_or_default(),
            "generic": product.is_generic,
            "hasLocalFiles": self.has_files.contains(product.booth_id.as_str()),
        });
        if adult {
            value["adult"] = json!(true);
        } else if !product.description_snippet.is_empty() {
            value["snippet"] = json!(
                product
                    .description_snippet
                    .chars()
                    .take(SNIPPET_CHARS)
                    .collect::<String>()
            );
        }
        value
    }
}

fn genre_label(key: &str) -> String {
    crate::genre::ASSET_GENRES
        .iter()
        .chain(crate::genre::WORLD_GENRES)
        .find(|(known, _)| *known == key)
        .map(|(_, label)| label.to_string())
        .unwrap_or_else(|| key.to_string())
}

fn find_avatar<'a>(snapshot: &'a view::Snapshot, raw: &str) -> Option<&'a crate::model::Avatar> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().count() > 200 {
        return None;
    }
    snapshot
        .avatars
        .iter()
        .find(|avatar| avatar.id == raw)
        .or_else(|| {
            snapshot
                .avatars
                .iter()
                .find(|avatar| avatar.display_name == raw)
        })
        .or_else(|| crate::resolve::resolve_name(raw, &snapshot.avatars))
}

fn search(connection: &Connection, body: SearchBody, adult_ok: bool) -> Result<Reply, String> {
    let snapshot = db::load_snapshot(connection)?;
    let facts = Facts::new(&snapshot);
    let query = body.query.trim();
    let avatar = match body
        .avatar
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
    {
        Some(raw) => match find_avatar(&snapshot, raw) {
            Some(avatar) => Some(avatar),
            None => {
                return Ok(Reply::error(
                    404,
                    "アバターが見つかりません。list_avatars で名前を確かめてください",
                ));
            }
        },
        None => None,
    };
    let kind = body
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    if kind.is_some_and(|kind| !matches!(kind, "asset" | "world" | "avatar")) {
        return Ok(Reply::error(
            400,
            "kind は asset・world・avatar のどれかです",
        ));
    }
    let genre = body
        .genre
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty());
    if query.is_empty() && avatar.is_none() && genre.is_none() {
        return Ok(Reply::error(
            400,
            "query・avatar・genre のどれかを指定してください",
        ));
    }
    let ids: Vec<String> = if query.is_empty() {
        snapshot
            .products
            .iter()
            .map(|product| product.booth_id.clone())
            .collect()
    } else {
        crate::search::product_ids(connection, query)?
    };
    // アバターの絞り込みは、アバター詳細と同じ並び（素体経由・汎用・対応版未設定を含む）。
    let for_avatar: Option<HashMap<String, bool>> = avatar.and_then(|avatar| {
        view::panel_avatar_sections(&snapshot, &avatar.id).map(|sections| {
            sections
                .into_iter()
                .flat_map(|section| section.items)
                .map(|item| (item.booth_id, item.via_base_body))
                .collect()
        })
    });
    let limit = body.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let mut total = 0usize;
    let mut items = Vec::new();
    for id in &ids {
        let Some(product) = facts.products.get(id.as_str()) else {
            continue;
        };
        if !adult_ok && facts.adult.contains(id.as_str()) {
            continue;
        }
        let category = product.category;
        let kind_ok = match kind {
            None => true,
            Some("avatar") => category == Some(Category::Avatar),
            Some("world") => category == Some(Category::World),
            Some(_) => !matches!(category, Some(Category::Avatar) | Some(Category::World)),
        };
        if !kind_ok {
            continue;
        }
        if let Some(genre) = genre
            && !product
                .genre
                .as_deref()
                .is_some_and(|key| key == genre || genre_label(key) == genre)
        {
            continue;
        }
        let via = match &for_avatar {
            Some(map) => match map.get(id) {
                Some(via) => Some(*via),
                None => continue,
            },
            None => None,
        };
        total += 1;
        if items.len() < limit {
            let mut value = facts.item(product);
            if let Some(via) = via {
                value["viaBaseBody"] = json!(via);
            }
            items.push(value);
        }
    }
    Ok(Reply::ok(json!({
        "total": total,
        "returned": items.len(),
        "items": items,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn bearer() -> String {
        format!("Bearer {TOKEN}")
    }

    #[test]
    fn gate_rejects_browsers_disabled_and_wrong_codes() {
        let ok = gate(
            "POST",
            "/mcp/list_avatars",
            None,
            Some(&bearer()),
            true,
            TOKEN,
            false,
        );
        assert_eq!(ok, Ok(Route::Avatars));
        let web = gate(
            "POST",
            "/mcp/list_avatars",
            Some("https://example.com"),
            Some(&bearer()),
            true,
            TOKEN,
            false,
        );
        assert_eq!(web.unwrap_err().0, 403);
        let extension = gate(
            "POST",
            "/mcp/list_avatars",
            Some("chrome-extension://ljogdbanapchdibbojckfaifjceffigj"),
            Some(&bearer()),
            true,
            TOKEN,
            false,
        );
        assert_eq!(extension.unwrap_err().0, 403);
        let off = gate(
            "POST",
            "/mcp/list_avatars",
            None,
            Some(&bearer()),
            false,
            TOKEN,
            false,
        );
        assert_eq!(off.unwrap_err().0, 403);
        let wrong = gate(
            "POST",
            "/mcp/list_avatars",
            None,
            Some("Bearer 0123456789abcdef0123456789abcdee"),
            true,
            TOKEN,
            false,
        );
        assert_eq!(wrong.unwrap_err().0, 401);
        let none = gate("POST", "/mcp/list_avatars", None, None, true, TOKEN, false);
        assert_eq!(none.unwrap_err().0, 401);
        let empty_expected = gate(
            "POST",
            "/mcp/list_avatars",
            None,
            Some("Bearer "),
            true,
            "",
            false,
        );
        assert_eq!(empty_expected.unwrap_err().0, 401);
        let locked = gate(
            "POST",
            "/mcp/list_avatars",
            None,
            Some(&bearer()),
            true,
            TOKEN,
            true,
        );
        assert_eq!(locked.unwrap_err().0, 429);
        let get = gate(
            "GET",
            "/mcp/list_avatars",
            None,
            Some(&bearer()),
            true,
            TOKEN,
            false,
        );
        assert_eq!(get.unwrap_err().0, 405);
        let unknown = gate(
            "POST",
            "/mcp/delete",
            None,
            Some(&bearer()),
            true,
            TOKEN,
            false,
        );
        assert_eq!(unknown.unwrap_err().0, 404);
    }

    fn json_of(reply: Reply) -> (u16, Value) {
        match reply {
            Reply::Json { status, body } => (status, body),
            Reply::Files(pending) => (200, finish_files(pending)),
            Reply::Download(plan) => (
                200,
                json!({ "plan": plan.filename, "variation": plan.variation }),
            ),
        }
    }

    #[test]
    fn download_needs_the_switch_and_refuses_adult_and_excluded() {
        let (_library, _outside, connection) = library();
        connection
            .execute_batch(
                "INSERT INTO product_files (booth_id, variation, filename) VALUES ('4242', 'Quest', 'missing.zip');
                 INSERT INTO product_files (booth_id, filename) VALUES ('5353', 'adult.zip');",
            )
            .unwrap();
        let ask = |body: &str| json_of(handle(&connection, Route::DownloadFile, body).unwrap());
        let body = r#"{"boothId":"4242","filename":"missing.zip","variation":""}"#;
        assert_eq!(ask(body).0, 403, "既定はオフ");
        let (_, caps) = json_of(handle(&connection, Route::Capabilities, "").unwrap());
        assert_eq!(caps["downloadFile"], false);
        db::set_setting(&connection, DOWNLOAD_KEY, "1").unwrap();
        let (_, caps) = json_of(handle(&connection, Route::Capabilities, "").unwrap());
        assert_eq!(caps["downloadFile"], true);
        let (status, plan) = ask(body);
        assert_eq!(status, 200);
        assert_eq!(plan["plan"], "missing.zip");
        assert_eq!(plan["variation"], "");
        assert_eq!(
            ask(r#"{"boothId":"4242","filename":"missing.zip"}"#).0,
            400,
            "2 つのバリエーションにある名前は指定が要る"
        );
        let (status, plan) =
            ask(r#"{"boothId":"4242","filename":"missing.zip","variation":"Quest"}"#);
        assert_eq!((status, plan["variation"].as_str()), (200, Some("Quest")));
        assert_eq!(ask(r#"{"boothId":"4242","filename":"nope.zip"}"#).0, 404);
        db::set_setting(&connection, ADULT_KEY, "1").unwrap();
        assert_eq!(
            ask(r#"{"boothId":"5353","filename":"adult.zip"}"#).0,
            403,
            "R-18 は許可しても取らない"
        );
        assert_eq!(
            ask(r#"{"boothId":"6464","filename":"x.zip"}"#).0,
            404,
            "除外した商品"
        );
        assert_eq!(
            ask(r#"{"boothId":"4242","filename":"a","files":["b"]}"#).0,
            400,
            "一括の指定は読まない"
        );
    }

    fn library() -> (tempfile::TempDir, tempfile::TempDir, Connection) {
        let library = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(library.path().join("4242")).unwrap();
        let inside = library.path().join("4242").join("pack.unitypackage");
        std::fs::write(&inside, b"x").unwrap();
        let elsewhere = outside.path().join("other.unitypackage");
        std::fs::write(&elsewhere, b"y").unwrap();
        let connection = db::open_memory().unwrap();
        connection
            .execute_batch(
                "INSERT INTO products (booth_id, title, description_snippet, imported_at, updated_at) VALUES ('4242', '架空のパーカー', '架空ちゃん対応のパーカーです', 't', 't');
                 INSERT INTO products (booth_id, title, is_adult, description_snippet, imported_at, updated_at) VALUES ('5353', '架空のパーカー 大人', 1, '本文', 't', 't');
                 INSERT INTO products (booth_id, title, imported_at, updated_at) VALUES ('6464', '架空のパーカー 除外', 't', 't');
                 INSERT INTO product_exclusions (booth_id, excluded_at) VALUES ('6464', 't');
                 INSERT INTO product_files (booth_id, filename) VALUES ('4242', 'missing.zip');",
            )
            .unwrap();
        for (name, path) in [
            ("pack.unitypackage", &inside),
            ("other.unitypackage", &elsewhere),
        ] {
            connection
                .execute(
                    "INSERT INTO product_files (booth_id, filename, downloaded, local_path) VALUES ('4242', ?1, 1, ?2)",
                    rusqlite::params![name, path.display().to_string()],
                )
                .unwrap();
        }
        // リモートにだけあるファイル。手元のパスは記録していない。
        let remote = library.path().join("remote");
        std::fs::create_dir_all(remote.join("4242")).unwrap();
        std::fs::write(remote.join("4242").join("synced.zip"), b"z").unwrap();
        connection
            .execute(
                "INSERT INTO product_files (booth_id, filename, downloaded) VALUES ('4242', 'synced.zip', 1)",
                [],
            )
            .unwrap();
        db::set_setting(
            &connection,
            "remote_library_folder",
            &remote.display().to_string(),
        )
        .unwrap();
        db::set_setting(
            &connection,
            "library_folder",
            &library.path().display().to_string(),
        )
        .unwrap();
        (library, outside, connection)
    }

    #[test]
    fn search_hides_excluded_and_adult_products_unless_allowed() {
        let (_library, _outside, connection) = library();
        let (status, body) =
            json_of(handle(&connection, Route::Search, r#"{"query":"パーカー"}"#).unwrap());
        assert_eq!(status, 200);
        let ids: Vec<&str> = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["boothId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["4242"]);
        assert_eq!(body["items"][0]["hasLocalFiles"], true);
        assert!(
            body["items"][0]["snippet"]
                .as_str()
                .unwrap()
                .contains("架空ちゃん")
        );
        assert!(body["items"][0].get("url").is_none());

        db::set_setting(&connection, ADULT_KEY, "1").unwrap();
        let (_, body) =
            json_of(handle(&connection, Route::Search, r#"{"query":"パーカー"}"#).unwrap());
        let adult = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["boothId"] == "5353")
            .expect("allowed adult item");
        assert_eq!(adult["adult"], true);
        assert!(adult.get("snippet").is_none(), "R-18 の説明は返さない");
        assert_eq!(body["total"], 2, "除外した商品は返さない");

        let (status, _) = json_of(handle(&connection, Route::Search, "{}").unwrap());
        assert_eq!(status, 400);
        let (status, _) =
            json_of(handle(&connection, Route::Search, r#"{"query":"x","rm":1}"#).unwrap());
        assert_eq!(status, 400);
    }

    #[test]
    fn product_files_returns_only_paths_inside_the_library() {
        let (_library, _outside, connection) = library();
        let (status, body) =
            json_of(handle(&connection, Route::ProductFiles, r#"{"boothId":"4242"}"#).unwrap());
        assert_eq!(status, 200);
        let files = body["files"].as_array().unwrap();
        let by_name = |name: &str| {
            files
                .iter()
                .find(|file| file["filename"] == name)
                .unwrap()
                .clone()
        };
        let inside = by_name("pack.unitypackage");
        assert_eq!(inside["state"], "local");
        assert!(
            inside["path"]
                .as_str()
                .unwrap()
                .ends_with("pack.unitypackage")
        );
        assert_eq!(inside["unityPackages"][0]["name"], "pack.unitypackage");
        let outside = by_name("other.unitypackage");
        assert_eq!(outside["state"], "outsideLibrary");
        assert!(outside.get("path").is_none());
        assert_eq!(by_name("missing.zip")["state"], "notDownloaded");
        let synced = by_name("synced.zip");
        assert_eq!(synced["state"], "remote");
        assert!(synced["path"].as_str().unwrap().ends_with("synced.zip"));

        let (status, _) =
            json_of(handle(&connection, Route::ProductFiles, r#"{"boothId":"5353"}"#).unwrap());
        assert_eq!(status, 404, "R-18 は許すまで見せない");
        let (status, _) =
            json_of(handle(&connection, Route::ProductFiles, r#"{"boothId":"6464"}"#).unwrap());
        assert_eq!(status, 404, "除外した商品");
        let (status, _) =
            json_of(handle(&connection, Route::ProductFiles, r#"{"boothId":"../1"}"#).unwrap());
        assert_eq!(status, 400);
    }

    #[test]
    fn token_file_follows_the_switch() {
        let dir = tempfile::tempdir().unwrap();
        let mut connection = db::open_memory().unwrap();
        sync_token_file(&connection, dir.path()).unwrap();
        assert!(!dir.path().join(TOKEN_FILE).exists());
        save(&mut connection, dir.path(), true, false, false).unwrap();
        let written = std::fs::read_to_string(dir.path().join(TOKEN_FILE)).unwrap();
        assert!(token_ok(&written));
        assert_eq!(written, token(&connection).unwrap());
        regenerate(&connection, dir.path()).unwrap();
        let renewed = std::fs::read_to_string(dir.path().join(TOKEN_FILE)).unwrap();
        assert_ne!(renewed, written);
        save(&mut connection, dir.path(), false, false, false).unwrap();
        assert!(!dir.path().join(TOKEN_FILE).exists());
    }
}
