use crate::AppState;
use crate::db::{self, ExtractJob};
use crate::model::{
    AvatarInput, BaseBodyInput, CatalogInfo, CatalogRefresh, ClassificationSnapshot,
    RegisterOwnedBodyResult,
    ExtractReport, ImportReport, Knowledge, LibraryStats, ProductUpdate,
    ScanReport, Category, Settings, SettingsInput, SetupStatus, StagedLeftover,
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

#[tauri::command]
pub fn extension_link(state: State<'_, AppState>) -> Result<crate::model::ExtensionLink, String> {
    let connection = db(&state)?;
    let token = db::extension_token(&connection)?;
    drop(connection);
    Ok(extension_link_view(&state, &token))
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

#[tauri::command]
pub fn integration_status(state: State<'_, AppState>) -> Result<crate::integration::IntegrationStatus, String> {
    let connection = db(&state)?;
    crate::integration::status(&connection)
}

#[tauri::command]
pub fn set_booth_import_mode(app: AppHandle, state: State<'_, AppState>, mode: String) -> Result<(), String> {
    let mut connection = db(&state)?;
    crate::integration::set_mode(&mut connection, &mode)?;
    drop(connection);
    let _ = app.emit("library-changed", ());
    Ok(())
}

#[tauri::command]
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

#[tauri::command]
pub fn library_stats(state: State<'_, AppState>) -> Result<LibraryStats, String> {
    let mut connection = db(&state)?;
    let snapshot = db::load_snapshot(&mut connection)?;
    Ok(view::library_stats(&snapshot))
}

#[tauri::command]
pub fn list_avatars(state: State<'_, AppState>) -> Result<Vec<AvatarCard>, String> {
    let mut connection = db(&state)?;
    let snapshot = db::load_snapshot(&mut connection)?;
    Ok(view::avatar_cards(&snapshot))
}

#[tauri::command]
pub fn list_products_by_category(
    state: State<'_, AppState>,
    category: String,
) -> Result<Vec<ItemView>, String> {
    let category = Category::parse(&category).ok_or_else(|| "商品の種類が不正です".to_string())?;
    let mut connection = db(&state)?;
    let ids = db::product_ids_by_category(&connection, category)?;
    let mut snapshot = db::load_snapshot(&mut connection)?;
    let remote = db::settings(&mut connection)?.remote_library_folder;
    crate::library_files::fill_local_places(&mut snapshot, &remote);
    Ok(view::products_by_ids(&snapshot, &ids))
}

#[tauri::command]
pub fn list_products_by_genre(state: State<'_, AppState>, kind: String, genre: Option<String>) -> Result<GenreProducts, String> {
    let mut connection = db(&state)?;
    let genres = crate::genre::kind_genres(&kind).ok_or_else(|| "ジャンルの種類が不正です".to_string())?;
    let all_ids = db::product_ids_by_genre(&connection, &kind, None)?;
    let mut snapshot = db::load_snapshot(&mut connection)?;
    let remote = db::settings(&mut connection)?.remote_library_folder;
    crate::library_files::fill_local_places(&mut snapshot, &remote);
    Ok(view::genre_products(&snapshot, genres, &all_ids, genre.as_deref()))
}

fn load_avatar(app: &AppHandle, id: &str, probe_remote: bool) -> Result<AvatarDetail, String> {
    let state = app.state::<AppState>();
    let (mut snapshot, remote) = {
        let mut connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        let snapshot = db::load_snapshot(&mut connection)?;
        let remote = db::settings(&mut connection)?.remote_library_folder;
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
        let mut connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        let snapshot = db::load_snapshot(&mut connection)?;
        let remote = db::settings(&mut connection)?.remote_library_folder;
        (snapshot, remote)
    };
    let probes = crate::library_files::probe_remote_for_products(&mut snapshot, &remote, &ids);
    Ok(crate::view::remote_product_probes(&snapshot, &probes))
}

#[tauri::command]
pub fn get_booth_page_interval(state: State<'_, AppState>) -> Result<u64, String> {
    let connection = db(&state)?;
    db::booth_page_interval_ms(&connection)
}

#[tauri::command]
pub fn set_booth_page_interval(
    state: State<'_, AppState>,
    interval_ms: u64,
) -> Result<(), String> {
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
    Ok(crate::unity::status(&selected, &crate::unity::Sources::for_current_os(), os))
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
            crate::unity::validate_manual(&path, os)?.display().to_string()
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

#[tauri::command]
#[allow(non_snake_case)]
pub fn product_for_edit(
    state: State<'_, AppState>,
    boothId: String,
) -> Result<ProductEditView, String> {
    let connection = db(&state)?;
    let snapshot = db::load_snapshot(&connection)?;
    view::product_edit_view(&snapshot, &boothId).ok_or_else(|| "商品が見つかりません".to_string())
}

#[tauri::command]
pub fn list_unlinked_files(
    state: State<'_, AppState>,
    offset: u32,
    limit: u32,
) -> Result<UnlinkedFilesPage, String> {
    let connection = db(&state)?;
    let snapshot = db::load_snapshot(&connection)?;
    Ok(view::unlinked_files_page(&snapshot, offset, limit.clamp(1, 200)))
}

#[tauri::command]
pub fn classification_log_runs(
    state: State<'_, AppState>,
    offset: u32,
    limit: u32,
) -> Result<db::ClassificationRunPage, String> {
    let connection = db(&state)?;
    db::run_page(&connection, offset, limit.clamp(1, 200))
}

#[tauri::command]
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

#[tauri::command]
pub fn classification_log_search(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> Result<Vec<db::ClassificationEntryView>, String> {
    let connection = db(&state)?;
    db::search_entries(&connection, &query, limit.clamp(1, 200))
}

#[tauri::command]
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

#[tauri::command]
pub fn list_product_options(state: State<'_, AppState>) -> Result<Vec<ProductOption>, String> {
    let mut connection = db(&state)?;
    let snapshot = db::load_snapshot(&mut connection)?;
    Ok(view::product_options(&snapshot))
}

#[tauri::command]
pub fn list_knowledge(state: State<'_, AppState>) -> Result<Knowledge, String> {
    let mut connection = db(&state)?;
    db::knowledge(&mut connection)
}

#[tauri::command]
pub fn remote_sync_status(app: AppHandle) -> Result<crate::remote_sync::Status, String> {
    crate::remote_sync::status(&app)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let mut connection = db(&state)?;
    db::settings(&mut connection)
}

#[tauri::command]
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
    crate::presence::apply_presence(&app, saved.background_mode, saved.launch_at_login)?;
    Ok(())
}

#[tauri::command]
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
        if let Err(error) =
            crate::presence::apply_presence(&app, saved.background_mode, saved.launch_at_login)
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
            crate::presence::remember_background(&app, before.background_mode);
            let _ = crate::presence::apply_presence(
                &app,
                before.background_mode,
                before.launch_at_login,
            );
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
fn add_folder_labels(app: &AppHandle, listing: &mut crate::file_browser::DirListing) -> Result<(), String> {
    let state = app.state::<AppState>();
    let connection = db(&state)?;
    let has_files = listing
        .entries
        .iter()
        .any(|entry| entry.kind == crate::file_browser::EntryKind::File);
    let linked = if has_files { db::linked_file_products(&connection)? } else { Vec::new() };
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
pub async fn list_zip_entries(app: AppHandle, path: String) -> Result<crate::file_browser::ZipListing, String> {
    let (library, remote) = library_roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || crate::file_browser::list_zip(&path, &library, &remote))
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

/// 「Unityで開く」: zip か .unitypackage の中の Unity パッケージを一覧する。展開しない。
#[tauri::command]
pub async fn unity_packages(
    app: AppHandle,
    path: String,
) -> Result<crate::unity_import::UnityPackageList, String> {
    let (library, remote) = library_roots(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let source = crate::unity_import::allowed_source(&path, &library, &remote)?;
        crate::unity_import::list_packages(&source)
    })
    .await
    .map_err(|err| format!("Unity パッケージを読めません: {err}"))?
}

/// 選んだ Unity パッケージを、アプリのキャッシュ `unity-import/<商品ID>/` に取り出す。
/// 単体の .unitypackage は取り出さず、そのまま返す。メイン画面からだけ受け付ける。
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
    tauri::async_runtime::spawn_blocking(move || {
        let source = crate::unity_import::allowed_source(&path, &library, &remote)?;
        let list = crate::unity_import::list_packages(&source)?;
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

/// 取り出した Unity パッケージを先頭から順に開く。`mode` は "unity"（設定の既定 Unity で
/// `-openfile`）か "system"（OS の関連付け）。Unity の起動を待つときは途中で止まり、残りは呼び直す。
#[tauri::command]
pub async fn launch_unity_packages(
    view: tauri::Webview,
    app: AppHandle,
    paths: Vec<String>,
    mode: String,
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
        match mode {
            crate::unity_import::LaunchMode::Unity => {
                let editor = crate::unity_import::editor_from_setting(&selected)?;
                crate::unity_import::launch_with_editor(
                    &editor,
                    &packages,
                    crate::unity_import::HANDOFF_TIMEOUT,
                    crate::unity_import::HANDOFF_GAP,
                )
            }
            crate::unity_import::LaunchMode::System => {
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
        }
    })
    .await
    .map_err(|err| format!("Unity パッケージを開けません: {err}"))?
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

fn finish_copy(progress: &ProgressGuard, filename: &str, result: &Result<(), String>) {
    if result.is_ok() {
        progress.done(format!("{filename} をコピーしました"));
    } else {
        progress.fail("コピーを中断しました");
    }
}

#[tauri::command]
pub async fn pull_remote_file(
    app: AppHandle,
    booth_id: String,
    variation: String,
    filename: String,
) -> Result<(), String> {
    let label = filename.clone();
    let progress = ProgressGuard::new(app.clone(), "copy");
    progress.update(0, 1, crate::progress::counted_message("コピー中", 1, 1, &label));
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
        let dest = crate::library_files::pull_file(
            Path::new(remote.trim()),
            Path::new(library.trim()),
            &booth_id,
            &filename,
        )?;
        let state = worker.state::<AppState>();
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        db::note_downloaded_file(&connection, &booth_id, &variation, &filename, &dest)
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
) -> Result<(), String> {
    let label = filename.clone();
    let progress = ProgressGuard::new(app.clone(), "copy");
    progress.update(0, 1, crate::progress::counted_message("コピー中", 1, 1, &label));
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
        crate::library_files::push_file(&source, Path::new(remote.trim()), &booth_id, &filename)?;
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
        crate::remote_sync::remember(&mut connection, &remote, &booth_id, &filename)
    })
    .await
    .map_err(|err| format!("コピーを実行できません: {err}"))?;
    finish_copy(&progress, &label, &copied);
    copied
}

#[tauri::command]
pub async fn open_booth_browser(app: tauri::AppHandle) -> Result<(), String> {
    crate::browser::dispatch_ui(app, |handle| crate::browser::open_browser(handle)).await
}

#[tauri::command]
pub async fn show_booth_library(app: tauri::AppHandle) -> Result<(), String> {
    crate::browser::dispatch_ui(app, |handle| crate::browser::show_library(handle)).await
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
    let report = crate::browser::import_library(app.clone()).await?;
    let _ = app.emit("library-changed", ());
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

#[tauri::command]
pub fn purchase_history(
    state: State<'_, AppState>,
    query: crate::purchases::PurchaseQuery,
) -> Result<crate::purchases::PurchasePage, String> {
    let connection = db(&state)?;
    crate::purchases::page(&connection, &query, crate::browser::purchase_walk_running())
}

#[tauri::command]
pub fn purchase_status(state: State<'_, AppState>) -> Result<crate::purchases::PurchaseStatus, String> {
    let connection = db(&state)?;
    crate::purchases::status(&connection, crate::browser::purchase_walk_running())
}

#[tauri::command]
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

#[tauri::command]
pub fn create_purchase_label(
    state: State<'_, AppState>,
    name: String,
) -> Result<crate::purchases::PurchaseLabel, String> {
    let connection = db(&state)?;
    crate::purchases::create_label(&connection, &name)
}

#[tauri::command]
pub fn rename_purchase_label(state: State<'_, AppState>, id: i64, name: String) -> Result<(), String> {
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

#[tauri::command]
pub async fn close_booth_browser(app: tauri::AppHandle) -> Result<(), String> {
    crate::browser::dispatch_ui(app, |handle| crate::browser::close_browser(handle)).await
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeFilesReport {
    pub files: usize,
}

/// 「ファイルを取得」: one cookie-less public read for one product the user clicked.
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
    Ok(FreeFilesReport { files: count })
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

#[tauri::command]
pub fn classification_feedback(state: State<'_, AppState>, scope: String) -> Result<String, String> {
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
#[tauri::command]
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
    let mut connection = db(&state)?;
    db::link_file(&mut connection, &path, productId.as_deref())
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
    let mut connection = db(&state)?;
    db::save_base_body(&mut connection, &body)
}

#[tauri::command]
pub fn delete_base_body(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::delete_base_body(&mut connection, &id)
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
    progress.done_with(
        report.indexed,
        format!(
            "{} 件を調べ、{} 件を商品と紐づけました",
            report.indexed, report.linked
        ),
    );
    Ok(report)
}

#[tauri::command]
pub fn classification_status(
    state: State<'_, crate::AppState>,
) -> Result<crate::model::ClassificationStatus, String> {
    let connection = db(&state)?;
    db::classification_status(&connection)
}

#[tauri::command]
pub async fn extract_products(
    app: AppHandle,
    state: State<'_, AppState>,
    target: String,
) -> Result<ExtractReport, String> {
    let running = Arc::clone(&state.extract_running);
    if running.swap(true, Ordering::SeqCst) {
        return Err("分類はすでに実行中です".into());
    }
    state.cancel_extract.store(false, Ordering::SeqCst);
    let _clear = ClearRunning(running);
    let progress = ProgressGuard::new(app.clone(), "extract");
    let (jobs, threshold) = {
        let mut connection = db(&state)?;
        let jobs = db::products_for_extract(&mut connection, db::parse_extract_target(&target)?)?;
        (jobs, db::settings(&connection)?.confidence_threshold)
    };
    // The log's run. Each product locks the database and logs on its own, so an edit made
    // between two products is not attributed to this run.
    let mut run = db::ClassificationRun::new(db::KIND_RECLASSIFY, db::CLASSIFIER_RULES)
        .with_note(&target);
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
            &app,
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
        &app,
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
            format!("{processed} 件分類（カテゴリとタグ {booth_settled}、タイトルと説明 {rule_filled}）"),
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

#[tauri::command]
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

#[tauri::command]
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
        let mut connection = db(&state)?;
        db::thumbnail_jobs(&mut connection)?
    };
    let progress = ProgressGuard::new(app.clone(), "thumbs");
    let thumbs = state.thumbs_dir.clone();
    let client = crate::thumb::download_client()?;
    let total = jobs.len() as u32;
    let mut saved = 0u32;
    for (index, job) in jobs.iter().enumerate() {
        if let Some(path) = &job.thumbnail_path {
            if Path::new(path).is_file() {
                continue;
            }
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
            let mut connection = db(&state)?;
            db::set_thumbnail_path(&mut connection, &job.booth_id, &path)?;
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
    if let Some(path) = crate::preview_image::cached_preview(&directory, &boothId, original.as_str()) {
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
    if let Some(path) = crate::preview_image::cached_preview(&directory, &boothId, original.as_str()) {
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
    SelectTab { tab_id: String },
    CloseTab { tab_id: String },
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
    PanelArea { area: Option<PanelRect>, visible: bool },
    ReorderTabs { host_id: String, tab_ids: Vec<String> },
    DetachTab { tab_id: String, x: f64, y: f64 },
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
        crate::browser_surface::SurfaceArea::from_panel(rect.left, rect.top, rect.width, rect.height)
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
            BrowserAction::Navigate { url, tab_id, host_id } => {
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
            BrowserAction::Zoom { action, tab_id, host_id } => {
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
            }
            BrowserAction::ReorderTabs { host_id, tab_ids } => {
                surface::reorder(handle, &host_id, &tab_ids)?;
            }
            BrowserAction::DetachTab { tab_id, x, y } => surface::detach_tab(handle, &tab_id, x, y)?,
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
        {
            let state = handle.state::<AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::save_browser_bookmark(&connection, originalUrl.as_deref(), &title, &url)?;
        }
        let _ = handle.emit("browser-state-changed", ());
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
        {
            let state = handle.state::<AppState>();
            let connection = state
                .db
                .lock()
                .map_err(|_| "データベースをロックできません".to_string())?;
            db::delete_browser_bookmark(&connection, &url)?;
        }
        let _ = handle.emit("browser-state-changed", ());
        crate::browser_surface::state(handle)
    })
    .await
}

#[cfg(test)]
mod browser_action_tests {
    use super::BrowserAction;

    #[test]
    fn older_browser_actions_parse_and_new_fields_stay_optional() {
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"newTab"}"#).unwrap();
        assert!(matches!(action, BrowserAction::NewTab { url: None, host_id: None }));
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"back"}"#).unwrap();
        assert!(matches!(action, BrowserAction::Back { tab_id: None, host_id: None }));
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"undock"}"#).unwrap();
        assert!(matches!(action, BrowserAction::Undock));
        let action: BrowserAction = serde_json::from_str(r#"{"kind":"dock","area":null}"#).unwrap();
        assert!(matches!(action, BrowserAction::Dock { area: None, host_id: None }));
        let action: BrowserAction =
            serde_json::from_str(r#"{"kind":"detachTab","tabId":"booth-tab-1","x":-10.0,"y":4.0}"#).unwrap();
        assert!(matches!(
            action,
            BrowserAction::DetachTab { ref tab_id, x, y }
                if tab_id == "booth-tab-1" && x == -10.0 && y == 4.0
        ));
        assert!(serde_json::from_str::<BrowserAction>(
            r#"{"kind":"navigate","url":"https://example.com","extra":1}"#
        )
        .is_err());
    }
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn product_avatar_suggestions(state: State<'_, AppState>, boothId: String) -> Result<db::AvatarSuggestions, String> {
    let connection = db(&state)?;
    db::product_avatar_suggestions(&connection, &boothId)
}

#[tauri::command]
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

#[tauri::command]
pub fn asset_update_status(view: tauri::Webview, state: State<'_, AppState>) -> Result<crate::asset_updates::Status, String> {
    if view.label() != "main" {
        return Err("この画面からアセットの更新を確認する権限がありません".into());
    }
    let connection = db(&state)?;
    crate::asset_updates::status(&connection)
}

#[tauri::command]
pub fn asset_update_history(view: tauri::Webview, state: State<'_, AppState>) -> Result<Vec<crate::asset_updates::UpdateRecord>, String> {
    trusted_updater(&view)?;
    let connection = db(&state)?;
    crate::asset_updates::history(&connection)
}

#[tauri::command]
pub fn configure_asset_updates(view: tauri::Webview, state: State<'_, AppState>, input: crate::asset_updates::Configure) -> Result<crate::asset_updates::Status, String> {
    if view.label() != "main" {
        return Err("この画面からアセットの更新を確認する権限がありません".into());
    }
    let mut connection = db(&state)?;
    crate::asset_updates::configure(&mut connection, input)
}

#[tauri::command]
pub async fn check_asset_updates(view: tauri::Webview, app: AppHandle) -> Result<crate::asset_updates::Status, String> {
    if view.label() != "main" {
        return Err("この画面からアセットの更新を確認する権限がありません".into());
    }
    crate::asset_updates::check(&app, true).await
}

#[tauri::command]
pub async fn preview_existing_import(view: tauri::Webview, app: AppHandle, path: String) -> Result<crate::existing_import::Preview, String> {
    trusted_updater(&view)?;
    let rows={let state=app.state::<AppState>(); let connection=db(&state)?; crate::existing_import::catalog(&connection)?};
    tauri::async_runtime::spawn_blocking(move || crate::existing_import::preview(rows, Path::new(&path))).await.map_err(|_|"フォルダを調べられません")?
}
#[tauri::command]
pub async fn register_existing_import(view: tauri::Webview, app: AppHandle, path: String, selections: Vec<crate::existing_import::Selection>, copy: bool) -> Result<crate::existing_import::Report, String> {
    trusted_updater(&view)?;
    tauri::async_runtime::spawn_blocking(move || crate::existing_import::register(&app,Path::new(&path),selections,copy)).await.map_err(|_|"取り込みを実行できません")?
}
