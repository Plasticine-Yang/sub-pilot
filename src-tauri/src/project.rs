//! Project persistence (ADR-0003): each Project is a directory holding the
//! source video reference, `original.srt`, and `project.json` (state machine,
//! model, ...). No database.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Canonical file names inside a Project directory.
pub const PROJECT_FILE: &str = "project.json";
pub const ORIGINAL_SRT: &str = "original.srt";
pub const TRANSLATED_SRT: &str = "translated.srt";

/// Explicit project state machine. Ticket 1 covers `imported → transcribing →
/// transcribed`; `failed` is the failure state. Later tickets extend it
/// (`prompt_ready`, `translation_imported`, `validated`, `exported`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Imported,
    Transcribing,
    Transcribed,
    PromptReady,
    TranslationImported,
    Validated,
    Exported,
    Failed,
}

/// The persisted state of one subtitle project. Serialized as `project.json`;
/// field names are camelCase so the frontend consumes it directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub video_path: PathBuf,
    pub video_file_name: String,
    pub duration_ms: u64,
    pub model: String,
    pub status: ProjectStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Video containers the importer accepts (lower-case extensions).
pub const SUPPORTED_EXT: [&str; 4] = ["mp4", "mkv", "mov", "avi"];

/// Bundled default model, transcribed with unless the user picks another
/// (ADR-0002: `base` is the built-in default).
pub const DEFAULT_MODEL: &str = "base";

/// Why a video could not be imported into a new project.
#[derive(Debug, PartialEq, Eq)]
pub enum ImportError {
    UnsupportedFormat,
    InvalidFileName,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::UnsupportedFormat => {
                write!(f, "不支持的文件格式，请选择 MP4 / MKV / MOV / AVI")
            }
            ImportError::InvalidFileName => write!(f, "无效的文件名"),
        }
    }
}

impl Project {
    /// Builds a freshly-imported project from a video path and its probed
    /// duration, validating the container format and deriving the display name.
    /// The default model is used; the state starts at `imported`.
    pub fn new_import(id: String, video: PathBuf, duration_ms: u64) -> Result<Self, ImportError> {
        let ext = video
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);
        match ext {
            Some(e) if SUPPORTED_EXT.contains(&e.as_str()) => {}
            _ => return Err(ImportError::UnsupportedFormat),
        }
        let file_name = video
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(ImportError::InvalidFileName)?
            .to_string();
        Ok(Project {
            id,
            video_path: video,
            video_file_name: file_name,
            duration_ms,
            model: DEFAULT_MODEL.to_string(),
            status: ProjectStatus::Imported,
            error: None,
        })
    }
}

/// Why a `project.json` could not be loaded from disk.
#[derive(Debug)]
pub enum ProjectLoadError {
    Io(std::io::Error),
    Parse(serde_json::Error),
}

impl std::fmt::Display for ProjectLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectLoadError::Io(e) => write!(f, "读取 project.json 失败：{e}"),
            ProjectLoadError::Parse(e) => write!(f, "解析 project.json 失败：{e}"),
        }
    }
}

impl std::error::Error for ProjectLoadError {}

/// Writes `project.json` into the project directory (pretty-printed).
pub fn save(project_dir: &Path, project: &Project) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(project).map_err(std::io::Error::from)?;
    std::fs::write(project_dir.join(PROJECT_FILE), json)
}

/// Reads and parses `project.json` from the project directory.
pub fn load(project_dir: &Path) -> Result<Project, ProjectLoadError> {
    let raw =
        std::fs::read_to_string(project_dir.join(PROJECT_FILE)).map_err(ProjectLoadError::Io)?;
    serde_json::from_str(&raw).map_err(ProjectLoadError::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Project {
        Project {
            id: "proj-123".to_string(),
            video_path: PathBuf::from("/videos/ep01.mkv"),
            video_file_name: "ep01.mkv".to_string(),
            duration_ms: 1_234_000,
            model: "base".to_string(),
            status: ProjectStatus::Imported,
            error: None,
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let project = sample();
        save(dir.path(), &project).unwrap();
        assert_eq!(load(dir.path()).unwrap(), project);
    }

    #[test]
    fn new_import_accepts_supported_formats_case_insensitively() {
        let project =
            Project::new_import("p1".to_string(), PathBuf::from("/v/Show.MP4"), 5000).unwrap();
        assert_eq!(project.video_file_name, "Show.MP4");
        assert_eq!(project.duration_ms, 5000);
        assert_eq!(project.model, DEFAULT_MODEL);
        assert_eq!(project.status, ProjectStatus::Imported);
    }

    #[test]
    fn new_import_rejects_unsupported_format() {
        let err =
            Project::new_import("p1".to_string(), PathBuf::from("/v/clip.webm"), 5000).unwrap_err();
        assert_eq!(err, ImportError::UnsupportedFormat);
    }

    #[test]
    fn new_import_rejects_path_without_extension() {
        let err =
            Project::new_import("p1".to_string(), PathBuf::from("/v/noext"), 5000).unwrap_err();
        assert_eq!(err, ImportError::UnsupportedFormat);
    }

    #[test]
    fn status_serializes_as_snake_case() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = sample();
        project.status = ProjectStatus::Transcribing;
        save(dir.path(), &project).unwrap();
        let raw = std::fs::read_to_string(dir.path().join(PROJECT_FILE)).unwrap();
        assert!(raw.contains("\"status\": \"transcribing\""), "got: {raw}");
        assert!(
            raw.contains("\"videoFileName\": \"ep01.mkv\""),
            "got: {raw}"
        );
    }

    #[test]
    fn error_field_omitted_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &sample()).unwrap();
        let raw = std::fs::read_to_string(dir.path().join(PROJECT_FILE)).unwrap();
        assert!(
            !raw.contains("error"),
            "error should be omitted, got: {raw}"
        );
    }
}
