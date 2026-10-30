mod apple_fm;
mod booth_category;
mod booth_detail;
mod browser;
mod browser_url;
#[cfg(target_os = "linux")]
mod browser_linux;
mod browser_tabs;
mod browser_surface;
mod mask;
mod catalog;
mod classifier;
mod cloud;
mod commands;
mod compat;
mod genre;
mod db;
mod import_data;
mod integration;
mod library_files;
mod remote_sync;
mod library_walk;
mod local_import;
mod matching;
mod model;
mod normalize;
mod ollama;
mod presence;
mod preview_image;
mod quiet;
mod resolve;
mod scan;
mod thumb;
mod view;
mod updater;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::Manager;

// Reuse the manifest that tauri-build generates for the app. Without it the
// Windows test loader cannot resolve Common Controls v6's TaskDialogIndirect.
#[cfg(all(test, windows, target_env = "msvc"))]
#[link(name = "resource")]
unsafe extern "C" {}

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub cancel_extract: AtomicBool,
    pub extract_running: Arc<AtomicBool>,
    pub thumbs_dir: std::path::PathBuf,
    pub previews_dir: std::path::PathBuf,
    pub receiver_error: Mutex<Option<String>>,
    pub import_guard: Mutex<local_import::ImportGuard>,
    pub background_mode: AtomicBool,
    pub staged_left: Mutex<Vec<model::StagedLeftover>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            presence::show_main_later(app);
        }));
    }
    builder
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([presence::AUTOSTART_ARG])
                .build(),
        )
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .on_window_event(|window, event| {
            presence::handle_main_close(window, event);
        })
        .setup(|app| {
            use tauri_plugin_deep_link::DeepLinkExt;
            updater::initialize(app.handle());
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let thumbs = directory.join("thumbs");
            let previews = directory.join("previews");
            std::fs::create_dir_all(&thumbs)?;
            std::fs::create_dir_all(&previews)?;
            let connection = db::open(&directory.join("library.sqlite"))
                .map_err(|err| std::io::Error::other(err))?;
            app.manage(AppState {
                db: Mutex::new(connection),
                cancel_extract: AtomicBool::new(false),
                extract_running: Arc::new(AtomicBool::new(false)),
                thumbs_dir: thumbs,
                previews_dir: previews,
                receiver_error: Mutex::new(None),
                import_guard: Mutex::new(local_import::ImportGuard::default()),
                background_mode: AtomicBool::new(false),
                staged_left: Mutex::new(Vec::new()),
            });
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(connection) = state.db.lock() {
                    let _ = db::extension_token(&connection);
                }
            }
            if let Err(err) = local_import::start_receiver(app.handle().clone()) {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut slot) = state.receiver_error.lock() {
                        *slot = Some(err);
                    }
                }
            }
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            {
                app.deep_link()
                    .register_all()
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                let ours = event
                    .urls()
                    .iter()
                    .any(|url| url.as_str().to_ascii_lowercase().starts_with("vrclibman:"));
                if ours {
                    presence::show_main_later(&handle);
                }
            });
            let (background_mode, launch_at_login, left) = {
                let state = app.state::<AppState>();
                let mut connection = state
                    .db
                    .lock()
                    .map_err(|err| std::io::Error::other(err.to_string()))?;
                let settings =
                    db::settings(&connection).map_err(|err| std::io::Error::other(err))?;
                let report = db::sweep_staged_downloads(&mut connection);
                (
                    settings.background_mode,
                    settings.launch_at_login,
                    report.left,
                )
            };
            presence::remember_background(app.handle(), background_mode);
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(mut slot) = state.staged_left.lock() {
                    *slot = left;
                }
            }
            crate::remote_sync::start(app.handle());
            let _ = presence::sync_login_item(
                app.handle(),
                presence::autostart_should_enable(background_mode, launch_at_login),
            );
            if background_mode {
                let _ = presence::ensure_tray(app.handle());
            }
            if presence::should_open_window_on_start(
                background_mode,
                presence::launched_by_login_item(std::env::args()),
            ) {
                presence::show_main_window(app.handle())
                    .map_err(|err| std::io::Error::other(err))?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::updater_status,
            commands::check_app_update,
            commands::download_app_update,
            commands::install_app_update,
            commands::import_export,
            commands::extension_link,
            commands::integration_status,
            commands::set_booth_import_mode,
            commands::regenerate_extension_token,
            commands::library_stats,
            commands::list_managed_products,
            commands::set_product_excluded,
            commands::list_avatars,
            commands::list_products_by_category,
            commands::list_products_by_genre,
            commands::get_avatar,
            commands::probe_remote_files,
            commands::probe_remote_products,
            commands::list_queue,
            commands::list_product_options,
            commands::list_knowledge,
            commands::get_settings,
            commands::save_settings,
            commands::update_settings,
            commands::get_booth_page_interval,
            commands::set_booth_page_interval,
            commands::update_product,
            commands::move_product_genre,
            commands::restore_product_classification,
            commands::unpin_product_classification,
            commands::register_owned_body,
            commands::classification_feedback,
            commands::save_classification_feedback,
            commands::attach_product_to_avatar,
            commands::link_file,
            commands::save_avatar,
            commands::delete_avatar,
            commands::save_base_body,
            commands::delete_base_body,
            commands::merge_avatars,
            commands::restore_seed,
            commands::reresolve,
            commands::scan_library,
            commands::classification_status,
            commands::extract_products,
            commands::cancel_extract,
            commands::test_ollama,
            commands::cloud_key_status,
            commands::save_cloud_api_key,
            commands::delete_cloud_api_key,
            commands::test_cloud_classifier,
            commands::ollama_loaded_models,
            commands::unload_ollama_model,
            commands::apple_fm_status,
            commands::catalog_info,
            commands::refresh_avatar_catalog,
            commands::setup_status,
            commands::complete_setup,
            commands::reopen_setup,
            commands::pull_ollama_model,
            commands::avatar_candidates,
            commands::register_avatar_candidate,
            commands::alias_avatar_candidate,
            commands::mark_owned_avatar,
            commands::add_free_product,
            commands::pull_remote_file,
            commands::push_remote_file,
            commands::remote_sync_status,
            commands::cache_thumbnails,
            commands::cache_product_preview,
            commands::open_booth_browser,
            commands::browser_state,
            commands::browser_action,
            commands::save_browser_bookmark,
            commands::delete_browser_bookmark,
            commands::show_booth_library,
            commands::browse_booth_history,
            commands::import_booth_page,
            commands::import_booth_library,
            commands::cancel_booth_library,
            commands::clear_booth_session,
            commands::close_booth_browser,
            commands::booth_browser_log,
            commands::download_booth_file,
            commands::booth_account,
            commands::list_staged_downloads,
            commands::route_staged_download,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| presence::handle_run_event(app, event));
}
