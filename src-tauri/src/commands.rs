use crate::AppState;
use crate::db::{self, ExtractJob};
use crate::model::{
    AvatarCandidate, AvatarInput, BaseBodyInput, CatalogInfo, CatalogRefresh, ExtractReport,
    ImportReport, JobProgress, Knowledge, LibraryStats, OllamaStatus, ProductUpdate, ScanReport,
    Settings, SettingsInput, SetupStatus,
};
use crate::ollama::{self, model_ready};
use crate::scan;
use crate::view::{self, AvatarCard, AvatarDetail, ProductOption, QueuePage};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, MutexGuard};
use tauri::{AppHandle, Emitter, Manager, State};

struct ClearRunning(Arc<std::sync::atomic::AtomicBool>);

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
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let mut connection = db(&state)?;
    db::settings(&mut connection)
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: SettingsInput) -> Result<(), String> {
    let base = ollama::validate_base_url(&settings.ollama_base_url)?;
    let model = ollama::validate_model_name(&settings.ollama_model)?;
    let mut connection = db(&state)?;
    db::save_settings(
        &mut connection,
        &SettingsInput {
            ollama_base_url: base,
            ollama_model: model,
            confidence_threshold: settings.confidence_threshold,
            library_folder: settings.library_folder,
            builtin_browser: settings.builtin_browser,
            remote_library_folder: settings.remote_library_folder,
            route_downloads: settings.route_downloads,
        },
    )
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
    tauri::async_runtime::spawn_blocking(move || crate::browser::import_current_page(&app))
        .await
        .map_err(|err| err.to_string())?
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
    let report = db::apply_scan(&mut connection, &path, &outcome)?;
    db::set_library_folder(&mut connection, &path)?;
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
        return Err("抽出はすでに実行中です".into());
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
        let label: String = job.title.chars().take(40).collect();
        emit_progress(
            &app,
            "extract",
            index as u32,
            total,
            format!("抽出中: {label}"),
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
        match ollama::extract_product(
            &client,
            &settings.ollama_base_url,
            &settings.ollama_model,
            &job.title,
            &job.description,
        )
        .await
        {
            Ok(extraction) => {
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
            Err(err) => {
                failed += 1;
                if errors.len() < 8 {
                    errors.push(format!("{label}: {err}"));
                }
            }
        }
    }
    if processed > 0 {
        emit_progress(&app, "extract", total, total, "結び付きを再計算しています");
        let mut connection = db(&state)?;
        db::reresolve_all(&mut connection)?;
    }
    emit_progress(
        &app,
        "extract",
        total,
        total,
        if cancelled {
            "抽出を中断しました".into()
        } else {
            format!("{processed} 件抽出、{failed} 件失敗")
        },
    );
    Ok(ExtractReport {
        processed,
        failed,
        cancelled,
        errors,
    })
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
pub async fn pull_ollama_model(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let settings = {
        let connection = db(&state)?;
        db::settings(&connection)?
    };
    let client = ollama::pull_client()?;
    let app_progress = app.clone();
    ollama::pull_model(
        &client,
        &settings.ollama_base_url,
        &settings.ollama_model,
        |completed, total, status| {
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
