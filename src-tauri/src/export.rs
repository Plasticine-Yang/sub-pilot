//! Subtitle export orchestration (接缝 2).
//!
//! Drives the validated Translated Subtitle out of a Project — either as a
//! Sidecar `.srt` next to the video, or muxed into the video as a switchable
//! soft subtitle track (ADR: 外挂字幕). Burn-in (ticket 6) is a separate path.
//! Orchestration depends only on the `MediaProcessor` trait so it is exercised
//! with a fake; the real ffmpeg mux is covered by manual runs.

use crate::project::{self, Project, ProjectStatus, TRANSLATED_SRT};
use crate::transcribe::MediaProcessor;
use std::path::{Path, PathBuf};

/// Failure of an export, mapped to a user-facing message.
#[derive(Debug)]
pub enum ExportError {
    /// The project isn't in a state that can be exported (needs `validated`).
    NotValidated,
    /// The ffmpeg soft-subtitle mux failed.
    Mux(String),
    /// The ffmpeg burn-in export failed.
    BurnIn(String),
    Io(std::io::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::NotValidated => {
                write!(f, "请先导入并校验译文字幕后再导出")
            }
            ExportError::Mux(m) => write!(f, "封装软字幕失败：{m}"),
            ExportError::BurnIn(m) => write!(f, "烧录字幕失败：{m}"),
            ExportError::Io(e) => write!(f, "导出文件失败：{e}"),
        }
    }
}

impl std::error::Error for ExportError {}

/// Copies the project's validated `translated.srt` next to the video (Sidecar),
/// returning the written path. The file is named after the video stem so the
/// two travel together (`ep01.mkv` → `ep01.zh.srt`).
pub fn export_sidecar(
    project_dir: &Path,
    project: &Project,
    out_dir: &Path,
) -> Result<PathBuf, ExportError> {
    require_validated(project)?;
    let out_path = out_dir.join(sidecar_file_name(&project.video_file_name));
    std::fs::copy(project_dir.join(TRANSLATED_SRT), &out_path).map_err(ExportError::Io)?;
    Ok(out_path)
}

/// Muxes the validated subtitle into the source video as a switchable soft
/// subtitle track, advancing the project to `exported` on success. Returns the
/// written video path.
pub fn export_soft_subtitle(
    media: &dyn MediaProcessor,
    project_dir: &Path,
    project: &mut Project,
    out_dir: &Path,
) -> Result<PathBuf, ExportError> {
    require_validated(project)?;
    let out_path = out_dir.join(soft_sub_file_name(&project.video_file_name));
    media
        .mux_subtitle(
            &project.video_path,
            &project_dir.join(TRANSLATED_SRT),
            &out_path,
        )
        .map_err(ExportError::Mux)?;
    project.status = ProjectStatus::Exported;
    project::save(project_dir, project).map_err(ExportError::Io)?;
    Ok(out_path)
}

/// Burns the validated subtitle into the video's pixels (Burn-in), relaying
/// progress via `on_progress` and advancing the project to `exported` on
/// success. Returns the written video path.
pub fn export_burn_in(
    media: &dyn MediaProcessor,
    project_dir: &Path,
    project: &mut Project,
    out_dir: &Path,
    on_progress: &mut dyn FnMut(f32),
) -> Result<PathBuf, ExportError> {
    require_validated(project)?;
    let out_path = out_dir.join(burned_file_name(&project.video_file_name));
    media
        .burn_in(
            &project.video_path,
            &project_dir.join(TRANSLATED_SRT),
            &out_path,
            project.duration_ms,
            on_progress,
        )
        .map_err(ExportError::BurnIn)?;
    project.status = ProjectStatus::Exported;
    project::save(project_dir, project).map_err(ExportError::Io)?;
    Ok(out_path)
}

fn require_validated(project: &Project) -> Result<(), ExportError> {
    match project.status {
        ProjectStatus::Validated | ProjectStatus::Exported => Ok(()),
        _ => Err(ExportError::NotValidated),
    }
}

/// `ep01.mkv` → `ep01.zh.srt`. Falls back to appending when there is no dot.
fn sidecar_file_name(video_file_name: &str) -> String {
    format!("{}.zh.srt", strip_ext(video_file_name))
}

/// `ep01.mkv` → `ep01.softsub.mkv`, preserving the original container.
fn soft_sub_file_name(video_file_name: &str) -> String {
    match video_file_name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.softsub.{ext}"),
        None => format!("{video_file_name}.softsub"),
    }
}

/// `ep01.mkv` → `ep01.burned.mkv`, preserving the original container.
fn burned_file_name(video_file_name: &str) -> String {
    match video_file_name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.burned.{ext}"),
        None => format!("{video_file_name}.burned"),
    }
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn validated_project() -> Project {
        Project {
            id: "p1".to_string(),
            video_path: PathBuf::from("/videos/ep01.mkv"),
            video_file_name: "ep01.mkv".to_string(),
            duration_ms: 60_000,
            model: "base".to_string(),
            status: ProjectStatus::Validated,
            error: None,
        }
    }

    /// Records the arguments a mux call received so the test can assert on the
    /// orchestration without touching ffmpeg.
    struct FakeMedia {
        result: Result<(), String>,
        last_call: RefCell<Option<(PathBuf, PathBuf, PathBuf)>>,
        /// Progress fractions the fake `burn_in` replays before returning.
        burn_progresses: Vec<f32>,
    }

    impl FakeMedia {
        fn ok() -> Self {
            Self {
                result: Ok(()),
                last_call: RefCell::new(None),
                burn_progresses: vec![],
            }
        }
        fn failing() -> Self {
            Self {
                result: Err("codec not found".to_string()),
                last_call: RefCell::new(None),
                burn_progresses: vec![],
            }
        }
        fn burning(progresses: Vec<f32>) -> Self {
            Self {
                result: Ok(()),
                last_call: RefCell::new(None),
                burn_progresses: progresses,
            }
        }
    }

    impl MediaProcessor for FakeMedia {
        fn probe_duration_ms(&self, _video: &Path) -> Result<u64, String> {
            unreachable!()
        }
        fn extract_audio(&self, _video: &Path, _out: &Path) -> Result<(), String> {
            unreachable!()
        }
        fn mux_subtitle(
            &self,
            video: &Path,
            subtitle: &Path,
            out_video: &Path,
        ) -> Result<(), String> {
            *self.last_call.borrow_mut() = Some((
                video.to_path_buf(),
                subtitle.to_path_buf(),
                out_video.to_path_buf(),
            ));
            self.result.clone()
        }
        fn burn_in(
            &self,
            video: &Path,
            subtitle: &Path,
            out_video: &Path,
            _total_duration_ms: u64,
            on_progress: &mut dyn FnMut(f32),
        ) -> Result<(), String> {
            *self.last_call.borrow_mut() = Some((
                video.to_path_buf(),
                subtitle.to_path_buf(),
                out_video.to_path_buf(),
            ));
            for f in &self.burn_progresses {
                on_progress(*f);
            }
            self.result.clone()
        }
    }

    #[test]
    fn sidecar_copies_translated_srt_named_after_the_video() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRANSLATED_SRT), "1\n...\n你好\n").unwrap();

        let path = export_sidecar(dir.path(), &validated_project(), out.path()).unwrap();

        assert_eq!(path, out.path().join("ep01.zh.srt"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1\n...\n你好\n");
    }

    #[test]
    fn sidecar_refuses_when_not_validated() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let mut project = validated_project();
        project.status = ProjectStatus::PromptReady;

        let err = export_sidecar(dir.path(), &project, out.path()).unwrap_err();
        assert!(matches!(err, ExportError::NotValidated));
    }

    #[test]
    fn soft_subtitle_muxes_and_advances_to_exported() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRANSLATED_SRT), "sub").unwrap();
        let media = FakeMedia::ok();
        let mut project = validated_project();

        let path = export_soft_subtitle(&media, dir.path(), &mut project, out.path()).unwrap();

        assert_eq!(path, out.path().join("ep01.softsub.mkv"));
        assert_eq!(project.status, ProjectStatus::Exported);
        // Orchestration passed the source video, the project's translated.srt,
        // and the computed output path through to the media processor.
        let call = media.last_call.borrow();
        let (video, subtitle, out_video) = call.as_ref().unwrap();
        assert_eq!(video, &PathBuf::from("/videos/ep01.mkv"));
        assert_eq!(subtitle, &dir.path().join(TRANSLATED_SRT));
        assert_eq!(out_video, &out.path().join("ep01.softsub.mkv"));
        // Persisted.
        assert_eq!(
            project::load(dir.path()).unwrap().status,
            ProjectStatus::Exported
        );
    }

    #[test]
    fn soft_subtitle_mux_failure_leaves_state_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRANSLATED_SRT), "sub").unwrap();
        let media = FakeMedia::failing();
        let mut project = validated_project();

        let err = export_soft_subtitle(&media, dir.path(), &mut project, out.path()).unwrap_err();

        assert!(matches!(err, ExportError::Mux(_)));
        assert_eq!(project.status, ProjectStatus::Validated);
    }

    #[test]
    fn burn_in_relays_progress_and_advances_to_exported() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRANSLATED_SRT), "sub").unwrap();
        let media = FakeMedia::burning(vec![0.25, 0.5, 1.0]);
        let mut project = validated_project();
        let mut seen = Vec::new();

        let path = export_burn_in(&media, dir.path(), &mut project, out.path(), &mut |f| {
            seen.push(f)
        })
        .unwrap();

        assert_eq!(path, out.path().join("ep01.burned.mkv"));
        assert_eq!(seen, vec![0.25, 0.5, 1.0]);
        assert_eq!(project.status, ProjectStatus::Exported);
        let call = media.last_call.borrow();
        let (video, subtitle, out_video) = call.as_ref().unwrap();
        assert_eq!(video, &PathBuf::from("/videos/ep01.mkv"));
        assert_eq!(subtitle, &dir.path().join(TRANSLATED_SRT));
        assert_eq!(out_video, &out.path().join("ep01.burned.mkv"));
        assert_eq!(
            project::load(dir.path()).unwrap().status,
            ProjectStatus::Exported
        );
    }

    #[test]
    fn burn_in_failure_leaves_state_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(TRANSLATED_SRT), "sub").unwrap();
        let media = FakeMedia::failing();
        let mut project = validated_project();

        let err =
            export_burn_in(&media, dir.path(), &mut project, out.path(), &mut |_| {}).unwrap_err();

        assert!(matches!(err, ExportError::BurnIn(_)));
        assert_eq!(project.status, ProjectStatus::Validated);
    }

    #[test]
    fn burn_in_refuses_when_not_validated() {
        let dir = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        let media = FakeMedia::ok();
        let mut project = validated_project();
        project.status = ProjectStatus::Transcribed;

        let err =
            export_burn_in(&media, dir.path(), &mut project, out.path(), &mut |_| {}).unwrap_err();
        assert!(matches!(err, ExportError::NotValidated));
    }
}
