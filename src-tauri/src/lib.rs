mod commands;
mod db;
mod import_data;
mod matching;
mod model;
mod normalize;
mod ollama;
mod resolve;
mod scan;
mod thumb;
mod view;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub cancel_extract: AtomicBool,
    pub extract_running: Arc<AtomicBool>,
    pub thumbs_dir: std::path::PathBuf,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri::Manager;
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
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import_export,
            commands::library_stats,
            commands::list_avatars,
            commands::get_avatar,
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
            commands::cache_thumbnails,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
