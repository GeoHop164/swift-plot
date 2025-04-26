#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use tauri::Manager;

// Example command: Greet function
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! Greetings from Rust!", name)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            greet, // register your commands here
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
