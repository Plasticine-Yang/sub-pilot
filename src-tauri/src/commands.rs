//! Tauri command layer — a thin shell (per spec) that forwards frontend
//! requests to the subtitle/orchestration logic and relays progress as events.
//! No business logic lives here.

use crate::media::FfmpegMediaProcessor;
use crate::project::{self, Project, ProjectStatus};
use crate::selfcheck::{run_self_check, SelfCheckReport};
use crate::transcribe::{run_transcription, Clock, MediaProcessor, TranscriptionEvent};
use crate::whisper::WhisperTranscriber;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Event channel the frontend subscribes to for transcription updates.
pub const EVENT_TRANSCRIPTION: &str = "transcription://event";

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

/// Imports a video: probes its duration, builds the project (validating the
/// container format), creates the project directory + `project.json`, and
/// returns the new Project.
#[tauri::command]
pub fn import_video(app: AppHandle, path: String) -> Result<Project, String> {
    let video = PathBuf::from(&path);
    let media = FfmpegMediaProcessor::new(ffmpeg_path(&app)?);
    let duration_ms = media.probe_duration_ms(&video)?;

    let id = uuid::Uuid::new_v4().to_string();
    let project = Project::new_import(id.clone(), video, duration_ms).map_err(|e| e.to_string())?;

    let dir = project_dir(&app, &id)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建项目目录失败：{e}"))?;
    project::save(&dir, &project).map_err(|e| format!("保存 project.json 失败：{e}"))?;
    Ok(project)
}

/// Starts transcription for a project on a background thread, emitting
/// status/progress events. Returns immediately; results arrive via events and
/// `read_original_srt`.
#[tauri::command]
pub fn start_transcription(app: AppHandle, project_id: String) -> Result<(), String> {
    let dir = project_dir(&app, &project_id)?;
    let mut proj = project::load(&dir).map_err(|e| e.to_string())?;

    let media = FfmpegMediaProcessor::new(ffmpeg_path(&app)?);
    let transcriber = WhisperTranscriber::new(whisper_path(&app)?, model_dir(&app)?);

    std::thread::spawn(move || {
        let clock = SystemClock::new();
        let emitter = app.clone();
        let pid = project_id.clone();
        let mut on_event = move |event: TranscriptionEvent| {
            let _ = emitter.emit(EVENT_TRANSCRIPTION, EmitPayload::new(&pid, event));
        };
        let _ = run_transcription(&media, &transcriber, &clock, &dir, &mut proj, &mut on_event);
    });
    Ok(())
}

/// Reads the generated `original.srt` for display in the UI.
#[tauri::command]
pub fn read_original_srt(app: AppHandle, project_id: String) -> Result<String, String> {
    let dir = project_dir(&app, &project_id)?;
    std::fs::read_to_string(dir.join(project::ORIGINAL_SRT))
        .map_err(|e| format!("读取字幕失败：{e}"))
}

/// Absolute paths the frontend needs for "open subtitle" / "open directory".
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectLocation {
    pub project_dir: String,
    pub original_srt: String,
}

/// Returns the on-disk locations for a project.
#[tauri::command]
pub fn project_location(app: AppHandle, project_id: String) -> Result<ProjectLocation, String> {
    let dir = project_dir(&app, &project_id)?;
    let srt = dir.join(project::ORIGINAL_SRT);
    Ok(ProjectLocation {
        project_dir: dir.to_string_lossy().to_string(),
        original_srt: srt.to_string_lossy().to_string(),
    })
}

// --- event payload -----------------------------------------------------------

/// Serializable transcription event tagged for the frontend. `project_id`
/// lets the UI correlate events to the active project.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum EmitPayload {
    Status {
        project_id: String,
        status: ProjectStatus,
    },
    Progress {
        project_id: String,
        fraction: f32,
        eta_ms: Option<u64>,
    },
}

impl EmitPayload {
    fn new(project_id: &str, event: TranscriptionEvent) -> Self {
        match event {
            TranscriptionEvent::StatusChanged(status) => EmitPayload::Status {
                project_id: project_id.to_string(),
                status,
            },
            TranscriptionEvent::Progress(p) => EmitPayload::Progress {
                project_id: project_id.to_string(),
                fraction: p.fraction,
                eta_ms: p.eta_ms,
            },
        }
    }
}

// --- clock -------------------------------------------------------------------

/// Real monotonic clock used in production; tests inject a deterministic fake.
/// Anchored at construction so `now_ms` returns elapsed milliseconds.
struct SystemClock {
    start: std::time::Instant,
}

impl SystemClock {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

// --- resource / storage resolution ------------------------------------------

fn resource_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resource_dir()
        .map_err(|e| format!("无法定位资源目录：{e}"))
}

fn ffmpeg_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(resource_dir(app)?.join("resources/ffmpeg/ffmpeg"))
}

fn model_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(resource_dir(app)?.join("resources/models"))
}

fn whisper_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(resource_dir(app)?.join("resources/whisper/whisper"))
}

fn projects_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{e}"))?
        .join("projects"))
}

fn project_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(projects_root(app)?.join(id))
}
