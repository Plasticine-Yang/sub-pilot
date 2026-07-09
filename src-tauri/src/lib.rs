mod commands;
mod selfcheck;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::self_check
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
