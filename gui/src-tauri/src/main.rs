// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use tauri_plugin_positioner::{Position, WindowExt};

#[tauri::command]
async fn set_frame(app_handle: tauri::AppHandle, visible: bool) {
    let win = app_handle.get_webview_window("main").unwrap();
    win.set_shadow(visible).unwrap();
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let win = app.get_webview_window("main").unwrap();
            win.move_window(Position::BottomRight).unwrap();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_frame])
        .run(tauri::generate_context!())
        .unwrap();

    tsuki_gui_lib::run()
}
