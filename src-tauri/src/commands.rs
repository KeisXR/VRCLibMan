use crate::AppState;
use crate::db::{self, ExtractJob};
use crate::model::{
    AvatarInput, BaseBodyInput, CatalogInfo, CatalogRefresh, Category, ClassificationSnapshot,
    ExtractReport, ImportReport, Knowledge, LibraryStats, ProductUpdate, RegisterOwnedBodyResult,
    ScanReport, Settings, SettingsInput, SetupStatus, StagedLeftover,
};
use crate::scan;
use crate::view::{
    self, AvatarCard, AvatarDetail, GenreProducts, ItemView, ProductEditView, ProductOption,
    UnlinkedFilesPage,
};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard};
use tauri::{AppHandle, Emitter, Manager, State};

struct ClearRunning(Arc<std::sync::atomic::AtomicBool>);

fn trusted_updater(view: &tauri::Webview) -> Result<(), String> {
    if view.label() == "main" {
        Ok(())
    } else {
        Err("この画面からアプリを更新する権限がありません".into())
    }
}

#[tauri::command]
pub fn updater_status(
    view: tauri::Webview,
    state: State<'_, crate::updater::UpdateState>,
) -> Result<crate::updater::Status, String> {
    trusted_updater(&view)?;
    state.status()
}

#[tauri::command]
pub async fn check_app_update(
    view: tauri::Webview,
    app: AppHandle,
    state: State<'_, crate::updater::UpdateState>,
    manual: Option<bool>,
) -> Result<crate::updater::Status, String> {
    trusted_updater(&view)?;
    crate::updater::check(&app, &state, manual.unwrap_or(true)).await
}

#[tauri::command]
pub async fn download_app_update(
    view: tauri::Webview,
    app: AppHandle,
    state: State<'_, crate::updater::UpdateState>,
) -> Result<crate::updater::Status, String> {
    trusted_updater(&view)?;
    crate::updater::download(&app, &state).await
}

#[tauri::command]
pub async fn install_app_update(
    view: tauri::Webview,
    app: AppHandle,
    state: State<'_, crate::updater::UpdateState>,
) -> Result<crate::updater::Status, String> {
    trusted_updater(&view)?;
    crate::updater::install(&app, &state).await
}

impl Drop for ClearRunning {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Commands that only read the database for a screen run off the UI thread
/// (`#[tauri::command(async)]`). A sync command runs on the UI thread and waits there for this
/// lock while another command, an extraction or the sync worker holds it, and the whole window
/// stops responding meanwhile. `read_commands_run_off_the_ui_thread` lists them.
fn db(state: &AppState) -> Result<MutexGuard<'_, rusqlite::Connection>, String> {
    state
        .db
        .lock()
        .map_err(|_| "データベースをロックできません".to_string())
}

use crate::progress::ProgressGuard;

#[tauri::command]
pub fn import_export(state: State<'_, AppState>, json: String) -> Result<ImportReport, String> {
    let mut connection = db(&state)?;
    db::import_export_json(&mut connection, &json)
}

#[tauri::command(async)]
pub fn extension_link(state: State<'_, AppState>) -> Result<crate::model::ExtensionLink, String> {
    let connection = db(&state)?;
    let token = db::extension_token(&connection)?;
    drop(connection);
    Ok(extension_link_view(&state, &token))
}

/// 拡張の「アプリで開く」で頼まれた行き先を 1 回だけ返す。メイン画面からだけ受け付ける。
/// データベースは読まないので、UI スレッドで待つものが無い。
#[tauri::command]
pub fn take_extension_route(view: tauri::Webview, state: State<'_, AppState>) -> Option<String> {
    if view.label() != "main" {
        return None;
    }
    state
        .extension_route
        .lock()
        .ok()
        .and_then(|mut pending| pending.take())
}

#[tauri::command]
pub fn regenerate_extension_token(
    state: State<'_, AppState>,
) -> Result<crate::model::ExtensionLink, String> {
    let connection = db(&state)?;
    let token = db::regenerate_extension_token(&connection)?;
    drop(connection);
    if let Ok(mut guard) = state.import_guard.lock() {
        guard.clear();
    }
    Ok(extension_link_view(&state, &token))
}

/// AI エージェント連携の設定と、エージェントに書かせる実行ファイルのパス。
#[tauri::command(async)]
pub fn mcp_status(state: State<'_, AppState>) -> Result<crate::mcp_api::McpStatus, String> {
    let connection = db(&state)?;
    crate::mcp_api::status(&connection)
}

/// AI エージェント連携のオン・オフと、R-18 の商品を含めるか。メイン画面からだけ受け付ける。
#[tauri::command(async)]
pub fn set_mcp_settings(
    view: tauri::Webview,
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
    include_adult: bool,
    allow_download: bool,
) -> Result<crate::mcp_api::McpStatus, String> {
    if view.label() != "main" {
        return Err("この画面からは変更できません".into());
    }
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| "アプリのデータフォルダが見つかりません".to_string())?;
    let mut connection = db(&state)?;
    crate::mcp_api::save(
        &mut connection,
        &directory,
        enabled,
        include_adult,
        allow_download,
    )?;
    crate::mcp_api::status(&connection)
}

/// AI エージェント連携の認証コードを作り直す。メイン画面からだけ受け付ける。
#[tauri::command(async)]
pub fn regenerate_mcp_token(
    view: tauri::Webview,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if view.label() != "main" {
        return Err("この画面からは変更できません".into());
    }
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|_| "アプリのデータフォルダが見つかりません".to_string())?;
    let connection = db(&state)?;
    crate::mcp_api::regenerate(&connection, &directory)
}

/// 同梱の SKILL のフォルダ（`agent-skill/vrclibman`）。
fn bundled_skill(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("agent-skill").join("vrclibman"));
    if let Some(dir) = bundled.filter(|dir| dir.join("SKILL.md").is_file()) {
        return Ok(dir);
    }
    // tauri dev ではリソースを写さないことがあるので、ソースのフォルダを使う。
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("agent-skill")
        .join("vrclibman");
    if cfg!(debug_assertions) && source.join("SKILL.md").is_file() {
        return Ok(source);
    }
    Err("同梱の SKILL が見つかりません。アプリを入れ直してください".into())
}

fn agent_env() -> Result<crate::agent_setup::Env, String> {
    crate::agent_setup::Env::current().ok_or_else(|| "ホームフォルダが見つかりません".to_string())
}

/// エージェントの設定は、AI エージェント連携がオンのときだけ書く。
fn require_mcp_enabled(view: &tauri::Webview, state: &AppState) -> Result<(), String> {
    if view.label() != "main" {
        return Err("この画面からは変更できません".into());
    }
    let connection = db(state)?;
    if crate::mcp_api::enabled(&connection)? {
        Ok(())
    } else {
        Err("先に「AI エージェント連携」をオンにしてください".into())
    }
}

/// 各 AI エージェントの設定と SKILL の状態。ファイルを読むだけ。
#[tauri::command]
pub async fn agent_clients() -> Result<Vec<crate::agent_setup::ClientStatus>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let env = agent_env()?;
        Ok(crate::agent_setup::statuses(
            &env,
            &crate::mcp_api::command_path(),
            env!("CARGO_PKG_VERSION"),
        ))
    })
    .await
    .map_err(|err| format!("エージェントの状態を読めません: {err}"))?
}

/// AI エージェントの設定ファイルに vrclibman を書く（`add`）か外す。メイン画面からだけ受け付ける。
#[tauri::command]
pub async fn change_agent_config(
    view: tauri::Webview,
    state: State<'_, AppState>,
    id: String,
    add: bool,
) -> Result<crate::agent_setup::ChangeReport, String> {
    require_mcp_enabled(&view, &state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let report = crate::agent_setup::change_config(
            &agent_env()?,
            &id,
            &crate::mcp_api::command_path(),
            add,
        )?;
        eprintln!(
            "AI エージェントの設定: {} {}",
            report.label,
            if add { "追加" } else { "削除" }
        );
        Ok(report)
    })
    .await
    .map_err(|err| format!("エージェントを設定できません: {err}"))?
}

/// 同梱の SKILL を AI エージェントのフォルダに入れる（`add`）か外す。メイン画面からだけ受け付ける。
#[tauri::command]
pub async fn change_agent_skill(
    view: tauri::Webview,
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    add: bool,
) -> Result<crate::agent_setup::ChangeReport, String> {
    require_mcp_enabled(&view, &state)?;
    let source = bundled_skill(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::agent_setup::change_skill(
            &agent_env()?,
            &id,
            &source,
            env!("CARGO_PKG_VERSION"),
            add,
        )
    })
    .await
    .map_err(|err| format!("SKILL を入れられません: {err}"))?
}

fn extension_link_view(state: &AppState, token: &str) -> crate::model::ExtensionLink {
    let error = state
        .receiver_error
        .lock()
        .ok()
        .and_then(|slot| slot.clone());
    crate::model::ExtensionLink {
        token: crate::local_import::display_token(token),
        port: crate::local_import::LOCAL_IMPORT_PORT,
        listening: error.is_none(),
        error,
    }
}

#[tauri::command(async)]
pub fn integration_status(
    state: State<'_, AppState>,
) -> Result<crate::integration::IntegrationStatus, String> {
    let connection = db(&state)?;
    crate::integration::status(&connection)
}

#[tauri::command]
pub fn extension_package_status(
    app: AppHandle,
) -> Result<crate::extension_package::PackageStatus, String> {
    crate::extension_package::status(&app)
}

#[tauri::command]
pub fn install_extension_package(
    app: AppHandle,
) -> Result<crate::extension_package::PackageStatus, String> {
    crate::extension_package::install(&app)
}

#[tauri::command]
pub fn set_booth_import_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    crate::integration::set_mode(&mut connection, &mode)?;
    drop(connection);
    if mode != "browser" {
        // 拡張を使い始めるときは、読み込ませるフォルダを先に用意しておく。失敗しても設定は保存済み。
        let _ = crate::extension_package::refresh(&app, true);
    }
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command(async)]
pub fn list_managed_products(
    state: State<'_, AppState>,
) -> Result<Vec<crate::model::ManagedProduct>, String> {
    let connection = db(&state)?;
    db::managed_products(&connection)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn set_product_excluded(
    app: AppHandle,
    state: State<'_, AppState>,
    boothId: String,
    excluded: bool,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::set_product_excluded(&mut connection, &boothId, excluded)?;
    drop(connection);
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command(async)]
pub fn library_stats(state: State<'_, AppState>) -> Result<LibraryStats, String> {
    let connection = db(&state)?;
    db::library_stats(&connection)
}

#[tauri::command(async)]
pub fn list_avatars(state: State<'_, AppState>) -> Result<Vec<AvatarCard>, String> {
    let connection = db(&state)?;
    let snapshot = db::load_snapshot(&connection)?;
    Ok(view::avatar_cards(&snapshot))
}

#[tauri::command(async)]
pub fn list_products_by_category(
    state: State<'_, AppState>,
    category: String,
) -> Result<Vec<ItemView>, String> {
    let category = Category::parse(&category).ok_or_else(|| "商品の種類が不正です".to_string())?;
    let connection = db(&state)?;
    let ids = db::product_ids_by_category(&connection, category)?;
    let mut snapshot = db::load_snapshot(&connection)?;
    let remote = db::settings(&connection)?.remote_library_folder;
    crate::library_files::fill_local_places(&mut snapshot, &remote);
    Ok(view::products_by_ids(&snapshot, &ids))
}

#[tauri::command(async)]
pub fn list_products_by_genre(
    state: State<'_, AppState>,
    kind: String,
    genre: Option<String>,
) -> Result<GenreProducts, String> {
    let connection = db(&state)?;
    let genres =
        crate::genre::kind_genres(&kind).ok_or_else(|| "ジャンルの種類が不正です".to_string())?;
    let all_ids = db::product_ids_by_genre(&connection, &kind, None)?;
    let mut snapshot = db::load_snapshot(&connection)?;
    let remote = db::settings(&connection)?.remote_library_folder;
    crate::library_files::fill_local_places(&mut snapshot, &remote);
    Ok(view::genre_products(
        &snapshot,
        genres,
        &all_ids,
        genre.as_deref(),
    ))
}

fn load_avatar(app: &AppHandle, id: &str, probe_remote: bool) -> Result<AvatarDetail, String> {
    let state = app.state::<AppState>();
    let (mut snapshot, remote) = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        let snapshot = db::load_snapshot(&connection)?;
        let remote = db::settings(&connection)?.remote_library_folder;
        (snapshot, remote)
    };
    if probe_remote {
        crate::library_files::fill_places(&mut snapshot, &remote);
    } else {
        crate::library_files::fill_local_places(&mut snapshot, &remote);
    }
    let mut detail = view::avatar_detail(&snapshot, id).ok_or("アバターが見つかりません")?;
    crate::hero_image::fill_display(&mut detail);
    Ok(detail)
}

#[tauri::command]
pub async fn get_avatar(app: AppHandle, id: String) -> Result<AvatarDetail, String> {
    tauri::async_runtime::spawn_blocking(move || load_avatar(&app, &id, false))
        .await
        .map_err(|err| format!("アバターを表示できません: {err}"))?
}

#[tauri::command]
pub async fn probe_remote_files(app: AppHandle, id: String) -> Result<AvatarDetail, String> {
    tauri::async_runtime::spawn_blocking(move || load_avatar(&app, &id, true))
        .await
        .map_err(|err| format!("リモートを確認できません: {err}"))?
}

#[tauri::command]
pub async fn probe_remote_products(
    app: AppHandle,
    ids: Vec<String>,
) -> Result<Vec<crate::view::RemoteProductProbe>, String> {
    tauri::async_runtime::spawn_blocking(move || probe_remote_products_blocking(&app, ids))
        .await
        .map_err(|err| format!("リモートを確認できません: {err}"))?
}

fn probe_remote_products_blocking(
    app: &AppHandle,
    ids: Vec<String>,
) -> Result<Vec<crate::view::RemoteProductProbe>, String> {
    let mut ids: Vec<String> = ids
        .into_iter()
        .filter(|id| crate::library_files::valid_booth_id(id))
        .collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let state = app.state::<AppState>();
    let (mut snapshot, remote) = {
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        let snapshot = db::load_snapshot(&connection)?;
        let remote = db::settings(&connection)?.remote_library_folder;
        (snapshot, remote)
    };
    let probes = crate::library_files::probe_remote_for_products(&mut snapshot, &remote, &ids);
    Ok(crate::view::remote_product_probes(&snapshot, &probes))
}

#[tauri::command(async)]
pub fn get_booth_page_interval(state: State<'_, AppState>) -> Result<u64, String> {
    let connection = db(&state)?;
    db::booth_page_interval_ms(&connection)
}

#[tauri::command]
pub fn set_booth_page_interval(state: State<'_, AppState>, interval_ms: u64) -> Result<(), String> {
    if crate::browser::library_walk_running() {
        return Err("インポート中はアクセス間隔を変更できません".into());
    }
    let connection = db(&state)?;
    db::set_booth_page_interval_ms(&connection, interval_ms)
}

fn unity_status_blocking(app: &AppHandle) -> Result<crate::unity::UnityStatus, String> {
    let selected = {
        let state = app.state::<AppState>();
        let connection = db(&state)?;
        db::unity_editor_path(&connection)?
    };
    let os = crate::unity::Os::current();
    Ok(crate::unity::status(
        &selected,
        &crate::unity::Sources::for_current_os(),
        os,
    ))
}

/// インストール済みの Unity を検出する。Unity Hub の記録ファイルと既定のフォルダを読むだけ。
#[tauri::command]
pub async fn unity_editors(app: AppHandle) -> Result<crate::unity::UnityStatus, String> {
    tauri::async_runtime::spawn_blocking(move || unity_status_blocking(&app))
        .await
        .map_err(|err| format!("Unity を検出できません: {err}"))?
}

/// 既定の Unity を保存する。空文字で未設定に戻す。メイン画面からだけ受け付ける。
#[tauri::command]
pub async fn set_unity_editor(
    view: tauri::Webview,
    app: AppHandle,
    path: String,
) -> Result<crate::unity::UnityStatus, String> {
    if view.label() != "main" {
        return Err("この画面から Unity の設定は変更できません".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let os = crate::unity::Os::current();
        let value = if path.trim().is_empty() {
            String::new()
        } else {
            crate::unity::validate_manual(&path, os)?
                .display()
                .to_string()
        };
        {
            let state = app.state::<AppState>();
            let connection = db(&state)?;
            db::set_unity_editor_path(&connection, &value)?;
        }
        unity_status_blocking(&app)
    })
    .await
    .map_err(|err| format!("Unity の設定を保存できません: {err}"))?
}

#[tauri::command(async)]
#[allow(non_snake_case)]
pub fn product_for_edit(
    state: State<'_, AppState>,
    boothId: String,
) -> Result<ProductEditView, String> {
    let connection = db(&state)?;
    let snapshot = db::load_snapshot(&connection)?;
    view::product_edit_view(&snapshot, &boothId).ok_or_else(|| "商品が見つかりません".to_string())
}

#[tauri::command(async)]
pub fn list_unlinked_files(
    state: State<'_, AppState>,
    offset: u32,
    limit: u32,
) -> Result<UnlinkedFilesPage, String> {
    let connection = db(&state)?;
    let snapshot = db::load_snapshot(&connection)?;
    Ok(view::unlinked_files_page(
        &snapshot,
        offset,
        limit.clamp(1, 200),
    ))
}

#[tauri::command(async)]
pub fn classification_log_runs(
    state: State<'_, AppState>,
    offset: u32,
    limit: u32,
) -> Result<db::ClassificationRunPage, String> {
    let connection = db(&state)?;
    db::run_page(&connection, offset, limit.clamp(1, 200))
}

#[tauri::command(async)]
#[allow(non_snake_case)]
pub fn classification_log_entries(
    state: State<'_, AppState>,
    runId: i64,
    offset: u32,
    limit: u32,
) -> Result<Vec<db::ClassificationEntryView>, String> {
    let connection = db(&state)?;
    db::run_entries(&connection, runId, offset, limit.clamp(1, 200))
}

#[tauri::command(async)]
pub fn classification_log_search(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> Result<Vec<db::ClassificationEntryView>, String> {
    let connection = db(&state)?;
    db::search_entries(&connection, &query, limit.clamp(1, 200))
}

#[tauri::command(async)]
pub fn classification_unresolved_names(
    state: State<'_, AppState>,
) -> Result<Vec<db::UnresolvedNameCount>, String> {
    let connection = db(&state)?;
    db::unresolved_name_counts(&connection)
}

/// Main window only. Not during a classification run, whose entries are still being written.
#[tauri::command]
#[allow(non_snake_case)]
pub fn undo_classification_entry(
    view: tauri::Webview,
    state: State<'_, AppState>,
    entryId: i64,
) -> Result<(), String> {
    if view.label() != "main" {
        return Err("この画面から分類は戻せません".into());
    }
    if state.extract_running.load(Ordering::SeqCst) {
        return Err("分類の実行中は戻せません".into());
    }
    let mut connection = db(&state)?;
    db::undo_entry(&mut connection, entryId)?;
    // Undoing a registered body can remove its placeholder avatar and that avatar's image.
    let _ = crate::hero_image::prune(&connection, &state.hero_images_dir);
    Ok(())
}

#[tauri::command(async)]
pub fn list_product_options(state: State<'_, AppState>) -> Result<Vec<ProductOption>, String> {
    let connection = db(&state)?;
    db::product_options(&connection)
}

#[tauri::command(async)]
pub fn list_knowledge(state: State<'_, AppState>) -> Result<Knowledge, String> {
    let connection = db(&state)?;
    db::knowledge(&connection)
}

#[tauri::command]
pub fn remote_sync_status(app: AppHandle) -> Result<crate::remote_sync::Status, String> {
    crate::remote_sync::status(&app)
}

/// 「今すぐ再試行」. Wakes the one sync worker; it never starts a second one.
#[tauri::command]
pub fn retry_remote_sync(app: AppHandle) -> Result<(), String> {
    crate::remote_sync::wake(&app);
    Ok(())
}

/// Library search over each product's stored text, including the whole description.
#[tauri::command(async)]
pub fn search_product_ids(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<String>, String> {
    let connection = db(&state)?;
    crate::search::product_ids(&connection, &query)
}

#[tauri::command(async)]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let connection = db(&state)?;
    db::settings(&connection)
}

/// Off the UI thread: a save waits for the database and the disk, and the native pages in the
/// right panel are placed on the UI thread. The frontend keeps settings calls in order (`inSettingsOrder`).
#[tauri::command(async)]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: SettingsInput,
) -> Result<(), String> {
    let saved = {
        let connection = db(&state)?;
        db::save_settings(&connection, &settings)?;
        db::settings(&connection)?
    };
    crate::browser_surface::sync_force_dark(&app, saved.browser_force_dark);
    crate::remote_sync::wake(&app);
    crate::presence::remember_background(&app, saved.background_mode);
    apply_presence_on_ui(&app, saved.background_mode, saved.launch_at_login)?;
    Ok(())
}

/// The tray icon and the main window belong to the UI thread (on Windows a tray icon can only be
/// removed by the thread that made it), so the settings commands, which run off it, hand this
/// step back and wait. Never call this while holding the database lock: a command running on the
/// UI thread may be waiting for that lock.
fn apply_presence_on_ui(
    app: &AppHandle,
    background_mode: bool,
    launch_at_login: bool,
) -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = sender.send(crate::presence::apply_presence(
            &handle,
            background_mode,
            launch_at_login,
        ));
    })
    .map_err(|err| format!("画面の処理を予約できません: {err}"))?;
    receiver
        .recv()
        .map_err(|_| "画面の処理を完了できませんでした。もう一度お試しください".to_string())?
}

/// Off the UI thread like `save_settings`; the right panel saves its tabs and width here.
#[tauri::command(async)]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: crate::model::SettingsPatch,
) -> Result<Settings, String> {
    let presence_changed = patch.background_mode.is_some() || patch.launch_at_login.is_some();
    let (before, saved) = {
        let connection = db(&state)?;
        (
            db::settings(&connection)?,
            db::update_settings(&connection, &patch)?,
        )
    };
    if before.browser_force_dark != saved.browser_force_dark {
        crate::browser_surface::sync_force_dark(&app, saved.browser_force_dark);
    }
    if patch.remote_auto_sync.is_some() || patch.remote_library_folder.is_some() {
        crate::remote_sync::wake(&app);
    }
    if presence_changed {
        if let Err(error) = apply_presence_on_ui(&app, saved.background_mode, saved.launch_at_login)
        {
            {
                let connection = db(&state)?;
                db::update_settings(
                    &connection,
                    &crate::model::SettingsPatch {
                        background_mode: Some(before.background_mode),
                        launch_at_login: Some(before.launch_at_login),
                        ..Default::default()
                    },
                )?;
            }
            crate::presence::remember_background(&app, before.background_mode);
            let _ = apply_presence_on_ui(&app, before.background_mode, before.launch_at_login);
            return Err(error);
        }
        crate::presence::remember_background(&app, saved.background_mode);
    }
    Ok(saved)
}

/// File tab: one folder, read only. An empty path opens the library folder.
#[tauri::command]
pub async fn list_directory(
    app: AppHandle,
    path: String,
) -> Result<crate::file_browser::DirListing, String> {
    let (library, remote) = library_roots(&app)?;
    let mut listing = tauri::async_runtime::spawn_blocking(move || {
        crate::file_browser::list_directory(&path, &library, &remote)
    })
    .await
    .map_err(|err| format!("フォルダを読めません: {err}"))??;
    // Labels are a nicety. A DB problem leaves the plain names instead of failing the listing.
    let _ = add_folder_labels(&app, &mut listing);
    Ok(listing)
}

/// `name（title）` for product folders and registered files. Titles come from the local DB only.
fn add_folder_labels(
    app: &AppHandle,
    listing: &mut crate::file_browser::DirListing,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let connection = db(&state)?;
    let has_files = listing
        .entries
        .iter()
        .any(|entry| entry.kind == crate::file_browser::EntryKind::File);
    let linked = if has_files {
        db::linked_file_products(&connection)?
    } else {
        Vec::new()
    };
    let ids = crate::file_browser::label_ids(listing, &linked);
    if ids.is_empty() {
        return Ok(());
    }
    let titles = db::product_labels(&connection, &ids)?;
    crate::file_browser::apply_labels(listing, &linked, &titles);
    Ok(())
}

/// File tab: a zip file's entries from its central directory. Nothing is extracted.
#[tauri::command]
pub async fn list_zip_entries(
    app: AppHandle,
    path: String,
) -> Result<crate::file_browser::ZipListing, String> {
    let (library, remote) = library_roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::file_browser::list_zip(&path, &library, &remote)
    })
    .await
    .map_err(|err| format!("zip を読めません: {err}"))?
}

fn library_roots(app: &AppHandle) -> Result<(String, String), String> {
    let state = app.state::<AppState>();
    let connection = db(&state)?;
    let settings = db::settings(&connection)?;
    Ok((settings.library_folder, settings.remote_library_folder))
}

fn unity_cache_root(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map_err(|_| "アプリのキャッシュフォルダが見つかりません".to_string())
}

/// 単体のファイルを Unity パッケージに包むときの、`Assets/VRCLibMan/` の下のフォルダ名。
/// 商品名はローカルの DB からだけ読む。読めなければ商品 ID。
fn unity_loose_folder(app: &AppHandle, booth_id: Option<&str>) -> String {
    let booth_id = booth_id
        .map(str::trim)
        .filter(|id| crate::library_files::valid_booth_id(id));
    let title = booth_id.and_then(|id| {
        let state = app.state::<AppState>();
        let connection = db(&state).ok()?;
        db::product_labels(&connection, &[id.to_string()])
            .ok()?
            .remove(id)
            .map(|label| label.title)
    });
    crate::unity_wrap::folder_name(title.as_deref(), booth_id)
}

fn unity_loose_destination(
    app: &AppHandle,
    booth_id: Option<&str>,
    list: &crate::unity_import::UnityPackageList,
) -> Result<String, String> {
    let name = list
        .packages
        .first()
        .map(|item| item.name.as_str())
        .unwrap_or("");
    crate::unity_wrap::destination(&unity_loose_folder(app, booth_id), name)
        .ok_or_else(|| "Unity に入れられないファイルです".to_string())
}

/// 「Unityで開く」: zip か .unitypackage の中の Unity パッケージを一覧する。展開しない。
/// mp3・png などの単体のファイルは "loose" として、Unity の中で入る場所を添えて返す。
#[tauri::command]
pub async fn unity_packages(
    app: AppHandle,
    path: String,
    booth_id: Option<String>,
) -> Result<crate::unity_import::UnityPackageList, String> {
    let (library, remote) = library_roots(&app)?;
    let mut list = tauri::async_runtime::spawn_blocking(move || {
        let source = crate::unity_import::allowed_source(&path, &library, &remote)?;
        crate::unity_import::list_packages(&source)
    })
    .await
    .map_err(|err| format!("Unity パッケージを読めません: {err}"))??;
    if list.kind == "loose" {
        list.destination = Some(unity_loose_destination(&app, booth_id.as_deref(), &list)?);
    }
    Ok(list)
}

/// 選んだ Unity パッケージを、アプリのキャッシュ `unity-import/<商品ID>/` に取り出す。
/// 単体の .unitypackage は取り出さず、そのまま返す。mp3・png などは同じ場所で Unity パッケージに包む。
/// メイン画面からだけ受け付ける。
#[tauri::command]
pub async fn prepare_unity_packages(
    view: tauri::Webview,
    app: AppHandle,
    path: String,
    entries: Vec<String>,
    booth_id: Option<String>,
) -> Result<Vec<crate::unity_import::PreparedPackage>, String> {
    if view.label() != "main" {
        return Err("この画面から Unity パッケージは開けません".into());
    }
    let (library, remote) = library_roots(&app)?;
    let cache_root = unity_cache_root(&app)?;
    let folder = unity_loose_folder(&app, booth_id.as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let source = crate::unity_import::allowed_source(&path, &library, &remote)?;
        let list = crate::unity_import::list_packages(&source)?;
        if list.kind == "loose" {
            // 一覧で見せた場所と同じ場所に入るよう、同じ規則で決める。
            let name = list
                .packages
                .first()
                .map(|item| item.name.as_str())
                .unwrap_or("");
            let place = crate::unity_wrap::destination(&folder, name)
                .ok_or_else(|| "Unity に入れられないファイルです".to_string())?;
            let dest = crate::unity_import::cache_folder(&cache_root, booth_id.as_deref());
            return crate::unity_wrap::wrap_file(&source, &place, &dest)
                .map(|package| vec![package]);
        }
        if list.kind == "package" {
            return Ok(list
                .packages
                .into_iter()
                .map(|package| crate::unity_import::PreparedPackage {
                    name: package.name,
                    path: list.path.clone(),
                    size: package.size,
                })
                .collect());
        }
        let dest = crate::unity_import::cache_folder(&cache_root, booth_id.as_deref());
        crate::unity_import::extract_packages(
            &source,
            &entries,
            &dest,
            crate::unity_import::MAX_PACKAGE_BYTES,
        )
    })
    .await
    .map_err(|err| format!("Unity パッケージを取り出せません: {err}"))?
}

/// 起動中の Unity エディタの窓（プロジェクトごと）。2 つ以上あれば、画面が渡す先を選ばせる。
/// Windows 以外では空。
#[tauri::command(async)]
pub fn unity_instances(
    view: tauri::Webview,
) -> Result<Vec<crate::unity_import::UnityInstance>, String> {
    if view.label() != "main" {
        return Err("この画面から Unity は選べません".into());
    }
    Ok(crate::unity_import::running_instances())
}

/// Unity プロジェクトの一覧。ALCOM / VCC の一覧と、手で足した・結び付けたものを毎回読み直す。
/// プロジェクトのフォルダは読むだけ。
fn unity_project_list_blocking(
    app: &AppHandle,
) -> Result<crate::unity_projects::UnityProjectList, String> {
    use crate::unity_projects as projects;
    let (stored, links, selected) = {
        let state = app.state::<AppState>();
        let connection = db(&state)?;
        (
            db::stored_unity_projects(&connection)?,
            db::unity_project_links(&connection)?,
            db::unity_editor_path(&connection)?,
        )
    };
    let os = crate::unity::Os::current();
    let sources = projects::Sources::for_current_os();
    let alcom_json = sources.alcom_settings.clone().and_then(|path| {
        crate::library_files::wait_for(projects::READ_TIMEOUT, move || {
            std::fs::read_to_string(path).ok()
        })
        .flatten()
    });
    let alcom = alcom_json.as_deref().map(projects::parse_user_projects);
    let editors =
        crate::unity::status(&selected, &crate::unity::Sources::for_current_os(), os).editors;
    let running = projects::running_projects();
    let input = projects::ListInput {
        alcom: alcom.as_deref(),
        stored: &stored,
        links: &links,
        editors: &editors,
        running: running.as_deref(),
        os,
    };
    let found = projects::candidates(&input);
    let paths: Vec<String> = found.iter().map(|(_, path, _)| path.clone()).collect();
    let reads = projects::read_projects(&paths, projects::READ_TIMEOUT, projects::LIST_TIMEOUT);
    Ok(projects::UnityProjectList {
        projects: projects::build_list(&input, &found, &reads),
        alcom_found: alcom.is_some(),
        alcom_path: sources
            .alcom_settings
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    })
}

fn find_unity_project(
    app: &AppHandle,
    project_id: &str,
) -> Result<crate::unity_projects::UnityProject, String> {
    if !crate::unity_projects::valid_project_id(project_id) {
        return Err("プロジェクトが見つかりません".into());
    }
    unity_project_list_blocking(app)?
        .projects
        .into_iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| "プロジェクトが見つかりません。一覧を読み直してください".into())
}

fn require_main(view: &tauri::Webview) -> Result<(), String> {
    if view.label() != "main" {
        return Err("この画面から Unity のプロジェクトは変更できません".into());
    }
    Ok(())
}

async fn unity_project_task<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || work(&app))
        .await
        .map_err(|err| format!("Unity のプロジェクトを読めません: {err}"))?
}

#[tauri::command]
pub async fn unity_projects(
    app: AppHandle,
) -> Result<crate::unity_projects::UnityProjectList, String> {
    unity_project_task(app, unity_project_list_blocking).await
}

/// 商品を渡せそうなプロジェクト。対応アバターに結び付けたもの、ワールドの商品ならワールドのプロジェクト。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn product_unity_projects(
    app: AppHandle,
    boothId: String,
) -> Result<Vec<crate::unity_projects::UnityProject>, String> {
    unity_project_task(app, move |app| {
        let (avatars, world): (std::collections::HashSet<String>, bool) = {
            let state = app.state::<AppState>();
            let connection = db(&state)?;
            (
                db::product_avatar_ids(&connection, &boothId)?
                    .into_iter()
                    .collect(),
                db::product_is_world(&connection, &boothId)?,
            )
        };
        if avatars.is_empty() && !world {
            return Ok(Vec::new());
        }
        Ok(crate::unity_projects::projects_for_product(
            unity_project_list_blocking(app)?.projects,
            &avatars,
            world,
        ))
    })
    .await
}

#[tauri::command]
pub async fn add_unity_project(
    view: tauri::Webview,
    app: AppHandle,
    path: String,
) -> Result<crate::unity_projects::UnityProjectList, String> {
    require_main(&view)?;
    unity_project_task(app, move |app| {
        let os = crate::unity::Os::current();
        let path = crate::unity_projects::validate_new_project(&path, os)?;
        let id = crate::unity_projects::project_id(&path, os);
        {
            let state = app.state::<AppState>();
            let connection = db(&state)?;
            db::add_manual_unity_project(&connection, &id, &path)?;
        }
        unity_project_list_blocking(app)
    })
    .await
}

/// 手で足したもの・覚えているものを一覧から消す。ALCOM の一覧にあるものは隠すだけ。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn remove_unity_project(
    view: tauri::Webview,
    app: AppHandle,
    projectId: String,
) -> Result<crate::unity_projects::UnityProjectList, String> {
    require_main(&view)?;
    unity_project_task(app, move |app| {
        let project = find_unity_project(app, &projectId)?;
        if project.source == "alcom" {
            return Err("ALCOM の一覧にあるプロジェクトは、一覧から隠してください".into());
        }
        {
            let state = app.state::<AppState>();
            let connection = db(&state)?;
            db::forget_unity_project(&connection, &project.id)?;
        }
        unity_project_list_blocking(app)
    })
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_unity_project_hidden(
    view: tauri::Webview,
    app: AppHandle,
    projectId: String,
    hidden: bool,
) -> Result<crate::unity_projects::UnityProjectList, String> {
    require_main(&view)?;
    unity_project_task(app, move |app| {
        let project = find_unity_project(app, &projectId)?;
        {
            let state = app.state::<AppState>();
            let mut connection = db(&state)?;
            db::set_unity_project_hidden(&mut connection, &project.id, &project.path, hidden)?;
        }
        unity_project_list_blocking(app)
    })
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn link_unity_project_avatar(
    view: tauri::Webview,
    app: AppHandle,
    projectId: String,
    avatarId: String,
    linked: bool,
) -> Result<crate::unity_projects::UnityProjectList, String> {
    require_main(&view)?;
    unity_project_task(app, move |app| {
        let project = find_unity_project(app, &projectId)?;
        {
            let state = app.state::<AppState>();
            let mut connection = db(&state)?;
            db::link_unity_project_avatar(
                &mut connection,
                &project.id,
                &project.path,
                &avatarId,
                linked,
            )?;
        }
        unity_project_list_blocking(app)
    })
    .await
}

/// プロジェクトの版とちょうど同じ版の Unity で開く。開いていれば起動しない。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn open_unity_project(
    view: tauri::Webview,
    app: AppHandle,
    projectId: String,
) -> Result<crate::unity_projects::OpenResult, String> {
    require_main(&view)?;
    unity_project_task(app, move |app| {
        let project = find_unity_project(app, &projectId)?;
        let selected = {
            let state = app.state::<AppState>();
            let connection = db(&state)?;
            db::unity_editor_path(&connection)?
        };
        let os = crate::unity::Os::current();
        let editors =
            crate::unity::status(&selected, &crate::unity::Sources::for_current_os(), os).editors;
        match crate::unity_projects::editor_for_open(&project, &editors)? {
            None => Ok(crate::unity_projects::OpenResult { already_open: true }),
            Some(editor) => {
                crate::unity_projects::launch(
                    std::path::Path::new(&editor.path),
                    std::path::Path::new(&project.path),
                )?;
                Ok(crate::unity_projects::OpenResult {
                    already_open: false,
                })
            }
        }
    })
    .await
}

/// プロジェクトに入っている商品の 1 件（画面用）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProductView {
    pub booth_id: String,
    pub title: String,
    pub thumbnail_path: Option<String>,
    pub thumbnail_url: Option<String>,
    pub category: Option<String>,
    pub link: Option<crate::purchases::LibraryLink>,
    pub verdict: crate::unity_index::Verdict,
    pub packages: Vec<crate::unity_index_store::PackageInstall>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContentsView {
    pub scanned_at: Option<i64>,
    pub complete: bool,
    pub stale: bool,
    pub products: Vec<ProjectProductView>,
    /// 歩いた VPM パッケージの読みやすい名前（`jp.lilxyzw.liltoon` → lilToon）。「入っているパッケージ」に使う。
    pub package_names: std::collections::HashMap<String, String>,
}

/// 調べた結果に、ライブラリの題と行き先を付ける。除外した・ライブラリに無い商品は出さない。
fn project_contents_view(
    app: &AppHandle,
    contents: crate::unity_index_store::ProjectContents,
) -> Result<ProjectContentsView, String> {
    let ids: Vec<String> = contents
        .products
        .iter()
        .map(|product| product.booth_id.clone())
        .collect();
    let refs = {
        let state = app.state::<AppState>();
        let connection = db(&state)?;
        crate::purchases::product_refs(&connection, &ids)?
    };
    let package_names =
        crate::unity_index_jobs::read(app, crate::unity_index_store::vpm_display_names)
            .unwrap_or_default();
    Ok(ProjectContentsView {
        scanned_at: contents.scanned_at,
        complete: contents.complete,
        stale: contents.stale,
        package_names,
        products: contents
            .products
            .into_iter()
            .filter_map(|product| {
                let product_ref = refs.get(&product.booth_id)?.clone();
                Some(ProjectProductView {
                    booth_id: product.booth_id,
                    title: product_ref.title,
                    thumbnail_path: product_ref.thumbnail_path,
                    thumbnail_url: product_ref.thumbnail_url,
                    category: product_ref.category,
                    link: product_ref.link,
                    verdict: product.verdict,
                    packages: product.packages,
                })
            })
            .collect(),
    })
}

/// プロジェクトを調べる（docs/unity-projects.md の段階 2）。索引を新しくしてから `Assets` を歩く。読むだけ。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn scan_unity_project(
    view: tauri::Webview,
    app: AppHandle,
    projectId: String,
) -> Result<ProjectContentsView, String> {
    require_main(&view)?;
    unity_project_task(app, move |app| {
        let project = find_unity_project(app, &projectId)?;
        if project.state != "ok" {
            return Err("プロジェクトのフォルダを読めません".into());
        }
        let vpm: Vec<(String, String)> = project
            .packages
            .iter()
            .map(|package| (package.name.clone(), package.version.clone()))
            .collect();
        let contents = crate::unity_index_jobs::scan_project(
            app,
            &project.id,
            std::path::Path::new(&project.path),
            &vpm,
        )?;
        project_contents_view(app, contents)
    })
    .await
}

/// 前に調べた結果。調べ直さない。調べていなければ `scannedAt` が無い。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn unity_project_contents(
    app: AppHandle,
    projectId: String,
) -> Result<ProjectContentsView, String> {
    unity_project_task(app, move |app| {
        if !crate::unity_projects::valid_project_id(&projectId) {
            return Err("プロジェクトが見つかりません".into());
        }
        let contents = crate::unity_index_jobs::read(app, |connection| {
            crate::unity_index_store::project_contents(connection, &projectId)
        })?;
        project_contents_view(app, contents)
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductInstallView {
    pub project_id: String,
    pub name: String,
    pub verdict: crate::unity_index::Verdict,
}

/// 商品が入っていると分かったプロジェクト（調べたものだけ）。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn product_unity_installs(
    app: AppHandle,
    boothId: String,
) -> Result<Vec<ProductInstallView>, String> {
    unity_project_task(app, move |app| {
        let found = crate::unity_index_jobs::read(app, |connection| {
            crate::unity_index_store::product_projects(connection, &boothId)
        })?;
        if found.is_empty() {
            return Ok(Vec::new());
        }
        let projects = unity_project_list_blocking(app)?.projects;
        Ok(found
            .into_iter()
            .filter_map(|(project_id, verdict)| {
                let project = projects
                    .iter()
                    .find(|project| project.id == project_id && !project.hidden)?;
                Some(ProductInstallView {
                    project_id,
                    name: project.name.clone(),
                    verdict,
                })
            })
            .collect())
    })
    .await
}

/// 取り出した Unity パッケージを先頭から順に開く。`mode` は "unity"（設定の既定 Unity で
/// `-openfile`）か "system"（OS の関連付け）。"unity" で `target` があれば、選んだ Unity の窓へ直接渡す。
/// Unity の起動を待つときは途中で止まり、残りは呼び直す。
#[tauri::command]
pub async fn launch_unity_packages(
    view: tauri::Webview,
    app: AppHandle,
    paths: Vec<String>,
    mode: String,
    target: Option<crate::unity_import::UnityTarget>,
) -> Result<crate::unity_import::LaunchResult, String> {
    if view.label() != "main" {
        return Err("この画面から Unity パッケージは開けません".into());
    }
    let mode = crate::unity_import::LaunchMode::parse(&mode)?;
    if paths.is_empty() {
        return Err("開く Unity パッケージがありません".into());
    }
    let (library, remote) = library_roots(&app)?;
    let selected = {
        let state = app.state::<AppState>();
        let connection = db(&state)?;
        db::unity_editor_path(&connection)?
    };
    let cache_root = unity_cache_root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let packages = paths
            .iter()
            .map(|path| crate::unity_import::allowed_launch_path(path, &cache_root, &library, &remote))
            .collect::<Result<Vec<_>, _>>()?;
        // 渡す先のプロジェクトが分かれば、前に調べた結果を古いとする（取り込まれたかもしれないので）。
        let receiver = match (mode, target.as_ref()) {
            (crate::unity_import::LaunchMode::Unity, target) => instance_project(&app, target),
            _ => None,
        };
        let result = match (mode, target) {
            (crate::unity_import::LaunchMode::Unity, Some(target)) => {
                crate::unity_import::launch_into_instance(
                    &target,
                    &packages,
                    crate::unity_import::HANDOFF_GAP,
                )
            }
            (crate::unity_import::LaunchMode::Unity, None) => {
                let editor = crate::unity_import::editor_from_setting(&selected)?;
                let running = crate::unity_import::running_editor_paths();
                crate::unity_import::require_running_editor(&editor, running.as_deref())?;
                crate::unity_import::launch_with_editor(
                    &editor,
                    &packages,
                    crate::unity_import::HANDOFF_TIMEOUT,
                    crate::unity_import::HANDOFF_GAP,
                )
            }
            (crate::unity_import::LaunchMode::System, _) => {
                use tauri_plugin_opener::OpenerExt;
                for (index, package) in packages.iter().enumerate() {
                    if index > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                    }
                    app.opener()
                        .open_path(package.display().to_string(), None::<&str>)
                        .map_err(|_| {
                            "関連付けで開けませんでした。Unity Hub で .unitypackage の関連付けを確かめてください"
                                .to_string()
                        })?;
                }
                Ok(crate::unity_import::LaunchResult { launched: packages.len(), waiting: false })
            }
        };
        if let (Ok(launched), Some(project)) = (&result, receiver)
            && launched.launched > 0
        {
            crate::unity_index_jobs::mark_project_stale(&app, project.id);
        }
        result
    })
    .await
    .map_err(|err| format!("Unity パッケージを開けません: {err}"))?
}

/// 渡す先の Unity が開いているプロジェクト。`target` が無ければ、開いている Unity が 1 つのときだけ。
/// 窓の見出しと版で照らし、当てはまるプロジェクトがちょうど 1 つのときだけ返す（Windows のみ）。
fn instance_project(
    app: &AppHandle,
    target: Option<&crate::unity_import::UnityTarget>,
) -> Option<crate::unity_projects::UnityProject> {
    let instances = crate::unity_import::running_instances();
    let instance = match target {
        Some(target) => instances
            .into_iter()
            .find(|instance| instance.window == target.window && instance.pid == target.pid)?,
        None => {
            let mut instances = instances.into_iter();
            let only = instances.next()?;
            if instances.next().is_some() {
                return None;
            }
            only
        }
    };
    let projects = unity_project_list_blocking(app).ok()?.projects;
    let mut matched = projects.into_iter().filter(|project| {
        project.state == "ok"
            && !project.version.is_empty()
            && project.version == instance.version
            && crate::unity_projects::heading_names(&instance.heading, &project.name)
    });
    let project = matched.next()?;
    matched.next().is_none().then_some(project)
}

/// 渡す先のプロジェクトに足りないもの 1 つ（段階 3）。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageNeedView {
    /// "vpm"（ALCOM で足すパッケージ）、"vpmVersion"（入っているが、入っている版にそのファイルが無い）、
    /// "product"（ライブラリのほかの商品）。
    pub kind: &'static str,
    /// 見せる名前。VPM パッケージは `displayName`（無ければ名前そのもの）、商品は題。
    pub name: String,
    /// VPM パッケージの名前（`jp.lilxyzw.liltoon`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub booth_id: Option<String>,
}

/// 選んだもののうち、渡す先に同じ VPM パッケージが入っている同梱版。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundledPackageView {
    /// zip の中のパス（単体の .unitypackage は空）。
    pub entry: String,
    /// 入っている VPM パッケージの見せる名前。
    pub name: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageCheckView {
    pub project_id: String,
    pub project_name: String,
    /// その商品がもう入っている（前に調べた結果）。
    pub installed: bool,
    /// 選んだパッケージが必要としていて、このプロジェクトに無いもの。
    pub missing: Vec<PackageNeedView>,
    pub bundled: Vec<BundledPackageView>,
}

/// プロジェクトの種類を決める VRChat SDK。足すものではないので「無いもの」に並べない
/// （古いアバター用の商品が Worlds SDK の DLL を 1 つだけ指していた例がある。アバターのプロジェクトに
/// Worlds SDK を足すよう案内してしまうため）。
const PROJECT_KIND_PACKAGES: &[&str] = &["com.vrchat.avatars", "com.vrchat.worlds"];

/// `Packages/manifest.json` の `dependencies` にその名前があるか。Unity の Package Manager で入れた
/// パッケージ（git の URL など）は `vpm-manifest.json` にも `Packages/<名前>/` にも無い。
fn upm_manifest_lists(json: &str, name: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json.trim_start_matches('\u{feff}'))
        .ok()
        .and_then(|value| {
            value
                .get("dependencies")
                .and_then(|dependencies| dependencies.as_object())
                .map(|dependencies| dependencies.contains_key(name))
        })
        .unwrap_or(false)
}

/// プロジェクトにそのパッケージがあるか。`vpm-manifest.json`、`Packages/<名前>/`、`Packages/manifest.json` で
/// 見る（いまの状態）。どれかにあれば「無いもの」に並べない。
fn project_has_vpm(project: &crate::unity_projects::UnityProject, name: &str) -> bool {
    if project.packages.iter().any(|package| package.name == name) {
        return true;
    }
    if !crate::unity_index_store::vpm_folder_name(name) {
        return false;
    }
    let packages = std::path::Path::new(&project.path).join("Packages");
    packages.join(name).is_dir()
        || std::fs::read_to_string(packages.join("manifest.json"))
            .map(|json| upm_manifest_lists(&json, name))
            .unwrap_or(false)
}

/// 「このプロジェクトに無いもの」に VPM パッケージを並べるか。
fn reportable_vpm(project: &crate::unity_projects::UnityProject, name: &str) -> bool {
    !PROJECT_KIND_PACKAGES.contains(&name) && !project_has_vpm(project, name)
}

/// 「Unityで開く」の前の確かめ（docs/unity-projects.md の段階 2・3）。渡す先のプロジェクトが決まるとき、
/// その商品がもう入っているか、選んだパッケージがこのプロジェクトに無いものを必要としているか、
/// 選んだものに要らない同梱版があるかを返す。言うことが無い・渡す先が決まらないときは `None`（止めない）。
/// 足りないものは、パッケージの中の参照と、どこかのプロジェクトで見つけた持ち主だけから決め、説明文は読まない。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn unity_package_check(
    view: tauri::Webview,
    app: AppHandle,
    path: String,
    entries: Vec<String>,
    boothId: Option<String>,
    target: Option<crate::unity_import::UnityTarget>,
) -> Result<Option<PackageCheckView>, String> {
    require_main(&view)?;
    unity_project_task(app, move |app| {
        let Some(project) = instance_project(app, target.as_ref()) else {
            return Ok(None);
        };
        let source = std::path::PathBuf::from(&path);
        let (installed, needs, bundled, names) =
            crate::unity_index_jobs::read(app, |connection| {
                let installed = match &boothId {
                    Some(booth) => crate::unity_index_store::product_projects(connection, booth)?
                        .into_iter()
                        .any(|(id, verdict)| {
                            id == project.id && verdict == crate::unity_index::Verdict::Installed
                        }),
                    None => false,
                };
                let project_vpm: Vec<(String, String)> = project
                    .packages
                    .iter()
                    .map(|package| (package.name.clone(), package.version.clone()))
                    .collect();
                let needs = crate::unity_index_store::package_needs(
                    connection,
                    &project.id,
                    &source,
                    &entries,
                    &project_vpm,
                )?
                .unwrap_or_default();
                let bundled =
                    crate::unity_index_store::bundled_packages(connection, &source, &entries)?;
                let names = crate::unity_index_store::vpm_display_names(connection)?;
                Ok((installed, needs, bundled, names))
            })?;
        let display = |name: &str| names.get(name).cloned().unwrap_or_else(|| name.to_string());
        let product_ids: Vec<String> = needs
            .iter()
            .filter_map(|(owner, _)| match owner {
                crate::unity_index_store::NeedOwner::Product(booth) => Some(booth.clone()),
                _ => None,
            })
            .collect();
        let refs = if product_ids.is_empty() {
            std::collections::HashMap::new()
        } else {
            let state = app.state::<AppState>();
            let connection = db(&state)?;
            crate::purchases::product_refs(&connection, &product_ids)?
        };
        let missing: Vec<PackageNeedView> = needs
            .into_iter()
            .filter_map(|(owner, _)| match owner {
                crate::unity_index_store::NeedOwner::Vpm(name) => reportable_vpm(&project, &name)
                    .then(|| PackageNeedView {
                        kind: "vpm",
                        name: display(&name),
                        package: Some(name),
                        booth_id: None,
                    }),
                crate::unity_index_store::NeedOwner::VpmLacking(name) => {
                    (!PROJECT_KIND_PACKAGES.contains(&name.as_str())).then(|| PackageNeedView {
                        kind: "vpmVersion",
                        name: display(&name),
                        package: Some(name),
                        booth_id: None,
                    })
                }
                // 除外した・ライブラリに無い商品は言わない。
                crate::unity_index_store::NeedOwner::Product(booth) => {
                    if boothId.as_deref() == Some(booth.as_str()) {
                        return None;
                    }
                    let product = refs.get(&booth)?;
                    Some(PackageNeedView {
                        kind: "product",
                        name: product.title.clone(),
                        package: None,
                        booth_id: Some(booth),
                    })
                }
            })
            .collect();
        let bundled: Vec<BundledPackageView> = bundled
            .into_iter()
            .filter(|(_, name)| project_has_vpm(&project, name))
            .map(|(entry, name)| BundledPackageView {
                entry,
                name: display(&name),
            })
            .collect();
        if !installed && missing.is_empty() && bundled.is_empty() {
            return Ok(None);
        }
        Ok(Some(PackageCheckView {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            installed,
            missing,
            bundled,
        }))
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSuggestions {
    /// 新しい調べた結果がある。調べていない・古いときは偽で、候補を出さない（入っていないと言わない）。
    pub scanned: bool,
    /// 結び付けたアバターに対応していて、入っていないと分かった商品。手元にファイルがある順。
    pub items: Vec<crate::view::ItemView>,
}

/// 結び付けたアバターに対応していて、このプロジェクトに入っていないと分かった商品。
/// 対応はアバター詳細と同じ組み立て（`load_avatar`）で、判定は調べた結果（索引にあって入っていない）だけ。
/// 手元にファイルが無い商品は判定できないので入れない。
pub(crate) fn project_suggestions(
    app: &AppHandle,
    project: &crate::unity_projects::UnityProject,
) -> Result<ProjectSuggestions, String> {
    let contents = crate::unity_index_jobs::read(app, |connection| {
        crate::unity_index_store::project_contents(connection, &project.id)
    })?;
    // 調べていない・古い・途中までしか歩けなかった結果からは候補を出さない（入っていないと言わない）。
    if !crate::unity_index_store::absence_known(&contents) {
        return Ok(ProjectSuggestions {
            scanned: false,
            items: Vec::new(),
        });
    }
    let missing: std::collections::HashSet<&str> = contents
        .products
        .iter()
        .filter(|product| product.verdict == crate::unity_index::Verdict::Missing)
        .map(|product| product.booth_id.as_str())
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for avatar in &project.avatars {
        let Ok(detail) = load_avatar(app, &avatar.id, false) else {
            continue;
        };
        for item in detail
            .sections
            .into_iter()
            .flat_map(|section| section.items)
        {
            if missing.contains(item.booth_id.as_str()) && seen.insert(item.booth_id.clone()) {
                items.push(item);
            }
        }
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.local_file_count));
    Ok(ProjectSuggestions {
        scanned: true,
        items,
    })
}

/// AI エージェントが渡したプロジェクト（フォルダか名前）を一覧から探す。名前は 1 つに決まるときだけ。
pub(crate) fn agent_project(
    projects: Vec<crate::unity_projects::UnityProject>,
    query: &str,
) -> Result<crate::unity_projects::UnityProject, (u16, String)> {
    let os = crate::unity::Os::current();
    let by_path = crate::unity_projects::project_id(query, os);
    if let Some(project) = projects.iter().find(|project| project.id == by_path) {
        return Ok(project.clone());
    }
    let mut named = projects
        .into_iter()
        .filter(|project| project.name == query.trim());
    match (named.next(), named.next()) {
        (Some(project), None) => Ok(project),
        (Some(_), Some(_)) => Err((
            409,
            "同じ名前のプロジェクトが 2 つ以上あります。プロジェクトのフォルダを渡してください".into(),
        )),
        (None, _) => Err((
            404,
            "プロジェクトが見つかりません。ALCOM の一覧にあるプロジェクトか、VRCLibMan の「プロジェクト」で足したものだけを調べられます".into(),
        )),
    }
}

/// MCP の `unity_project`。前に調べた結果を返すだけで、調べ直さない。
pub(crate) fn agent_unity_project(
    app: &AppHandle,
    query: &str,
) -> Result<serde_json::Value, (u16, String)> {
    let internal = |error: String| (500, error);
    let projects = unity_project_list_blocking(app).map_err(internal)?.projects;
    let project = agent_project(projects, query)?;
    let contents = crate::unity_index_jobs::read(app, |connection| {
        crate::unity_index_store::project_contents(connection, &project.id)
    })
    .map_err(internal)?;
    let missing: Vec<String> = project_suggestions(app, &project)
        .map_err(internal)?
        .items
        .into_iter()
        .map(|item| item.booth_id)
        .collect();
    let state = app.state::<AppState>();
    let connection = db(&state).map_err(internal)?;
    crate::mcp_api::unity_project_reply(&connection, &project, &contents, &missing)
        .map_err(internal)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn unity_project_suggestions(
    app: AppHandle,
    projectId: String,
) -> Result<ProjectSuggestions, String> {
    unity_project_task(app, move |app| {
        let project = find_unity_project(app, &projectId)?;
        project_suggestions(app, &project)
    })
    .await
}

#[tauri::command]
pub fn list_staged_downloads(state: State<'_, AppState>) -> Result<Vec<StagedLeftover>, String> {
    state
        .staged_left
        .lock()
        .map(|left| left.clone())
        .map_err(|_| "一覧を読めません".to_string())
}

#[tauri::command]
pub fn route_staged_download(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let source = std::path::PathBuf::from(path.trim());
    {
        let mut connection = db(&state)?;
        db::route_staged_download(&mut connection, &source)?;
    }
    if let Ok(mut left) = state.staged_left.lock() {
        let shown = source.display().to_string();
        left.retain(|item| item.path != shown && item.path != path);
    }
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
pub fn add_free_product(state: State<'_, AppState>, url: String) -> Result<String, String> {
    let mut connection = db(&state)?;
    db::add_free_product(&mut connection, &url)
}

/// The result of 「リモートへコピー」 / 「ローカルへコピー」. `conflict` means another file
/// already has the name and nothing was written; the screen asks before `overwrite`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    status: &'static str,
    source_size: Option<u64>,
    existing_size: Option<u64>,
}

impl From<crate::library_files::CopyOutcome> for CopyResult {
    fn from(outcome: crate::library_files::CopyOutcome) -> Self {
        use crate::library_files::CopyOutcome;
        match outcome {
            CopyOutcome::Copied => Self {
                status: "copied",
                source_size: None,
                existing_size: None,
            },
            CopyOutcome::AlreadyThere => Self {
                status: "already",
                source_size: None,
                existing_size: None,
            },
            CopyOutcome::Conflict {
                source_size,
                existing_size,
            } => Self {
                status: "conflict",
                source_size: Some(source_size),
                existing_size: Some(existing_size),
            },
        }
    }
}

fn finish_copy(progress: &ProgressGuard, filename: &str, result: &Result<CopyResult, String>) {
    match result {
        Ok(result) if result.status == "conflict" => progress.done(format!(
            "{filename} は同じ名前の別のファイルがあるため、コピーしませんでした"
        )),
        Ok(result) if result.status == "already" => {
            progress.done(format!("{filename} はコピー先にあります"))
        }
        Ok(_) => progress.done(format!("{filename} をコピーしました")),
        Err(_) => progress.fail("コピーを中断しました"),
    }
}

#[tauri::command]
pub async fn pull_remote_file(
    app: AppHandle,
    booth_id: String,
    variation: String,
    filename: String,
    overwrite: Option<bool>,
) -> Result<CopyResult, String> {
    let label = filename.clone();
    let progress = ProgressGuard::new(app.clone(), "copy");
    progress.update(
        0,
        1,
        crate::progress::counted_message("コピー中", 1, 1, &label),
    );
    let worker = app.clone();
    let copied = tauri::async_runtime::spawn_blocking(move || {
        let (remote, library) = {
            let state = worker.state::<AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::require_download(&connection, &booth_id, &variation, &filename)?;
            let settings = db::settings(&connection)?;
            if settings.remote_library_folder.trim().is_empty() {
                return Err("リモートのフォルダが未設定です".into());
            }
            if settings.library_folder.trim().is_empty() {
                return Err("ライブラリのフォルダを選んでください".into());
            }
            (settings.remote_library_folder, settings.library_folder)
        };
        let (dest, outcome) = crate::library_files::pull_file(
            Path::new(remote.trim()),
            Path::new(library.trim()),
            &booth_id,
            &filename,
            overwrite.unwrap_or(false),
        )?;
        if matches!(outcome, crate::library_files::CopyOutcome::Conflict { .. }) {
            return Ok(CopyResult::from(outcome));
        }
        let state = worker.state::<AppState>();
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        db::note_downloaded_file(&connection, &booth_id, &variation, &filename, &dest)?;
        Ok(CopyResult::from(outcome))
    })
    .await
    .map_err(|err| format!("コピーを実行できません: {err}"))?;
    finish_copy(&progress, &label, &copied);
    copied
}

#[tauri::command]
pub async fn push_remote_file(
    app: AppHandle,
    booth_id: String,
    variation: String,
    filename: String,
    overwrite: Option<bool>,
) -> Result<CopyResult, String> {
    let label = filename.clone();
    let progress = ProgressGuard::new(app.clone(), "copy");
    progress.update(
        0,
        1,
        crate::progress::counted_message("コピー中", 1, 1, &label),
    );
    let worker = app.clone();
    let copied = tauri::async_runtime::spawn_blocking(move || {
        let (source, remote) = {
            let state = worker.state::<AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            let source = db::local_source_for(&connection, &booth_id, &variation, &filename)?;
            let settings = db::settings(&connection)?;
            if settings.remote_library_folder.trim().is_empty() {
                return Err("リモートのフォルダが未設定です".into());
            }
            (source, settings.remote_library_folder)
        };
        let (_, outcome) = crate::library_files::push_file(
            &source,
            Path::new(remote.trim()),
            &booth_id,
            &filename,
            overwrite.unwrap_or(false),
        )?;
        if matches!(outcome, crate::library_files::CopyOutcome::Conflict { .. }) {
            return Ok(CopyResult::from(outcome));
        }
        let size = std::fs::metadata(&source)
            .map(|meta| meta.len())
            .map_err(|err| format!("ファイルのサイズを確認できません: {err}"))?;
        let state = worker.state::<AppState>();
        let mut connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        db::remember_file_size(&connection, &booth_id, &variation, &filename, size)?;
        // A manual copy is the way back after a recorded file left the remote folder.
        crate::remote_sync::remember(&mut connection, &remote, &booth_id, &filename)?;
        Ok(CopyResult::from(outcome))
    })
    .await
    .map_err(|err| format!("コピーを実行できません: {err}"))?;
    finish_copy(&progress, &label, &copied);
    copied
}

#[tauri::command]
pub async fn open_booth_browser(app: tauri::AppHandle) -> Result<(), String> {
    crate::browser::dispatch_ui(app, crate::browser::open_browser).await
}

#[tauri::command]
pub async fn show_booth_library(app: tauri::AppHandle) -> Result<(), String> {
    crate::browser::dispatch_ui(app, crate::browser::show_library).await
}

#[tauri::command]
pub async fn browse_booth_history(app: tauri::AppHandle, forward: bool) -> Result<(), String> {
    crate::browser::dispatch_ui(app, move |handle| {
        crate::browser::browse_history(handle, forward)
    })
    .await
}

#[tauri::command]
pub async fn import_booth_page(
    app: tauri::AppHandle,
    webview: tauri::Webview,
) -> Result<ImportReport, String> {
    // ページ読み取りの待ちを UI スレッドに載せない。
    trusted_browser_controls(&webview)?;
    let handle = app.clone();
    let caller = webview.label().to_string();
    let report = tauri::async_runtime::spawn_blocking(move || {
        crate::browser::import_current_page(&handle, &caller, None)
    })
    .await
    .map_err(|err| err.to_string())??;
    let _ = app.emit("library-changed", ());
    Ok(report)
}

#[tauri::command]
pub async fn import_booth_library(
    app: tauri::AppHandle,
) -> Result<crate::browser::LibraryWalkReport, String> {
    // 移動と eval は import_library の中で dispatch_ui に渡す。
    let mut report = crate::browser::import_library(app.clone()).await?;
    let _ = app.emit("library-changed", ());
    // 最後まで読めたときだけ、続けて未分類を分類する。中止・途中停止では公開情報が揃っていない。
    if crate::auto_classify::after_walk(report.cancelled, &report.notice) {
        report.classifying = crate::auto_classify::request(&app);
    }
    Ok(report)
}

#[tauri::command]
pub fn cancel_booth_library() {
    crate::browser::request_library_walk_cancel();
}

#[tauri::command]
pub async fn import_booth_purchases(
    app: tauri::AppHandle,
) -> Result<crate::purchase_walk::PurchaseWalkReport, String> {
    // 移動と eval は import_purchases の中で dispatch_ui に渡す。
    crate::browser::import_purchases(app).await
}

#[tauri::command]
pub fn cancel_booth_purchases() {
    crate::browser::request_purchase_walk_cancel();
}

#[tauri::command(async)]
pub fn purchase_history(
    state: State<'_, AppState>,
    query: crate::purchases::PurchaseQuery,
) -> Result<crate::purchases::PurchasePage, String> {
    let connection = db(&state)?;
    crate::purchases::page(&connection, &query, crate::browser::purchase_walk_running())
}

#[tauri::command(async)]
pub fn purchase_status(
    state: State<'_, AppState>,
) -> Result<crate::purchases::PurchaseStatus, String> {
    let connection = db(&state)?;
    crate::purchases::status(&connection, crate::browser::purchase_walk_running())
}

#[tauri::command(async)]
pub fn purchase_summary(
    state: State<'_, AppState>,
    year: Option<i32>,
) -> Result<crate::purchases::PurchaseSummary, String> {
    let connection = db(&state)?;
    crate::purchases::summary(&connection, year)
}

#[tauri::command]
pub fn set_purchase_show_prices(state: State<'_, AppState>, show: bool) -> Result<(), String> {
    let connection = db(&state)?;
    crate::purchases::set_show_prices(&connection, show)
}

/// もらったギフトの今の価格を公開の商品情報から読む。`refresh` が偽なら未取得の商品だけ。
#[tauri::command]
pub async fn fetch_received_gift_prices(
    view: tauri::Webview,
    app: AppHandle,
    refresh: bool,
) -> Result<crate::received_gifts::GiftPriceReport, String> {
    if view.label() != "main" {
        return Err("この画面から価格は取得できません".into());
    }
    crate::received_gifts::fetch(&app, refresh).await
}

#[tauri::command]
pub fn create_purchase_label(
    state: State<'_, AppState>,
    name: String,
) -> Result<crate::purchases::PurchaseLabel, String> {
    let connection = db(&state)?;
    crate::purchases::create_label(&connection, &name)
}

#[tauri::command]
pub fn rename_purchase_label(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<(), String> {
    let connection = db(&state)?;
    crate::purchases::rename_label(&connection, id, &name)
}

#[tauri::command]
pub fn delete_purchase_label(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let mut connection = db(&state)?;
    crate::purchases::delete_label(&mut connection, id)
}

#[tauri::command]
pub fn set_purchase_item_labels(
    state: State<'_, AppState>,
    item_id: i64,
    label_ids: Vec<i64>,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    crate::purchases::set_item_labels(&mut connection, item_id, &label_ids)
}

#[tauri::command]
pub fn set_purchase_gift(
    state: State<'_, AppState>,
    order_id: String,
    gift: Option<bool>,
) -> Result<(), String> {
    let connection = db(&state)?;
    crate::purchases::set_gift_override(&connection, &order_id, gift)
}

#[tauri::command]
pub async fn clear_booth_session(app: tauri::AppHandle) -> Result<(), String> {
    crate::browser::clear_session(&app).await
}

/// Whether the built-in browser left a profile on this device. The settings screen offers to
/// delete it even while the feature is off.
#[tauri::command]
pub async fn booth_session_stored(app: tauri::AppHandle) -> bool {
    crate::browser::session_stored(&app).await
}

#[tauri::command]
pub async fn close_booth_browser(app: tauri::AppHandle) -> Result<(), String> {
    crate::browser::dispatch_ui(app, crate::browser::close_browser).await
}

#[tauri::command]
pub fn booth_browser_log(
    app: tauri::AppHandle,
) -> Result<crate::browser::BrowserDiagnostic, String> {
    crate::browser::diagnostic(&app)
}

#[tauri::command]
pub async fn booth_account(app: tauri::AppHandle) -> Result<crate::quiet::BoothAccount, String> {
    crate::browser::read_booth_account(app).await
}

#[tauri::command]
pub async fn download_booth_file(
    app: tauri::AppHandle,
    booth_id: String,
    variation: String,
    filename: String,
) -> Result<String, String> {
    crate::browser::download_one_file(app, booth_id, variation, filename).await
}

/// 「このアセットを更新しますか？」で更新するとき、ライブラリを読み直したあとに受け取るファイル。
/// 読むだけで、ダウンロードは画面が 1 件ずつ `download_booth_file` を呼ぶ。
#[tauri::command(async)]
pub fn asset_update_downloads(
    state: State<'_, AppState>,
    requests: Vec<view::UpdateRequest>,
) -> Result<Vec<view::UpdateTarget>, String> {
    if requests.len() > 500 {
        return Err("更新するアセットが多すぎます".into());
    }
    let requests: Vec<view::UpdateRequest> = requests
        .into_iter()
        .filter(|request| crate::library_files::valid_booth_id(request.booth_id.trim()))
        .map(|request| view::UpdateRequest {
            booth_id: request.booth_id.trim().to_string(),
            ..request
        })
        .collect();
    let (mut snapshot, remote) = {
        let connection = db(&state)?;
        let snapshot = db::load_snapshot(&connection)?;
        let remote = db::settings(&connection)?.remote_library_folder;
        (snapshot, remote)
    };
    // リモートにあるファイルは受け取り直さない。確かめるのは頼まれた商品だけ。
    let mut ids: Vec<String> = requests
        .iter()
        .map(|request| request.booth_id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    crate::library_files::probe_remote_for_products(&mut snapshot, &remote, &ids);
    Ok(view::update_download_targets(&snapshot, &requests))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeFilesReport {
    pub files: usize,
    /// 有料・一覧なしのように、確認できたが無料配布のファイルが無かったとき。何も保存していない。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

/// 「無料配布のファイルを探す」: one cookie-less public read for one product the user clicked.
/// Stores only free-download file rows. It does not download, buy, or add anything to BOOTH.
#[tauri::command]
pub async fn fetch_free_files(
    app: tauri::AppHandle,
    booth_id: String,
) -> Result<FreeFilesReport, String> {
    if !crate::library_files::valid_booth_id(&booth_id) {
        return Err("商品のIDを読めません".into());
    }
    let app_for_check = app.clone();
    let id_for_check = booth_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_for_check.state::<AppState>();
        let connection = db(&state)?;
        db::free_files_target(&connection, &id_for_check)
    })
    .await
    .map_err(|_| "商品を確認できません".to_string())??;
    let report = crate::free_files::fetch(&booth_id).await?;
    crate::browser::browser_log(
        &app,
        &crate::free_files::log_line(&booth_id, report.http_status, &report.outcome),
    );
    if let Some(message) = crate::free_files::nothing_found(&report.outcome) {
        return Ok(FreeFilesReport {
            files: 0,
            notice: Some(message.to_string()),
        });
    }
    if let Some(message) = crate::free_files::refusal(&report.outcome) {
        return Err(message.to_string());
    }
    let crate::free_files::FreeFilesOutcome::Free {
        files,
        is_free,
        is_adult,
    } = report.outcome
    else {
        return Err("商品情報を確認できません".into());
    };
    let app_for_store = app.clone();
    let count = tauri::async_runtime::spawn_blocking(move || {
        let state = app_for_store.state::<AppState>();
        let mut connection = db(&state)?;
        db::store_free_files(&mut connection, &booth_id, &files, is_free, is_adult)
    })
    .await
    .map_err(|_| "ファイル情報を保存できません".to_string())??;
    let _ = app.emit("library-changed", ());
    Ok(FreeFilesReport {
        files: count,
        notice: None,
    })
}

pub(crate) const PURCHASED_FILES_NOT_FOUND: &str = "BOOTH のライブラリの新しい購入の中に、この商品は見つかりませんでした。支払いが済んでいない注文か、ギフトとして贈った注文かもしれません。前に買った商品は、インポートの「ライブラリ全取得」で読み込めます";

/// 「購入したファイルを読み込む」: 本人が押した 1 商品のファイル一覧を、内蔵ブラウザのログインで
/// BOOTH のライブラリから読む（隠した子 WebView、表示中のタブは動かさない）。ファイル行と
/// バリエーションだけを保存し、ダウンロード・購入はしない。購入完了ページから登録した商品は
/// ファイル情報が無く、無料配布を探しても見つからないため。
#[tauri::command]
pub async fn fetch_purchased_files(
    app: tauri::AppHandle,
    booth_id: String,
) -> Result<FreeFilesReport, String> {
    if !crate::library_files::valid_booth_id(&booth_id) {
        return Err("商品のIDを読めません".into());
    }
    let app_for_check = app.clone();
    let id_for_check = booth_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_for_check.state::<AppState>();
        let connection = db(&state)?;
        if !db::settings(&connection)?.builtin_browser {
            return Err("内蔵ブラウザを有効にしてから押してください".to_string());
        }
        db::free_files_target(&connection, &id_for_check)
    })
    .await
    .map_err(|_| "商品を確認できません".to_string())??;
    let source = crate::browser_surface::active_view(&app)
        .map(|view| view.label().to_string())
        .unwrap_or_default();
    let wanted = vec![booth_id.clone()];
    let found = crate::browser::read_purchased_files(
        &app,
        &source,
        &wanted,
        crate::browser::PURCHASED_FILE_PAGES,
    )
    .await?;
    crate::browser::browser_log(
        &app,
        &format!(
            "購入したファイルの確認 {} {}",
            crate::booth_detail::id_tail(&booth_id),
            if found.is_empty() {
                "ライブラリになし"
            } else {
                "ライブラリにあり"
            }
        ),
    );
    if found.is_empty() {
        return Ok(FreeFilesReport {
            files: 0,
            notice: Some(PURCHASED_FILES_NOT_FOUND.to_string()),
        });
    }
    crate::browser::store_purchased_files(&app, found).await?;
    let id_for_count = booth_id.clone();
    let app_for_count = app.clone();
    let files = tauri::async_runtime::spawn_blocking(move || {
        let state = app_for_count.state::<AppState>();
        let connection = db(&state)?;
        db::product_file_count(&connection, &id_for_count)
    })
    .await
    .map_err(|_| "ファイル情報を確認できません".to_string())??;
    Ok(FreeFilesReport {
        files,
        notice: None,
    })
}

/// スキャン済みファイルと、記録済みの商品ファイルだけを既定のアプリで開く。
/// JS 側の opener に open-path 権限を足さず、Rust で登録済みのパスか確かめてから開く。
#[tauri::command]
pub async fn open_local_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let known = {
            let state = app.state::<AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::is_known_local_path(&connection, &path)?
        };
        if !known {
            return Err("ライブラリに登録されていない場所は開けません".to_string());
        }
        if std::fs::symlink_metadata(&path).is_err() {
            return Err("ファイルが見つかりません。フォルダを調べ直してください".to_string());
        }
        // 開くと実行される種類（プログラム、スクリプト、ショートカット）は既定のアプリへ渡さない。
        // 登録済みでも、押しただけで動かさないため。代わりに保存先のフォルダで示す。
        if crate::library_files::runs_when_opened(Path::new(&path)) {
            return Err(
                if tauri_plugin_opener::reveal_item_in_dir(&path).is_ok() {
                    "プログラムやショートカットは、ここからは開きません。保存先のフォルダを開きました"
                } else {
                    "プログラムやショートカットは、ここからは開きません。保存先のフォルダで確認してください"
                }
                .to_string(),
            );
        }
        tauri_plugin_opener::open_path(&path, None::<&str>)
            .map_err(|_| "ファイルを開けませんでした".to_string())
    })
    .await
    .map_err(|err| format!("ファイルを開けません: {err}"))?
}

#[tauri::command]
pub fn update_product(state: State<'_, AppState>, update: ProductUpdate) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::update_product(&mut connection, &update)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn move_product_genre(
    state: State<'_, AppState>,
    boothId: String,
    kind: String,
    genre: String,
    avatarIds: Option<Vec<String>>,
) -> Result<ClassificationSnapshot, String> {
    let mut connection = db(&state)?;
    db::move_product_genre_with_avatars(&mut connection, &boothId, &kind, &genre, avatarIds)
}

#[tauri::command]
pub fn restore_product_classification(
    state: State<'_, AppState>,
    snapshot: ClassificationSnapshot,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::restore_product_classification(&mut connection, &snapshot)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn unpin_product_classification(
    state: State<'_, AppState>,
    boothId: String,
) -> Result<ClassificationSnapshot, String> {
    let mut connection = db(&state)?;
    db::unpin_product_classification(&mut connection, &boothId)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn register_owned_body(
    state: State<'_, AppState>,
    boothId: String,
    name: Option<String>,
) -> Result<RegisterOwnedBodyResult, String> {
    let mut connection = db(&state)?;
    db::register_manual_owned_body(&mut connection, &boothId, name.as_deref())
}

#[tauri::command(async)]
pub fn classification_feedback(
    state: State<'_, AppState>,
    scope: String,
) -> Result<String, String> {
    let connection = db(&state)?;
    db::classification_feedback_json(&connection, &scope)
}

#[tauri::command]
pub fn save_classification_feedback(
    state: State<'_, AppState>,
    scope: String,
    path: String,
) -> Result<(), String> {
    let json = {
        let connection = db(&state)?;
        db::classification_feedback_json(&connection, &scope)?
    };
    db::save_classification_feedback_body(Path::new(&path), &json)
}

fn sync_export_app_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// The manifest alone, so every image reference is `null`. Only `save_sync_export` adds files.
#[tauri::command(async)]
pub fn sync_export_json(
    view: tauri::Webview,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if view.label() != "main" {
        return Err("この画面からデータ移行のファイルは書き出せません".into());
    }
    let mut connection = db(&state)?;
    let file = crate::sync_export::build(&mut connection, &sync_export_app_version(&app))?;
    crate::sync_export::to_json(&file)
}

/// The database lock covers reading the rows only, so copying images to a slow share cannot hold
/// up the app.
#[tauri::command]
pub async fn save_sync_export(
    view: tauri::Webview,
    app: AppHandle,
    path: String,
) -> Result<crate::sync_export::SyncExportReport, String> {
    if view.label() != "main" {
        return Err("この画面からデータ移行のファイルは書き出せません".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (file, images) = {
            let mut connection = db(&state)?;
            let file = crate::sync_export::build(&mut connection, &sync_export_app_version(&app))?;
            (file, crate::sync_export::package_images(&connection)?)
        };
        let counts = crate::sync_export::save_package(Path::new(&path), file, &images)?;
        Ok(crate::sync_export::SyncExportReport { counts })
    })
    .await
    .map_err(|err| format!("データ移行のファイルを保存できません: {err}"))?
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn attach_product_to_avatar(
    state: State<'_, AppState>,
    avatarId: String,
    boothId: String,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::attach_product_to_avatar(&mut connection, &avatarId, &boothId)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn link_file(
    state: State<'_, AppState>,
    path: String,
    productId: Option<String>,
) -> Result<(), String> {
    let connection = db(&state)?;
    db::link_file(&connection, &path, productId.as_deref())
}

#[tauri::command]
pub fn save_avatar(state: State<'_, AppState>, avatar: AvatarInput) -> Result<String, String> {
    let mut connection = db(&state)?;
    db::save_avatar(&mut connection, &avatar)
}

#[tauri::command]
pub fn delete_avatar(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::delete_avatar(&mut connection, &id)?;
    let _ = crate::hero_image::prune(&connection, &state.hero_images_dir);
    Ok(())
}

/// Copies the chosen image into `hero_images` and returns the stored path. Main window only.
#[tauri::command]
#[allow(non_snake_case)]
pub async fn set_avatar_hero_image(
    view: tauri::Webview,
    app: AppHandle,
    avatarId: String,
    sourcePath: String,
) -> Result<String, String> {
    if view.label() != "main" {
        return Err("この画面からヒーロー画像は変更できません".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let prepared = crate::hero_image::prepare(std::path::Path::new(&sourcePath))?;
        let state = app.state::<AppState>();
        let mut connection = db(&state)?;
        let stored = crate::hero_image::store(
            &mut connection,
            &state.hero_images_dir,
            &avatarId,
            &prepared,
        )?;
        Ok(stored.display().to_string())
    })
    .await
    .map_err(|err| format!("画像を保存できません: {err}"))?
}

/// Back to the thumbnail. Main window only.
#[tauri::command]
#[allow(non_snake_case)]
pub fn clear_avatar_hero_image(
    view: tauri::Webview,
    state: State<'_, AppState>,
    avatarId: String,
) -> Result<(), String> {
    if view.label() != "main" {
        return Err("この画面からヒーロー画像は変更できません".into());
    }
    let connection = db(&state)?;
    crate::hero_image::clear(&connection, &state.hero_images_dir, &avatarId)
}

#[tauri::command]
pub fn save_base_body(state: State<'_, AppState>, body: BaseBodyInput) -> Result<String, String> {
    let connection = db(&state)?;
    db::save_base_body(&connection, &body)
}

#[tauri::command]
pub fn delete_base_body(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let connection = db(&state)?;
    db::delete_base_body(&connection, &id)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn merge_avatars(
    state: State<'_, AppState>,
    fromId: String,
    intoId: String,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::merge_avatars(&mut connection, &fromId, &intoId)?;
    let _ = crate::hero_image::prune(&connection, &state.hero_images_dir);
    Ok(())
}

#[tauri::command]
pub fn restore_seed(state: State<'_, AppState>) -> Result<u32, String> {
    let mut connection = db(&state)?;
    db::restore_seed(&mut connection)
}

#[tauri::command]
pub fn reresolve(state: State<'_, AppState>) -> Result<u32, String> {
    let mut connection = db(&state)?;
    db::reapply_catalog(&mut connection)
}

#[tauri::command]
pub async fn scan_library(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ScanReport, String> {
    let root = path.clone();
    let progress = ProgressGuard::new(app.clone(), "scan");
    progress.update(0, 0, "フォルダを調べています");
    let outcome = tauri::async_runtime::spawn_blocking(move || scan::scan_folder(Path::new(&root)))
        .await
        .map_err(|err| format!("スキャンを実行できません: {err}"))??;
    let mut connection = db(&state)?;
    let report = db::apply_library_scan(&mut connection, &path, &outcome)?;
    crate::unity_index_jobs::wake(&app);
    progress.done_with(
        report.indexed,
        format!(
            "{} 件を調べ、{} 件を商品と紐づけました",
            report.indexed, report.linked
        ),
    );
    Ok(report)
}

#[tauri::command(async)]
pub fn classification_status(
    state: State<'_, crate::AppState>,
) -> Result<crate::model::ClassificationStatus, String> {
    let connection = db(&state)?;
    db::classification_status(&connection)
}

#[tauri::command]
pub async fn extract_products(app: AppHandle, target: String) -> Result<ExtractReport, String> {
    let report = run_extract(&app, &target).await?;
    // 取り込みのあとの分類がこの実行の間に頼まれていたら、続けて行う（中断したときは捨てる）。
    crate::auto_classify::after_run(&app, report.cancelled);
    Ok(report)
}

/// 分類がすでに走っているときの `run_extract` のエラー。
pub(crate) const EXTRACT_ALREADY_RUNNING: &str = "分類はすでに実行中です";

/// 分類の本体。画面の「未分類を分類」などと、取り込みのあとの自動の分類（`auto_classify`）が使う。
pub(crate) async fn run_extract(app: &AppHandle, target: &str) -> Result<ExtractReport, String> {
    let state = app.state::<AppState>();
    let running = Arc::clone(&state.extract_running);
    if running.swap(true, Ordering::SeqCst) {
        return Err(EXTRACT_ALREADY_RUNNING.into());
    }
    state.cancel_extract.store(false, Ordering::SeqCst);
    let _clear = ClearRunning(running);
    let progress = ProgressGuard::new(app.clone(), "extract");
    let (jobs, threshold) = {
        let connection = db(&state)?;
        let jobs = db::products_for_extract(&connection, db::parse_extract_target(target)?)?;
        (jobs, db::settings(&connection)?.confidence_threshold)
    };
    // The log's run. Each product locks the database and logs on its own, so an edit made
    // between two products is not attributed to this run.
    let mut run =
        db::ClassificationRun::new(db::KIND_RECLASSIFY, db::CLASSIFIER_RULES).with_note(target);
    let total = jobs.len() as u32;
    let mut processed = 0;
    let mut booth_settled = 0;
    let mut rule_filled = 0;
    let mut cancelled = false;
    for (index, job) in jobs.iter().enumerate() {
        if state.cancel_extract.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        classify_job(
            app,
            &state,
            &mut run,
            job,
            threshold,
            &mut processed,
            &mut booth_settled,
            &mut rule_filled,
        )?;
        let label: String = job.title.chars().take(40).collect();
        progress.update(
            index as u32 + 1,
            total,
            crate::progress::counted_message("分類中", index as u32 + 1, total, &label),
        );
        // Rules run without waiting on anything. Yield so a long batch does not hold a
        // runtime thread, and so 中断 is seen between products.
        tokio::task::yield_now().await;
    }
    crate::browser::browser_log(
        app,
        &crate::browser::classification_done_line(processed, booth_settled, rule_filled, cancelled),
    );
    if processed > 0 {
        progress.update(total, total, "分類結果を整理しています");
        let mut connection = db(&state)?;
        db::reresolve_all_in_run(&mut connection, &mut run)?;
    }
    if cancelled {
        progress.cancel("分類を中断しました");
    } else {
        progress.done_with(
            total,
            format!(
                "{processed} 件分類（カテゴリとタグ {booth_settled}、タイトルと説明 {rule_filled}）"
            ),
        );
    }
    Ok(ExtractReport {
        processed,
        cancelled,
        booth_settled,
        rule_filled,
    })
}

/// One product: a closed listing, a settled catalog or owned body, the BOOTH category and
/// tags, and otherwise the title and description rules.
#[allow(clippy::too_many_arguments)]
fn classify_job(
    app: &AppHandle,
    state: &State<'_, AppState>,
    run: &mut db::ClassificationRun,
    job: &ExtractJob,
    threshold: f64,
    processed: &mut u32,
    booth_settled: &mut u32,
    rule_filled: &mut u32,
) -> Result<(), String> {
    let mut connection = db(state)?;
    if db::product_excluded(&connection, &job.booth_id)? {
        return Ok(());
    }
    if job.description == crate::model::UNAVAILABLE_DESCRIPTION {
        db::observe(&mut connection, run, &job.booth_id, |connection| {
            db::mark_description_seen(connection, &job.booth_id)
        })?;
        *processed += 1;
        return Ok(());
    }
    if db::observe(&mut connection, run, &job.booth_id, |connection| {
        db::skip_settled(connection, &job.booth_id)
    })? {
        *processed += 1;
        return Ok(());
    }
    if db::observe(&mut connection, run, &job.booth_id, |connection| {
        db::apply_booth_settlement(connection, &job.booth_id)
    })? {
        *booth_settled += 1;
        *processed += 1;
        return Ok(());
    }
    if crate::booth_category::category_rule(job.booth_category_id, &job.booth_category_name)
        == crate::booth_category::CategoryRule::Unknown
    {
        crate::browser::log_unknown_category_once(
            app,
            job.booth_category_id,
            &job.booth_category_name,
            &job.booth_category_parent,
        );
    }
    let extraction = crate::classifier::rule_extraction(&job.title, &job.description);
    db::apply_extraction_in_run(&mut connection, run, &job.booth_id, &extraction, threshold)?;
    *rule_filled += 1;
    *processed += 1;
    Ok(())
}

#[tauri::command]
pub fn cancel_extract(state: State<'_, AppState>) {
    state.cancel_extract.store(true, Ordering::SeqCst);
}

#[tauri::command(async)]
pub fn catalog_info(state: State<'_, AppState>) -> Result<CatalogInfo, String> {
    let connection = db(&state)?;
    db::catalog_info(&connection)
}

#[tauri::command]
pub async fn refresh_avatar_catalog(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CatalogRefresh, String> {
    let progress = ProgressGuard::new(app.clone(), "catalog");
    progress.update(0, 0, "プロファイル一覧を取得しています");
    let client = crate::catalog::client()?;
    let response = client
        .get(crate::catalog::DATA_URL)
        .header("user-agent", "VRCLibMan")
        .send()
        .await
        .map_err(|err| format!("プロファイル一覧を取得できません: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "プロファイル一覧を取得できません: {}",
            response.status()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| format!("プロファイル一覧を読めません: {err}"))?;
    if bytes.len() > 8_000_000 {
        return Err("プロファイル一覧が大きすぎます".into());
    }
    let text = String::from_utf8(bytes.to_vec())
        .map_err(|_| "プロファイル一覧の文字コードを読めません".to_string())?;
    let parsed = crate::catalog::parse_catalog_json(&text)?;
    let mut connection = db(&state)?;
    let report = db::install_catalog(&mut connection, &parsed)?;
    progress.done_with(
        report.total,
        format!("対応表を更新しました（{} 件）", report.total),
    );
    let _ = app.emit("library-changed", ());
    Ok(report)
}

#[tauri::command(async)]
pub fn setup_status(state: State<'_, AppState>) -> Result<SetupStatus, String> {
    let connection = db(&state)?;
    db::setup_status(&connection)
}

#[tauri::command]
pub fn complete_setup(state: State<'_, AppState>) -> Result<(), String> {
    let connection = db(&state)?;
    db::set_setup_complete(&connection, true)
}

#[tauri::command]
pub fn reopen_setup(state: State<'_, AppState>) -> Result<(), String> {
    let connection = db(&state)?;
    db::set_setup_complete(&connection, false)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn mark_owned_avatar(state: State<'_, AppState>, boothId: String) -> Result<String, String> {
    let mut connection = db(&state)?;
    db::mark_owned_body(&mut connection, &boothId)
}

#[tauri::command]
pub async fn cache_thumbnails(app: AppHandle, state: State<'_, AppState>) -> Result<u32, String> {
    let jobs = {
        let connection = db(&state)?;
        db::thumbnail_jobs(&connection)?
    };
    let progress = ProgressGuard::new(app.clone(), "thumbs");
    let thumbs = state.thumbs_dir.clone();
    let client = crate::thumb::download_client()?;
    let total = jobs.len() as u32;
    let mut saved = 0u32;
    for (index, job) in jobs.iter().enumerate() {
        if let Some(path) = &job.thumbnail_path
            && Path::new(path).is_file()
        {
            continue;
        }
        progress.update(
            index as u32,
            total,
            format!("サムネイルを保存しています ({}/{total})", index + 1),
        );
        if let Ok(path) =
            crate::thumb::download_thumbnail(&client, &job.thumbnail_url, &thumbs, &job.booth_id)
                .await
        {
            let connection = db(&state)?;
            db::set_thumbnail_path(&connection, &job.booth_id, &path)?;
            saved += 1;
        }
    }
    progress.done_with(total, format!("{saved} 件のサムネイルを保存しました"));
    Ok(saved)
}

fn preview_fetch_lock() -> &'static tauri::async_runtime::Mutex<()> {
    static LOCK: std::sync::OnceLock<tauri::async_runtime::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| tauri::async_runtime::Mutex::new(()))
}

/// 選択中の商品の大きい画像を1件扱う。`download` が false のときは保存済みだけを返す。
/// 年齢制限の商品は `reveal` まで大きい画像を返さない。
#[tauri::command]
#[allow(non_snake_case)]
pub async fn cache_product_preview(
    state: State<'_, AppState>,
    boothId: String,
    download: bool,
    reveal: bool,
) -> Result<Option<String>, String> {
    if !crate::preview_image::valid_preview_id(&boothId) {
        return Err("商品IDが不正です".into());
    }
    let source = {
        let connection = db(&state)?;
        db::product_preview_source(&connection, &boothId)?
    };
    let Some(source) = source else {
        return Ok(None);
    };
    if !crate::preview_image::may_show_large(source.is_adult, reveal) {
        return Ok(None);
    }
    let Some(raw) = source.thumbnail_url.filter(|url| !url.trim().is_empty()) else {
        return Ok(None);
    };
    let original = match crate::preview_image::original_image_url(&raw) {
        Ok(url) => url,
        Err(_) => return Ok(None),
    };
    let directory = state.previews_dir.clone();
    if let Some(path) =
        crate::preview_image::cached_preview(&directory, &boothId, original.as_str())
    {
        return Ok(Some(path.display().to_string()));
    }
    if !download {
        return Ok(None);
    }
    let client = match crate::thumb::download_client() {
        Ok(client) => client,
        Err(_) => return Ok(None),
    };
    let _guard = preview_fetch_lock().lock().await;
    if let Some(path) =
        crate::preview_image::cached_preview(&directory, &boothId, original.as_str())
    {
        return Ok(Some(path.display().to_string()));
    }
    match crate::preview_image::download_preview(&client, &original, &directory, &boothId).await {
        Ok(path) => Ok(Some(path.display().to_string())),
        Err(_) => Ok(None),
    }
}

fn trusted_browser_controls(view: &tauri::Webview) -> Result<(), String> {
    let label = view.label();
    if label == "main" || crate::browser_tabs::is_toolbar_label(label) {
        Ok(())
    } else {
        Err("この画面からブラウザを操作する権限がありません".into())
    }
}

/// A toolbar acts on its own window. The main window passes an explicit host when it means the panel.
fn caller_host(label: &str) -> Option<String> {
    let inferred = crate::browser_surface::host_id_for_label(label)?;
    if label == crate::browser_surface::PANEL_WINDOW_LABEL {
        None
    } else {
        Some(inferred)
    }
}

/// Only the main window measures its own right panel.
fn main_window_only(label: &str) -> Result<(), String> {
    if label == crate::browser_surface::PANEL_WINDOW_LABEL {
        Ok(())
    } else {
        Err("この画面からブラウザの表示場所は変えられません".into())
    }
}

#[derive(serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BrowserAction {
    NewTab {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        host_id: Option<String>,
    },
    Navigate {
        url: String,
        #[serde(default)]
        tab_id: Option<String>,
        #[serde(default)]
        host_id: Option<String>,
    },
    SelectTab {
        tab_id: String,
    },
    CloseTab {
        tab_id: String,
    },
    Back {
        #[serde(default)]
        tab_id: Option<String>,
        #[serde(default)]
        host_id: Option<String>,
    },
    Forward {
        #[serde(default)]
        tab_id: Option<String>,
        #[serde(default)]
        host_id: Option<String>,
    },
    Reload {
        #[serde(default)]
        tab_id: Option<String>,
        #[serde(default)]
        host_id: Option<String>,
    },
    Stop {
        #[serde(default)]
        tab_id: Option<String>,
        #[serde(default)]
        host_id: Option<String>,
    },
    Zoom {
        action: crate::browser_tabs::ZoomChange,
        #[serde(default)]
        tab_id: Option<String>,
        #[serde(default)]
        host_id: Option<String>,
    },
    ToolbarHeight {
        height: f64,
        #[serde(default)]
        host_id: Option<String>,
    },
    /// Move one window's pages into the right panel, or every separate window when omitted.
    Dock {
        area: Option<PanelRect>,
        #[serde(default)]
        host_id: Option<String>,
    },
    /// Move the right panel's pages into one new window.
    Undock,
    /// The right panel's page area, or None while that tab is not on screen.
    PanelArea {
        area: Option<PanelRect>,
        visible: bool,
    },
    ReorderTabs {
        host_id: String,
        tab_ids: Vec<String>,
    },
    DetachTab {
        tab_id: String,
        x: f64,
        y: f64,
    },
    DockTab {
        tab_id: String,
        #[serde(default)]
        index: Option<usize>,
    },
}

/// A rectangle in the main window, in CSS (logical) pixels.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PanelRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

fn panel_rect(
    rect: Option<PanelRect>,
) -> Result<Option<crate::browser_surface::SurfaceArea>, String> {
    rect.map(|rect| {
        crate::browser_surface::SurfaceArea::from_panel(
            rect.left,
            rect.top,
            rect.width,
            rect.height,
        )
    })
    .transpose()
}

#[tauri::command]
pub async fn browser_state(
    app: AppHandle,
    webview: tauri::Webview,
) -> Result<crate::browser_tabs::BrowserState, String> {
    trusted_browser_controls(&webview)?;
    crate::browser::dispatch_ui(app, crate::browser_surface::state).await
}

#[tauri::command]
pub async fn browser_panel_snapshot(
    app: AppHandle,
    webview: tauri::Webview,
) -> Result<Option<String>, String> {
    trusted_browser_controls(&webview)?;
    main_window_only(webview.label())?;
    crate::browser_surface::panel_snapshot(&app).await
}

#[tauri::command]
pub async fn browser_action(
    app: AppHandle,
    webview: tauri::Webview,
    action: BrowserAction,
) -> Result<crate::browser_tabs::BrowserState, String> {
    trusted_browser_controls(&webview)?;
    let label = webview.label().to_string();
    crate::browser::dispatch_ui(app, move |handle| {
        use crate::browser_surface as surface;
        match action {
            BrowserAction::NewTab { url, host_id } => {
                match host_id.or_else(|| caller_host(&label)) {
                    Some(host) => {
                        surface::new_tab_on(handle, url.as_deref(), Some(host))?;
                        if let Some(window) = surface::host_window(handle) {
                            let _ = window.show();
                            let _ = window.unminimize();
                            let _ = window.set_focus();
                        }
                    }
                    None => surface::new_tab(handle, url.as_deref())?,
                }
            }
            BrowserAction::Navigate {
                url,
                tab_id,
                host_id,
            } => {
                let id = surface::tab_for_caller(&label, tab_id, host_id)?;
                surface::navigate_tab(handle, &id, &url)?;
            }
            BrowserAction::SelectTab { tab_id } => surface::select(handle, &tab_id)?,
            BrowserAction::CloseTab { tab_id } => surface::close_tab(handle, &tab_id)?,
            BrowserAction::Back { tab_id, host_id } => {
                let id = surface::tab_for_caller(&label, tab_id, host_id)?;
                surface::history_tab(handle, &id, false)?;
            }
            BrowserAction::Forward { tab_id, host_id } => {
                let id = surface::tab_for_caller(&label, tab_id, host_id)?;
                surface::history_tab(handle, &id, true)?;
            }
            BrowserAction::Reload { tab_id, host_id } => {
                if tab_id.is_none() && host_id.is_none() && caller_host(&label).is_none() {
                    surface::reload(handle)?;
                } else {
                    let id = surface::tab_for_caller(&label, tab_id, host_id)?;
                    surface::reload_tab(handle, &id)?;
                }
            }
            BrowserAction::Stop { tab_id, host_id } => {
                if tab_id.is_none() && host_id.is_none() && caller_host(&label).is_none() {
                    surface::stop(handle)?;
                } else {
                    let id = surface::tab_for_caller(&label, tab_id, host_id)?;
                    surface::stop_tab(handle, &id)?;
                }
            }
            BrowserAction::Zoom {
                action,
                tab_id,
                host_id,
            } => {
                let id = surface::tab_for_caller(&label, tab_id, host_id)?;
                surface::zoom_tab(handle, &id, action)?;
            }
            BrowserAction::ToolbarHeight { height, host_id } => {
                let host = host_id.or_else(|| caller_host(&label));
                surface::toolbar_height(handle, height, host)?;
            }
            // The separate window's "パネルに戻す" sends no rectangle; the panel reports it next.
            BrowserAction::Dock { area, host_id } => {
                let host = host_id.or_else(|| caller_host(&label));
                surface::dock(handle, panel_rect(area)?, host)?;
            }
            BrowserAction::Undock => surface::undock(handle)?,
            BrowserAction::PanelArea { area, visible } => {
                main_window_only(&label)?;
                surface::panel_area(handle, panel_rect(area)?, visible)?;
                // Only placement changed. Sent on every move of the panel, so the pages are not
                // asked for their address and history here.
                return surface::current_state(handle);
            }
            BrowserAction::ReorderTabs { host_id, tab_ids } => {
                surface::reorder(handle, &host_id, &tab_ids)?;
            }
            BrowserAction::DetachTab { tab_id, x, y } => {
                surface::detach_tab(handle, &tab_id, x, y)?
            }
            BrowserAction::DockTab { tab_id, index } => surface::dock_tab(handle, &tab_id, index)?,
        }
        surface::state(handle)
    })
    .await
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn save_browser_bookmark(
    app: AppHandle,
    webview: tauri::Webview,
    originalUrl: Option<String>,
    title: String,
    url: String,
) -> Result<crate::browser_tabs::BrowserState, String> {
    trusted_browser_controls(&webview)?;
    crate::browser::dispatch_ui(app, move |handle| {
        let bookmarks = {
            let state = handle.state::<AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::save_browser_bookmark(&connection, originalUrl.as_deref(), &title, &url)?;
            db::browser_bookmarks(&connection)?
        };
        crate::browser_surface::set_bookmarks(handle, bookmarks);
        crate::browser_surface::state(handle)
    })
    .await
}

#[tauri::command]
pub async fn delete_browser_bookmark(
    app: AppHandle,
    webview: tauri::Webview,
    url: String,
) -> Result<crate::browser_tabs::BrowserState, String> {
    trusted_browser_controls(&webview)?;
    crate::browser::dispatch_ui(app, move |handle| {
        let bookmarks = {
            let state = handle.state::<AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::delete_browser_bookmark(&connection, &url)?;
            db::browser_bookmarks(&connection)?
        };
        crate::browser_surface::set_bookmarks(handle, bookmarks);
        crate::browser_surface::state(handle)
    })
    .await
}

#[cfg(test)]
mod agent_project_tests {
    use super::agent_project;
    use crate::unity_projects::{UnityProject, project_id};

    fn project(path: &str) -> UnityProject {
        let os = crate::unity::Os::current();
        UnityProject {
            id: project_id(path, os),
            name: path.rsplit(['\\', '/']).next().unwrap().into(),
            path: path.into(),
            state: "ok",
            version: "2022.3.22f1".into(),
            kind: "avatar",
            packages: vec![],
            source: "alcom",
            hidden: false,
            running: "closed",
            editor_available: true,
            avatars: vec![],
            modified: None,
        }
    }

    #[test]
    fn agents_find_projects_by_folder_or_unique_name() {
        let sep = std::path::MAIN_SEPARATOR;
        let a = format!("{sep}work{sep}Plum");
        let b = format!("{sep}old{sep}Plum");
        let c = format!("{sep}work{sep}Room");
        let all = || vec![project(&a), project(&b), project(&c)];
        assert_eq!(agent_project(all(), &a).unwrap().path, a);
        assert_eq!(agent_project(all(), &format!("{b}{sep}")).unwrap().path, b);
        assert_eq!(agent_project(all(), "Room").unwrap().path, c);
        assert_eq!(agent_project(all(), "Plum").unwrap_err().0, 409);
        assert_eq!(agent_project(all(), "Nothing").unwrap_err().0, 404);
    }
}

#[cfg(test)]
mod browser_action_tests {
    use super::BrowserAction;

    #[test]
    fn older_browser_actions_parse_and_new_fields_stay_optional() {
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"newTab"}"#).unwrap();
        assert!(matches!(
            action,
            BrowserAction::NewTab {
                url: None,
                host_id: None
            }
        ));
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"back"}"#).unwrap();
        assert!(matches!(
            action,
            BrowserAction::Back {
                tab_id: None,
                host_id: None
            }
        ));
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"undock"}"#).unwrap();
        assert!(matches!(action, BrowserAction::Undock));
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"dock","area":null}"#).unwrap();
        assert!(matches!(
            action,
            BrowserAction::Dock {
                area: None,
                host_id: None
            }
        ));
        let action: BrowserAction =
            serde_json::from_str(r#"{"kind":"detachTab","tabId":"booth-tab-1","x":-10.0,"y":4.0}"#)
                .unwrap();
        assert!(matches!(
            action,
            BrowserAction::DetachTab { ref tab_id, x, y }
                if tab_id == "booth-tab-1" && x == -10.0 && y == 4.0
        ));
        assert!(
            serde_json::from_str::<BrowserAction>(
                r#"{"kind":"navigate","url":"https://example.com","extra":1}"#
            )
            .is_err()
        );
    }
}

#[tauri::command(async)]
#[allow(non_snake_case)]
pub fn product_avatar_suggestions(
    state: State<'_, AppState>,
    boothId: String,
) -> Result<db::AvatarSuggestions, String> {
    let connection = db(&state)?;
    db::product_avatar_suggestions(&connection, &boothId)
}

/// 「取得した商品の分類」で最初に選んでおく候補。読むだけで保存しない。
#[tauri::command(async)]
#[allow(non_snake_case)]
pub fn product_genre_suggestion(
    state: State<'_, AppState>,
    boothId: String,
) -> Result<db::GenreSuggestion, String> {
    let connection = db(&state)?;
    db::product_genre_suggestion(&connection, &boothId)
}

#[tauri::command(async)]
pub fn browser_auto_pending(
    state: State<'_, AppState>,
) -> Result<Vec<db::BrowserAutoItem>, String> {
    let connection = db(&state)?;
    db::browser_auto_pending(&connection)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn browser_auto_finish(
    app: AppHandle,
    state: State<'_, AppState>,
    boothId: String,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::browser_auto_finish(&mut connection, &boothId)?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command(async)]
pub fn asset_update_status(
    view: tauri::Webview,
    state: State<'_, AppState>,
) -> Result<crate::asset_updates::Status, String> {
    if view.label() != "main" {
        return Err("この画面からアセットの更新を確認する権限がありません".into());
    }
    let connection = db(&state)?;
    crate::asset_updates::status(&connection)
}

#[tauri::command(async)]
pub fn asset_update_history(
    view: tauri::Webview,
    state: State<'_, AppState>,
) -> Result<Vec<crate::asset_updates::UpdateRecord>, String> {
    trusted_updater(&view)?;
    let connection = db(&state)?;
    crate::asset_updates::history(&connection)
}

#[tauri::command]
pub fn configure_asset_updates(
    view: tauri::Webview,
    state: State<'_, AppState>,
    input: crate::asset_updates::Configure,
) -> Result<crate::asset_updates::Status, String> {
    if view.label() != "main" {
        return Err("この画面からアセットの更新を確認する権限がありません".into());
    }
    let mut connection = db(&state)?;
    crate::asset_updates::configure(&mut connection, input)
}

#[tauri::command]
pub async fn check_asset_updates(
    view: tauri::Webview,
    app: AppHandle,
) -> Result<crate::asset_updates::Status, String> {
    if view.label() != "main" {
        return Err("この画面からアセットの更新を確認する権限がありません".into());
    }
    crate::asset_updates::check(&app, true).await
}

#[tauri::command]
pub async fn preview_existing_import(
    view: tauri::Webview,
    app: AppHandle,
    path: String,
) -> Result<crate::existing_import::Preview, String> {
    trusted_updater(&view)?;
    let rows = {
        let state = app.state::<AppState>();
        let connection = db(&state)?;
        crate::existing_import::catalog(&connection)?
    };
    tauri::async_runtime::spawn_blocking(move || {
        crate::existing_import::preview(rows, Path::new(&path))
    })
    .await
    .map_err(|_| "フォルダを調べられません")?
}
#[tauri::command]
pub async fn register_existing_import(
    view: tauri::Webview,
    app: AppHandle,
    path: String,
    selections: Vec<crate::existing_import::Selection>,
    copy: bool,
) -> Result<crate::existing_import::Report, String> {
    trusted_updater(&view)?;
    let report = tauri::async_runtime::spawn_blocking({
        let app = app.clone();
        move || crate::existing_import::register(&app, Path::new(&path), selections, copy)
    })
    .await
    .map_err(|_| "取り込みを実行できません")??;
    crate::unity_index_jobs::wake(&app);
    Ok(report)
}

#[cfg(test)]
mod package_check_tests {
    use super::*;

    fn project(path: &std::path::Path, packages: &[&str]) -> crate::unity_projects::UnityProject {
        crate::unity_projects::UnityProject {
            id: "0123456789abcdef".into(),
            name: "P".into(),
            path: path.display().to_string(),
            state: "ok",
            version: "2022.3.22f1".into(),
            kind: "avatar",
            packages: packages
                .iter()
                .map(|name| crate::unity_projects::VpmPackage {
                    name: (*name).into(),
                    version: "1.0.0".into(),
                })
                .collect(),
            source: "alcom",
            hidden: false,
            running: "open",
            editor_available: true,
            avatars: Vec::new(),
            modified: None,
        }
    }

    #[test]
    fn only_addable_missing_vpm_packages_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &["jp.lilxyzw.liltoon", "com.vrchat.avatars"]);
        // vpm-manifest.json にあるもの、Packages/ に置いてあるもの、種類を決める SDK は並べない。
        assert!(!reportable_vpm(&p, "jp.lilxyzw.liltoon"));
        assert!(!reportable_vpm(&p, "com.vrchat.worlds"));
        assert!(!reportable_vpm(&p, "com.vrchat.avatars"));
        assert!(reportable_vpm(&p, "nadena.dev.modular-avatar"));
        std::fs::create_dir_all(
            dir.path()
                .join("Packages")
                .join("nadena.dev.modular-avatar"),
        )
        .unwrap();
        assert!(!reportable_vpm(&p, "nadena.dev.modular-avatar"));
        // フォルダ名として歩けない名前は Packages を見ない。
        assert!(reportable_vpm(&p, "../Assets"));
        assert!(reportable_vpm(&p, "com.vrchat.base"));
        // Unity の Package Manager で入れたもの（Packages/manifest.json）も、あるとする。
        assert!(reportable_vpm(&p, "net.ureishi.qvpen"));
        std::fs::write(
            dir.path().join("Packages").join("manifest.json"),
            "\u{feff}{\"dependencies\":{\"net.ureishi.qvpen\":\"https://example.invalid/qvpen.git\"}}",
        )
        .unwrap();
        assert!(!reportable_vpm(&p, "net.ureishi.qvpen"));
        assert!(reportable_vpm(&p, "com.vrchat.base"));
        assert!(!upm_manifest_lists("broken", "a.b"));
    }
}

#[cfg(test)]
mod settings_thread_tests {
    /// Attributes written right above `pub fn {name}(`.
    fn attributes<'a>(source: &'a str, name: &str) -> Vec<&'a str> {
        let at = source
            .find(&format!("pub fn {name}("))
            .unwrap_or_else(|| panic!("{name} exists"));
        source[..at]
            .lines()
            .rev()
            .take_while(|line| line.trim_start().starts_with("#["))
            .map(str::trim)
            .collect()
    }

    /// The screens read these while switching pages; a sync command would wait for the database
    /// lock on the UI thread (see `db`).
    #[test]
    fn read_commands_run_off_the_ui_thread() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        for name in [
            "library_stats",
            "list_avatars",
            "list_products_by_category",
            "list_products_by_genre",
            "list_product_options",
            "list_knowledge",
            "product_for_edit",
            "list_unlinked_files",
            "list_managed_products",
            "search_product_ids",
            "get_settings",
            "get_booth_page_interval",
            "integration_status",
            "extension_link",
            "classification_status",
            "classification_log_runs",
            "classification_log_entries",
            "classification_log_search",
            "classification_unresolved_names",
            "classification_feedback",
            "sync_export_json",
            "catalog_info",
            "setup_status",
            "product_avatar_suggestions",
            "product_genre_suggestion",
            "browser_auto_pending",
            "purchase_history",
            "purchase_status",
            "purchase_summary",
            "asset_update_status",
            "asset_update_history",
            "asset_update_downloads",
        ] {
            assert!(
                attributes(&source, name).contains(&"#[tauri::command(async)]"),
                "{name} runs off the UI thread"
            );
        }
    }

    #[test]
    fn settings_saves_run_off_the_ui_thread_but_change_the_tray_on_it() {
        let source = include_str!("commands.rs").replace("\r\n", "\n");
        for name in ["pub fn save_settings(", "pub fn update_settings("] {
            let at = source.find(name).unwrap();
            assert!(
                source[..at]
                    .trim_end()
                    .ends_with("#[tauri::command(async)]"),
                "{name} runs off the UI thread"
            );
            let body = &source[at..];
            let body = &body[..body.find("\n}\n").unwrap()];
            assert!(
                !body.contains("crate::presence::apply_presence("),
                "{name} changes the tray on the UI thread"
            );
            assert!(body.contains("apply_presence_on_ui("), "{name}");
        }
        let helper = &source[source.find("fn apply_presence_on_ui(").unwrap()..];
        assert!(helper[..helper.find("\n}\n").unwrap()].contains("run_on_main_thread"));
    }
}
