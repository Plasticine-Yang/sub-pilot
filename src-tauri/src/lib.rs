mod commands;
mod media;
mod project;
mod selfcheck;
mod subtitle;
mod transcribe;
mod whisper;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::self_check,
            commands::import_video,
            commands::start_transcription,
            commands::read_original_srt,
            commands::project_location
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
