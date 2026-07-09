mod commands;
mod download;
mod export;
mod media;
mod models;
mod project;
mod recent;
mod selfcheck;
mod subtitle;
mod transcribe;
mod whisper;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::self_check,
            commands::list_models,
            commands::download_model,
            commands::import_video,
            commands::list_recent,
            commands::open_project,
            commands::start_transcription,
            commands::read_original_srt,
            commands::generate_prompt,
            commands::import_translation,
            commands::export_sidecar,
            commands::export_soft_subtitle,
            commands::export_burn_in,
            commands::project_location
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
