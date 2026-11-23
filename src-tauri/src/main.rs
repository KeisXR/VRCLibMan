// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // AI エージェントが MCP サーバーとして起動したとき。窓もトレイも作らず、stdio だけで答える。
    if std::env::args().skip(1).any(|arg| arg == "--mcp") {
        tauri_app_lib::mcp_stdio::run();
        return;
    }
    tauri_app_lib::run()
}
