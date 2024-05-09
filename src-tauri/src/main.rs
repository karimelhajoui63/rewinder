// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod screen;

// Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
#[tauri::command]
fn greet(app_handle: tauri::AppHandle, name: &str) -> String {
    let app_dir = app_handle.path_resolver().app_local_data_dir();
    if let Some(app_dir) = app_dir {
        println!("app_local_data_dir: {}", app_dir.display());
    }
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tokio::main]
async fn main() {
    tauri::Builder::default()
        .setup(screen::setup_handler)
        .invoke_handler(tauri::generate_handler![greet])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
