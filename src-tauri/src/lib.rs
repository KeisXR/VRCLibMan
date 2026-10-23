mod apple_fm;
mod browser;
mod mask;
mod catalog;
mod commands;
mod compat;
mod db;
mod import_data;
mod library_files;
mod library_walk;
mod local_import;
mod matching;
mod model;
mod normalize;
mod ollama;
mod quiet;
mod resolve;
mod scan;
mod thumb;
mod view;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub cancel_extract: AtomicBool,
    pub extract_running: Arc<AtomicBool>,
    pub thumbs_dir: std::path::PathBuf,
    pub receiver_error: Mutex<Option<String>>,
    pub import_guard: Mutex<local_import::ImportGuard>,
}

fn focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            focus_main(app);
        }));
    }
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri_plugin_deep_link::DeepLinkExt;
            let directory = app.path().app_data_dir()?;
            std::fs::create_dir_all(&directory)?;
            let thumbs = directory.join("thumbs");
            std::fs::create_dir_all(&thumbs)?;
            let connection = db::open(&directory.join("library.sqlite"))
                .map_err(|err| std::io::Error::other(err))?;
            app.manage(AppState {
                db: Mutex::new(connection),
                cancel_extract: AtomicBool::new(false),
                extract_running: Arc::new(AtomicBool::new(false)),
                thumbs_dir: thumbs,
                receiver_error: Mutex::new(None),
                import_guard: Mutex::new(local_import::ImportGuard::default()),
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
                    focus_main(&handle);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import_export,
            commands::extension_link,
            commands::regenerate_extension_token,
            commands::library_stats,
            commands::list_avatars,
            commands::get_avatar,
            commands::probe_remote_files,
            commands::list_queue,
            commands::list_product_options,
            commands::list_knowledge,
            commands::get_settings,
            commands::save_settings,
            commands::update_product,
            commands::link_file,
            commands::save_avatar,
            commands::delete_avatar,
            commands::save_base_body,
            commands::delete_base_body,
            commands::merge_avatars,
            commands::restore_seed,
            commands::reresolve,
            commands::scan_library,
            commands::extract_products,
            commands::cancel_extract,
            commands::test_ollama,
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
            commands::cache_thumbnails,
            commands::open_booth_browser,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
