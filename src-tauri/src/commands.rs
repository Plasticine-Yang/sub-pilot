//! Tauri command layer — a thin shell (per spec) that forwards frontend
//! requests to the subtitle/orchestration logic and relays progress as events.
//! No business logic lives here.

use crate::download::HttpDownloader;
use crate::export;
use crate::media::FfmpegMediaProcessor;
use crate::models;
use crate::project::{self, Project, ProjectStatus};
use crate::recent;
use crate::selfcheck::{run_self_check, SelfCheckReport};
use crate::subtitle;
use crate::transcribe::{run_transcription, Clock, MediaProcessor, TranscriptionEvent};
use crate::whisper::WhisperTranscriber;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// Event channel the frontend subscribes to for transcription updates.
pub const EVENT_TRANSCRIPTION: &str = "transcription://event";

const WHISPER_UNAVAILABLE_MESSAGE: &str = "Whisper 运行时未随安装包正确部署，转写功能暂不可用。";

#[cfg(windows)]
const WHISPER_RESOURCES: &[&str] = &[
    "resources/whisper/windows/whisper/whisper.exe",
    "resources/whisper/venv/Scripts/whisper.exe",
];
#[cfg(not(windows))]
const WHISPER_RESOURCES: &[&str] = &["resources/whisper/whisper"];

#[cfg(windows)]
const FFMPEG_RESOURCE: &str = "resources/ffmpeg/ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_RESOURCE: &str = "resources/ffmpeg/ffmpeg";

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

/// Lists the selectable models and whether each is present on disk, for the
/// transcription model picker.
#[tauri::command]
pub fn list_models(app: AppHandle) -> Result<Vec<models::ModelStatus>, String> {
    Ok(models::model_statuses(
        &model_dir(&app)?,
        &downloads_model_dir(&app)?,
    ))
}

/// Event channel the frontend subscribes to for model-download updates.
pub const EVENT_MODEL_DOWNLOAD: &str = "model-download://event";

/// Downloads `model` on a background thread if missing, verifying its checksum,
/// emitting progress on `EVENT_MODEL_DOWNLOAD`. Returns immediately; a terminal
/// `done`/`error` event carries the outcome.
#[tauri::command]
pub fn download_model(app: AppHandle, model: models::ModelId) -> Result<(), String> {
    let dir = downloads_model_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建模型目录失败：{e}"))?;
    let info = model.info();

    std::thread::spawn(move || {
        let emitter = app.clone();
        let mut on_progress = |downloaded: u64, total: Option<u64>| {
            let _ = emitter.emit(EVENT_MODEL_DOWNLOAD, ModelDownloadPayload::Progress {
                model,
                downloaded,
                total,
            });
        };
        match models::ensure_model(&HttpDownloader, &dir, &info, &mut on_progress) {
            Ok(_) => {
                let _ = app.emit(EVENT_MODEL_DOWNLOAD, ModelDownloadPayload::Done { model });
            }
            Err(err) => {
                let _ = app.emit(EVENT_MODEL_DOWNLOAD, ModelDownloadPayload::Error {
                    model,
                    message: err.to_string(),
                });
            }
        }
    });
    Ok(())
}

/// Imports a video: probes its duration, builds the project (validating the
/// container format), creates the project directory + `project.json`, and
/// returns the new Project. `model` selects the transcription model (defaults
/// to the bundled base model when omitted).
#[tauri::command]
pub fn import_video(
    app: AppHandle,
    path: String,
    model: Option<String>,
) -> Result<Project, String> {
    let video = PathBuf::from(&path);
    let media = FfmpegMediaProcessor::new(ffmpeg_path(&app)?);
    let duration_ms = media.probe_duration_ms(&video)?;

    let id = uuid::Uuid::new_v4().to_string();
    let mut project =
        Project::new_import(id.clone(), video, duration_ms).map_err(|e| e.to_string())?;
    if let Some(model) = model {
        project.model = model;
    }

    let dir = project_dir(&app, &id)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建项目目录失败：{e}"))?;
    project::save(&dir, &project).map_err(|e| format!("保存 project.json 失败：{e}"))?;
    let _ = touch_recent(&app, &project);
    Ok(project)
}

/// Records/updates a project in the recent-projects index (best-effort; a
/// failure here must not fail the caller's primary operation).
fn touch_recent(app: &AppHandle, project: &Project) -> Result<(), String> {
    let data_dir = app_data_dir(app)?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let mut index = recent::load(&data_dir);
    index.upsert(recent::RecentEntry {
        id: project.id.clone(),
        video_file_name: project.video_file_name.clone(),
        status: project.status,
        updated_at: epoch_ms(),
    });
    recent::save(&data_dir, &index).map_err(|e| e.to_string())
}

/// Milliseconds since the Unix epoch, the recency key for the recent index.
fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Lists recent projects, refreshing each entry's status/name from its
/// authoritative `project.json` and dropping entries whose directory is gone
/// (graceful index/disk reconciliation). Persists the reconciled index.
#[tauri::command]
pub fn list_recent(app: AppHandle) -> Result<Vec<recent::RecentEntry>, String> {
    let data_dir = app_data_dir(&app)?;
    let mut index = recent::load(&data_dir);

    let mut existing = std::collections::HashSet::new();
    for entry in index.entries.iter_mut() {
        let dir = project_dir(&app, &entry.id)?;
        if let Ok(proj) = project::load(&dir) {
            // Refresh cached fields from the source of truth.
            entry.status = proj.status;
            entry.video_file_name = proj.video_file_name;
            existing.insert(entry.id.clone());
        }
    }
    index.reconcile(&existing);
    let _ = recent::save(&data_dir, &index);
    Ok(index.entries)
}

/// Reopens a project by id, returning its restored state from `project.json`
/// (the state machine step the home screen resumes from) plus its original and
/// (when validated) translated subtitle text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedProject {
    pub project: Project,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_srt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_srt: Option<String>,
}

/// Opens a recent project, restoring its persisted state. The original and, if
/// present, the validated translated subtitle are included so the UI can render
/// the restored step without a second round-trip.
#[tauri::command]
pub fn open_project(app: AppHandle, project_id: String) -> Result<OpenedProject, String> {
    let dir = project_dir(&app, &project_id)?;
    let project = project::load(&dir).map_err(|e| e.to_string())?;
    let original_srt = std::fs::read_to_string(dir.join(project::ORIGINAL_SRT)).ok();
    let translated_srt = std::fs::read_to_string(dir.join(project::TRANSLATED_SRT)).ok();
    let _ = touch_recent(&app, &project);
    Ok(OpenedProject {
        project,
        original_srt,
        translated_srt,
    })
}

/// Starts transcription for a project on a background thread, emitting
/// status/progress events. Returns immediately; results arrive via events and
/// `read_original_srt`.
#[tauri::command]
pub fn start_transcription(app: AppHandle, project_id: String) -> Result<(), String> {
    let dir = project_dir(&app, &project_id)?;
    let mut proj = project::load(&dir).map_err(|e| e.to_string())?;

    let media = FfmpegMediaProcessor::new(ffmpeg_path(&app)?);
    let transcriber = WhisperTranscriber::new(
        whisper_path(&app)?,
        model_dir(&app)?,
        downloads_model_dir(&app)?,
    );

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

/// Generates the translation Prompt from the project's `original.srt`, advances
/// the project to `prompt_ready`, and returns the Prompt text for the UI to
/// copy. Idempotent: re-running from a later state still produces the Prompt.
#[tauri::command]
pub fn generate_prompt(app: AppHandle, project_id: String) -> Result<String, String> {
    let dir = project_dir(&app, &project_id)?;
    let srt = std::fs::read_to_string(dir.join(project::ORIGINAL_SRT))
        .map_err(|e| format!("读取字幕失败：{e}"))?;
    let cues = subtitle::parse_srt(&srt).map_err(|e| format!("解析原始字幕失败：{e:?}"))?;
    let prompt = subtitle::build_translation_prompt(&cues);

    let mut proj = project::load(&dir).map_err(|e| e.to_string())?;
    if proj.status == ProjectStatus::Transcribed {
        proj.status = ProjectStatus::PromptReady;
        project::save(&dir, &proj).map_err(|e| format!("保存 project.json 失败：{e}"))?;
    }
    Ok(prompt)
}

/// Outcome of importing a Translated Subtitle: either the validated,
/// normalized SRT (original timeline + translated text) or a located hard error.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub ok: bool,
    /// The validated SRT to display; present only when `ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub srt: Option<String>,
    /// User-facing error message; present only when `!ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 1-based segment to locate the user to; present when the error is locatable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment: Option<usize>,
}

/// Imports a Translated Subtitle from `translated_path`, validates it against
/// the project's `original.srt` (ADR-0004 hard errors, timeline from original),
/// and on success writes the normalized `translated.srt` and advances the
/// project `translation_imported → validated`. Validation failures are returned
/// as a structured result (not an `Err`) so the UI can locate the bad segment.
#[tauri::command]
pub fn import_translation(
    app: AppHandle,
    project_id: String,
    translated_path: String,
) -> Result<ValidationResult, String> {
    let dir = project_dir(&app, &project_id)?;
    let original_srt = std::fs::read_to_string(dir.join(project::ORIGINAL_SRT))
        .map_err(|e| format!("读取原始字幕失败：{e}"))?;
    let original =
        subtitle::parse_srt(&original_srt).map_err(|e| format!("解析原始字幕失败：{e:?}"))?;

    let translated_bytes = std::fs::read(&translated_path)
        .map_err(|e| format!("读取译文字幕失败：{e}"))?;

    let mut proj = project::load(&dir).map_err(|e| e.to_string())?;
    proj.status = ProjectStatus::TranslationImported;
    project::save(&dir, &proj).map_err(|e| format!("保存 project.json 失败：{e}"))?;

    match subtitle::validate_translation(&original, &translated_bytes) {
        Ok(cues) => {
            let srt = subtitle::to_srt(&cues);
            std::fs::write(dir.join(project::TRANSLATED_SRT), &srt)
                .map_err(|e| format!("保存译文字幕失败：{e}"))?;
            proj.status = ProjectStatus::Validated;
            project::save(&dir, &proj).map_err(|e| format!("保存 project.json 失败：{e}"))?;
            Ok(ValidationResult {
                ok: true,
                srt: Some(srt),
                message: None,
                segment: None,
            })
        }
        Err(err) => Ok(ValidationResult {
            ok: false,
            srt: None,
            message: Some(err.to_string()),
            segment: err.segment(),
        }),
    }
}

/// Exports the validated translated subtitle as a Sidecar `.srt` into
/// `out_dir`, returning the written path.
#[tauri::command]
pub fn export_sidecar(
    app: AppHandle,
    project_id: String,
    out_dir: String,
) -> Result<String, String> {
    let dir = project_dir(&app, &project_id)?;
    let proj = project::load(&dir).map_err(|e| e.to_string())?;
    let path = export::export_sidecar(&dir, &proj, &PathBuf::from(out_dir))
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Muxes the validated subtitle into the video as a switchable soft subtitle
/// track in `out_dir`, advancing the project to `exported`. Returns the path.
#[tauri::command]
pub fn export_soft_subtitle(
    app: AppHandle,
    project_id: String,
    out_dir: String,
) -> Result<String, String> {
    let dir = project_dir(&app, &project_id)?;
    let mut proj = project::load(&dir).map_err(|e| e.to_string())?;
    let media = FfmpegMediaProcessor::new(ffmpeg_path(&app)?);
    let path = export::export_soft_subtitle(&media, &dir, &mut proj, &PathBuf::from(out_dir))
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Event channel the frontend subscribes to for burn-in export updates.
pub const EVENT_EXPORT: &str = "export://event";

/// Burns the validated subtitle into the video in `out_dir` on a background
/// thread, emitting progress events on `EVENT_EXPORT`. Returns immediately; a
/// terminal `done`/`error` event carries the outcome and output path.
#[tauri::command]
pub fn export_burn_in(
    app: AppHandle,
    project_id: String,
    out_dir: String,
) -> Result<(), String> {
    let dir = project_dir(&app, &project_id)?;
    let mut proj = project::load(&dir).map_err(|e| e.to_string())?;
    let media = FfmpegMediaProcessor::new(ffmpeg_path(&app)?).with_fonts_dir(fonts_dir(&app)?);
    let out = PathBuf::from(out_dir);

    std::thread::spawn(move || {
        let emitter = app.clone();
        let pid = project_id.clone();
        let mut on_progress = |fraction: f32| {
            let _ = emitter.emit(EVENT_EXPORT, ExportPayload::Progress {
                project_id: pid.clone(),
                fraction,
            });
        };
        match export::export_burn_in(&media, &dir, &mut proj, &out, &mut on_progress) {
            Ok(path) => {
                let _ = app.emit(EVENT_EXPORT, ExportPayload::Done {
                    project_id,
                    path: path.to_string_lossy().to_string(),
                });
            }
            Err(err) => {
                let _ = app.emit(EVENT_EXPORT, ExportPayload::Error {
                    project_id,
                    message: err.to_string(),
                });
            }
        }
    });
    Ok(())
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
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
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

/// Serializable burn-in export event tagged for the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum ExportPayload {
    Progress { project_id: String, fraction: f32 },
    Done { project_id: String, path: String },
    Error { project_id: String, message: String },
}

/// Serializable model-download event tagged for the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum ModelDownloadPayload {
    Progress {
        model: models::ModelId,
        downloaded: u64,
        total: Option<u64>,
    },
    Done {
        model: models::ModelId,
    },
    Error {
        model: models::ModelId,
        message: String,
    },
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
    Ok(resource_dir(app)?.join(FFMPEG_RESOURCE))
}

fn model_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(resource_dir(app)?.join("resources/models"))
}

/// Writable directory for on-demand model downloads (ADR-0003: mutable state
/// lives in the app-data dir, never the read-only app bundle).
fn downloads_model_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("models"))
}

fn fonts_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(resource_dir(app)?.join("resources/fonts"))
}

fn whisper_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = resource_dir(app)?;
    let path = WHISPER_RESOURCES
        .iter()
        .map(|rel| dir.join(rel))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| WHISPER_UNAVAILABLE_MESSAGE.to_string())?;
    if !path.is_file() {
        return Err(WHISPER_UNAVAILABLE_MESSAGE.to_string());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = path
            .metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
        if !executable {
            return Err("Whisper 运行时存在但不可执行，转写功能暂不可用。".to_string());
        }
    }

    Ok(path)
}

fn projects_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("projects"))
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{e}"))
}

fn project_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(projects_root(app)?.join(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    // These lock the event payload's JSON keys to what the frontend reads
    // (src/lib/api.ts: `projectId`, `etaMs`). The enum-level `rename_all`
    // renames only the variant tag, NOT struct-variant fields — so without
    // `rename_all_fields` the backend emitted `project_id`/`eta_ms`, the UI's
    // `event.projectId` was undefined, every event was dropped by the id guard,
    // and transcription froze at "转写中 0%". Guards that regression.

    #[test]
    fn transcription_progress_uses_camelcase_keys_the_ui_reads() {
        let payload = EmitPayload::new(
            "p1",
            TranscriptionEvent::Progress(crate::transcribe::TranscriptionProgress {
                fraction: 0.5,
                eta_ms: Some(1000),
            }),
        );
        let v: Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(v["kind"], "progress");
        assert_eq!(v["projectId"], "p1", "UI reads event.projectId; got {v}");
        assert_eq!(v["etaMs"], 1000, "UI reads event.etaMs; got {v}");
        assert!(v.get("project_id").is_none(), "stale snake_case key: {v}");
        assert!(v.get("eta_ms").is_none(), "stale snake_case key: {v}");
    }

    #[test]
    fn transcription_status_uses_camelcase_keys_the_ui_reads() {
        let payload = EmitPayload::new(
            "p1",
            TranscriptionEvent::StatusChanged(ProjectStatus::Transcribed),
        );
        let v: Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(v["kind"], "status");
        assert_eq!(v["projectId"], "p1", "UI reads event.projectId; got {v}");
        assert_eq!(v["status"], "transcribed");
        assert!(v.get("project_id").is_none(), "stale snake_case key: {v}");
    }

    #[test]
    fn export_progress_uses_camelcase_keys_the_ui_reads() {
        let payload = ExportPayload::Progress {
            project_id: "p1".to_string(),
            fraction: 0.25,
        };
        let v: Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(v["projectId"], "p1", "UI reads event.projectId; got {v}");
        assert!(v.get("project_id").is_none(), "stale snake_case key: {v}");
    }
}
