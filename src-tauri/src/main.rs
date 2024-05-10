// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod screen;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn clear_image_history() -> String {
    screen::clear_screen_dir();
    "Image history cleared".to_string()
}

#[tauri::command]
fn get_encryption_status() -> bool {
    screen::is_encryption_enabled()
}

#[tauri::command]
fn toggle_encryption(enable: bool) -> String {
    screen::toggle_settings("encryption_enabled", enable);
    format!("Encryption enabled: {}", enable)
}

#[tauri::command]
fn get_periodic_capture_status() -> bool {
    screen::is_periodic_capture_enabled()
}

#[tauri::command]
fn toggle_periodic_capture(enable: bool) -> String {
    screen::toggle_settings("periodic_capture_enabled", enable);
    format!("Periodic capture enabled: {}", enable)
}

#[tauri::command]
fn get_click_event_status() -> bool {
    screen::is_click_event_enabled()
}

#[tauri::command]
fn toggle_click_event(enable: bool) -> String {
    screen::toggle_settings("click_event_enabled", enable);
    format!("Click event enabled: {}", enable)
}

#[tokio::main]
async fn main() {
    tauri::Builder::default()
        .setup(screen::setup_handler)
        .invoke_handler(tauri::generate_handler![
            greet,
            clear_image_history,
            toggle_encryption,
            toggle_periodic_capture,
            toggle_click_event,
            get_encryption_status,
            get_periodic_capture_status,
            get_click_event_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
