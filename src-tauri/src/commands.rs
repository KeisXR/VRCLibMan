use crate::AppState;
use crate::db::{self, ExtractJob};
use crate::model::{
    AvatarCandidate, AvatarInput, BaseBodyInput, CatalogInfo, CatalogRefresh, ExtractReport,
    ImportReport, JobProgress, Knowledge, LibraryStats, OllamaStatus, ProductUpdate, ScanReport,
    Category, Settings, SettingsInput, SetupStatus, StagedLeftover,
};
use crate::apple_fm::{self, AppleError};
use crate::model::Extraction;
use crate::ollama::{self, model_ready};
use crate::scan;
use crate::view::{self, AvatarCard, AvatarDetail, GenreProducts, ItemView, ProductOption, QueuePage};
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
) -> Result<crate::updater::Status, String> {
    trusted_updater(&view)?;
    crate::updater::check(&app, &state).await
}

#[tauri::command]
pub async fn download_app_update(
    view: tauri::Webview,
    state: State<'_, crate::updater::UpdateState>,
) -> Result<crate::updater::Status, String> {
    trusted_updater(&view)?;
    crate::updater::download(&state).await
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

fn emit_progress(app: &AppHandle, job: &str, current: u32, total: u32, message: impl Into<String>) {
    let _ = app.emit(
        "job-progress",
        JobProgress {
            job: job.to_string(),
            current,
            total,
            message: message.into(),
        },
    );
}

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
    let settings = db::settings(&mut connection)?;
    Ok(view::library_stats(&snapshot, &settings.ollama_model))
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
    let all_ids = db::product_ids_by_genre(&connection, &kind, None)?;
    let ids = db::product_ids_by_genre(&connection, &kind, genre.as_deref())?;
    let mut snapshot = db::load_snapshot(&mut connection)?;
    let remote = db::settings(&mut connection)?.remote_library_folder;
    crate::library_files::fill_local_places(&mut snapshot, &remote);
    let mut counts = std::collections::HashMap::new();
    for product in snapshot.products.iter().filter(|product| all_ids.contains(&product.booth_id)) {
        if let Some(value) = &product.genre { *counts.entry(value.clone()).or_insert(0) += 1; }
    }
    Ok(GenreProducts { items: view::products_by_ids(&snapshot, &ids), counts })
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
    view::avatar_detail(&snapshot, id).ok_or_else(|| "アバターが見つかりません".into())
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

#[tauri::command]
pub fn list_queue(
    state: State<'_, AppState>,
    kind: String,
    offset: u32,
    limit: u32,
) -> Result<QueuePage, String> {
    if !matches!(kind.as_str(), "unextracted" | "review" | "files") {
        return Err("キューの種類が不正です".into());
    }
    let mut connection = db(&state)?;
    let snapshot = db::load_snapshot(&mut connection)?;
    Ok(view::queue_page(
        &snapshot,
        &kind,
        offset,
        limit.clamp(1, 200),
    ))
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
    let base = ollama::validate_base_url(&settings.ollama_base_url)?;
    let model = ollama::validate_model_name(&settings.ollama_model)?;
    let saved = {
        let connection = db(&state)?;
        db::save_settings(
            &connection,
            &SettingsInput {
                ollama_base_url: base,
                ollama_model: model,
                confidence_threshold: settings.confidence_threshold,
                library_folder: settings.library_folder,
                builtin_browser: settings.builtin_browser,
                remote_library_folder: settings.remote_library_folder,
                remote_auto_sync: settings.remote_auto_sync,
                route_downloads: settings.route_downloads,
                llm_backend: settings.llm_backend,
                background_mode: settings.background_mode,
                launch_at_login: settings.launch_at_login,
            },
        )?;
        db::settings(&connection)?
    };
    crate::remote_sync::wake(&app);
    crate::presence::remember_background(&app, saved.background_mode);
    crate::presence::apply_presence(&app, saved.background_mode, saved.launch_at_login)
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

fn finish_copy(app: &AppHandle, filename: &str, result: &Result<(), String>) {
    let message = if result.is_ok() {
        format!("{filename} をコピーしました")
    } else {
        "コピーを中断しました".into()
    };
    emit_progress(app, "copy", 1, 1, message);
}

#[tauri::command]
pub async fn pull_remote_file(
    app: AppHandle,
    booth_id: String,
    variation: String,
    filename: String,
) -> Result<(), String> {
    let label = filename.clone();
    emit_progress(&app, "copy", 0, 1, format!("{label} をコピーしています"));
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
    finish_copy(&app, &label, &copied);
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
    emit_progress(&app, "copy", 0, 1, format!("{label} をコピーしています"));
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
        let connection = state
            .db
            .lock()
            .map_err(|_| "データベースをロックできません".to_string())?;
        db::remember_file_size(&connection, &booth_id, &variation, &filename, size)
    })
    .await
    .map_err(|err| format!("コピーを実行できません: {err}"))?;
    finish_copy(&app, &label, &copied);
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
pub async fn import_booth_page(app: tauri::AppHandle) -> Result<ImportReport, String> {
    // ページ読み取りの待ちを UI スレッドに載せない。
    let handle = app.clone();
    let report = tauri::async_runtime::spawn_blocking(move || crate::browser::import_current_page(&handle))
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
) -> Result<(), String> {
    crate::browser::download_one_file(app, booth_id, variation, filename).await
}

#[tauri::command]
pub fn update_product(state: State<'_, AppState>, update: ProductUpdate) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::update_product(&mut connection, &update)
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
    db::delete_avatar(&mut connection, &id)
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
    db::merge_avatars(&mut connection, &fromId, &intoId)
}

#[tauri::command]
pub fn restore_seed(state: State<'_, AppState>) -> Result<u32, String> {
    let mut connection = db(&state)?;
    db::restore_seed(&mut connection)
}

#[tauri::command]
pub fn reresolve(state: State<'_, AppState>) -> Result<u32, String> {
    let mut connection = db(&state)?;
    db::reresolve_all(&mut connection)
}

#[tauri::command]
pub async fn scan_library(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ScanReport, String> {
    let root = path.clone();
    emit_progress(&app, "scan", 0, 0, "フォルダを調べています");
    let outcome = tauri::async_runtime::spawn_blocking(move || scan::scan_folder(Path::new(&root)))
        .await
        .map_err(|err| format!("スキャンを実行できません: {err}"))??;
    let mut connection = db(&state)?;
    let report = db::apply_library_scan(&mut connection, &path, &outcome)?;
    emit_progress(
        &app,
        "scan",
        report.indexed,
        report.indexed,
        format!(
            "{} 件を調べ、{} 件を商品と紐づけました",
            report.indexed, report.linked
        ),
    );
    Ok(report)
}

#[tauri::command]
pub async fn extract_products(
    app: AppHandle,
    state: State<'_, AppState>,
    force: bool,
) -> Result<ExtractReport, String> {
    let running = Arc::clone(&state.extract_running);
    if running.swap(true, Ordering::SeqCst) {
        return Err("分類はすでに実行中です".into());
    }
    state.cancel_extract.store(false, Ordering::SeqCst);
    let _clear = ClearRunning(running);
    let jobs: Vec<ExtractJob> = {
        let mut connection = db(&state)?;
        db::products_for_extract(&mut connection, force)?
    };
    let settings = {
        let mut connection = db(&state)?;
        db::settings(&mut connection)?
    };
    let client = ollama::extraction_client()?;
    let keep = {
        let connection = db(&state)?;
        db::protected_names(&connection)?
    };
    let lists = wordlist_dir(&app);
    let ollama_ready = if settings.llm_backend == "apple" {
        ollama::list_models(&client, &settings.ollama_base_url)
            .await
            .map(|models| model_ready(&models, &settings.ollama_model))
            .unwrap_or(false)
    } else {
        false
    };
    let total = jobs.len() as u32;
    let mut processed = 0;
    let mut failed = 0;
    let mut errors = Vec::new();
    let mut cancelled = false;
    for (index, job) in jobs.iter().enumerate() {
        if state.cancel_extract.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        {
            let connection = db(&state)?;
            if db::product_excluded(&connection, &job.booth_id)? {
                continue;
            }
        }
        let label: String = job.title.chars().take(40).collect();
        emit_progress(
            &app,
            "extract",
            index as u32,
            total,
            format!("分類中: {label}"),
        );
        if job.description == crate::model::UNAVAILABLE_DESCRIPTION {
            let mut connection = db(&state)?;
            db::mark_description_seen(&mut connection, &job.booth_id)?;
            processed += 1;
            continue;
        }
        let settled = {
            let mut connection = db(&state)?;
            db::skip_model(&mut connection, &job.booth_id)?
        };
        if settled {
            processed += 1;
            continue;
        }
        let classified = classify_product(&client, &settings, job, &keep, &lists, ollama_ready).await;
        {
            let connection = db(&state)?;
            if db::product_excluded(&connection, &job.booth_id)? {
                continue;
            }
        }
        match classified {
            Ok(Classified::Done(extraction)) => {
                if state.cancel_extract.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                let mut connection = db(&state)?;
                db::apply_extraction(
                    &mut connection,
                    &job.booth_id,
                    &extraction,
                    settings.confidence_threshold,
                )?;
                processed += 1;
            }
            Ok(Classified::Review) => {
                let connection = db(&state)?;
                db::mark_guardrail_review(&connection, &job.booth_id)?;
                processed += 1;
            }
            Err(err) => {
                failed += 1;
                if errors.len() < 8 {
                    errors.push(format!("{label}: {err}"));
                }
            }
        }
    }
    if processed > 0 {
        emit_progress(&app, "extract", total, total, "分類結果を整理しています");
        let mut connection = db(&state)?;
        db::reresolve_all(&mut connection)?;
    }
    emit_progress(
        &app,
        "extract",
        total,
        total,
        if cancelled {
            "分類を中断しました".into()
        } else {
            format!("{processed} 件分類、{failed} 件失敗")
        },
    );
    Ok(ExtractReport {
        processed,
        failed,
        cancelled,
        errors,
    })
}

enum Classified {
    Done(Extraction),
    Review,
}

fn wordlist_dir(app: &AppHandle) -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("VRCLIBMAN_WORDLIST_DIR") {
        return std::path::PathBuf::from(dir);
    }
    app.path()
        .resource_dir()
        .map(|dir| dir.join("wordlists"))
        .unwrap_or_else(|_| crate::mask::wordlist_dir_from_env_or_manifest())
}

async fn classify_product(
    client: &reqwest::Client,
    settings: &Settings,
    job: &ExtractJob,
    keep: &[String],
    lists: &std::path::Path,
    ollama_ready: bool,
) -> Result<Classified, String> {
    if settings.llm_backend != "apple" {
        let extraction = ollama::extract_product(
            client,
            &settings.ollama_base_url,
            &settings.ollama_model,
            &job.title,
            &job.description,
        )
        .await?;
        return Ok(Classified::Done(extraction));
    }
    let first = run_apple(
        ollama::SYSTEM_PROMPT,
        &job.title,
        &job.description,
    )
    .await?;
    match first {
        Ok(extraction) => Ok(Classified::Done(extraction)),
        Err(AppleError::Guardrail) => {
            let masked = match (
                crate::mask::mask_text(&job.title, keep, lists),
                crate::mask::mask_text(&job.description, keep, lists),
            ) {
                (Ok(title), Ok(description)) => Some((title, description)),
                (Err(err), _) | (_, Err(err)) => {
                    eprintln!(
                        "VRCLibMan: 語リストを読めないので、伏せて読み直さず次の手段に進みます: {err}"
                    );
                    None
                }
            };
            if let Some((title, description)) = masked {
                match run_apple(ollama::SYSTEM_PROMPT, &title, &description).await? {
                    Ok(extraction) => return Ok(Classified::Done(extraction)),
                    Err(_) => {}
                }
            }
            if ollama_ready {
                let extraction = ollama::extract_product(
                    client,
                    &settings.ollama_base_url,
                    &settings.ollama_model,
                    &job.title,
                    &job.description,
                )
                .await?;
                Ok(Classified::Done(extraction))
            } else {
                Ok(Classified::Review)
            }
        }
        Err(AppleError::Message(message)) => Err(message),
    }
}

async fn run_apple(
    instructions: &str,
    title: &str,
    description: &str,
) -> Result<Result<Extraction, AppleError>, String> {
    let instructions = instructions.to_string();
    let title = title.to_string();
    let description = description.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        apple_fm::extract_warm(
            &instructions,
            &title,
            &description,
            std::time::Duration::from_secs(60),
        )
    })
    .await
    .map_err(|_| "Apple のオンデバイスモデルを実行できません".to_string())
}

#[tauri::command]
pub fn apple_fm_status() -> crate::apple_fm::AppleStatus {
    apple_fm::status(None)
}

#[tauri::command]
pub fn cancel_extract(state: State<'_, AppState>) {
    state.cancel_extract.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub async fn test_ollama(state: State<'_, AppState>) -> Result<OllamaStatus, String> {
    let settings = {
        let mut connection = db(&state)?;
        db::settings(&mut connection)?
    };
    let client = ollama::probe_client()?;
    let installed = ollama::list_models(&client, &settings.ollama_base_url).await?;
    let ready = model_ready(&installed, &settings.ollama_model);
    let model_size_bytes = ollama::installed_size(&installed, &settings.ollama_model);
    let mut models: Vec<String> = installed.into_iter().map(|model| model.name).collect();
    models.sort();
    Ok(OllamaStatus {
        models,
        model_ready: ready,
        model_size_bytes,
    })
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
    emit_progress(&app, "catalog", 0, 0, "プロファイル一覧を取得しています");
    let client = ollama::catalog_client()?;
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
    emit_progress(
        &app,
        "catalog",
        report.total,
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
pub async fn pull_ollama_model(app: AppHandle, state: State<'_, AppState>, model: String) -> Result<(), String> {
    let model = ollama::validate_model_name(&model)?;
    let settings = {
        let connection = db(&state)?;
        db::settings(&connection)?
    };
    let client = ollama::pull_client()?;
    let app_progress = app.clone();
    ollama::pull_model(
        &client,
        &settings.ollama_base_url,
        &model,
        |completed, total, status| {
            let status = ollama::model_download_status(status);
            let current = (completed / 1_000_000) as u32;
            let whole = (total / 1_000_000) as u32;
            let message = if total > 0 {
                format!("{status}（{current} / {whole} MB）")
            } else {
                status.to_string()
            };
            emit_progress(&app_progress, "pull", current, whole, message);
        },
    )
    .await?;
    emit_progress(&app, "pull", 1, 1, "モデルを取得しました");
    Ok(())
}

#[tauri::command]
pub fn avatar_candidates(state: State<'_, AppState>) -> Result<Vec<AvatarCandidate>, String> {
    let connection = db(&state)?;
    db::avatar_candidates(&connection)
}

#[tauri::command]
pub fn register_avatar_candidate(
    state: State<'_, AppState>,
    name: String,
) -> Result<String, String> {
    let mut connection = db(&state)?;
    db::register_candidate(&mut connection, &name)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn mark_owned_avatar(state: State<'_, AppState>, boothId: String) -> Result<String, String> {
    let mut connection = db(&state)?;
    db::mark_owned_body(&mut connection, &boothId)
}

#[tauri::command]
#[allow(non_snake_case)]
pub fn alias_avatar_candidate(
    state: State<'_, AppState>,
    name: String,
    avatarId: String,
) -> Result<(), String> {
    let mut connection = db(&state)?;
    db::alias_candidate(&mut connection, &name, &avatarId)
}

#[tauri::command]
pub async fn cache_thumbnails(app: AppHandle, state: State<'_, AppState>) -> Result<u32, String> {
    let jobs = {
        let mut connection = db(&state)?;
        db::thumbnail_jobs(&mut connection)?
    };
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
        emit_progress(
            &app,
            "thumbs",
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
    emit_progress(
        &app,
        "thumbs",
        total,
        total,
        format!("{saved} 件のサムネイルを保存しました"),
    );
    Ok(saved)
}

fn trusted_browser_controls(view: &tauri::Webview) -> Result<(), String> {
    if matches!(view.label(), "main" | crate::browser_tabs::TOOLBAR_LABEL) {
        Ok(())
    } else {
        Err("この画面からブラウザを操作する権限がありません".into())
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
    NewTab { url: Option<String> },
    Navigate { url: String },
    SelectTab { tab_id: String },
    CloseTab { tab_id: String },
    Back,
    Forward,
    Reload,
    Stop,
    ToolbarHeight { height: f64 },
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
pub async fn browser_action(
    app: AppHandle,
    webview: tauri::Webview,
    action: BrowserAction,
) -> Result<crate::browser_tabs::BrowserState, String> {
    trusted_browser_controls(&webview)?;
    crate::browser::dispatch_ui(app, move |handle| {
        use crate::browser_surface as surface;
        match action {
            BrowserAction::NewTab { url } => surface::new_tab(handle, url.as_deref())?,
            BrowserAction::Navigate { url } => surface::navigate(handle, &url)?,
            BrowserAction::SelectTab { tab_id } => surface::select(handle, &tab_id)?,
            BrowserAction::CloseTab { tab_id } => surface::close_tab(handle, &tab_id)?,
            BrowserAction::Back => surface::history(handle, false)?,
            BrowserAction::Forward => surface::history(handle, true)?,
            BrowserAction::Reload => surface::reload(handle)?,
            BrowserAction::Stop => surface::stop(handle)?,
            BrowserAction::ToolbarHeight { height } => surface::toolbar_height(handle, height)?,
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
