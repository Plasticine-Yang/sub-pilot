use crate::selfcheck::{run_self_check, SelfCheckReport};
use tauri::AppHandle;

/// Liveness probe proving the frontend↔Rust bridge is wired up.
#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}

/// First-launch self-check: verifies the bundled ffmpeg is executable and the
/// default base model file exists.
#[tauri::command]
pub fn self_check(app: AppHandle) -> SelfCheckReport {
    run_self_check(&app)
}
