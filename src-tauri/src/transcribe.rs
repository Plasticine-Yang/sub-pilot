//! Transcription orchestration (接缝 2).
//!
//! Defines the external-process adapter traits (`MediaProcessor`,
//! `Transcriber`) and the orchestration that drives a Project through
//! `imported → transcribing → transcribed` (or `failed`), relaying progress
//! with an estimated time remaining. Orchestration depends only on the traits
//! so it is exercised with fakes; the real ffmpeg/whisper implementations live
//! elsewhere and are covered by manual end-to-end runs.

use crate::project::{self, ORIGINAL_SRT};
use crate::project::{Project, ProjectStatus};
use crate::subtitle::{to_srt, SubtitleCue};
use std::path::Path;

/// Wall-clock source, injected so ETA computation is deterministic in tests.
pub trait Clock {
    /// Monotonic milliseconds since an arbitrary fixed epoch.
    fn now_ms(&self) -> u64;
}

/// ffmpeg-backed operations. Real impl shells out to the bundled binary.
pub trait MediaProcessor {
    /// Probes the media container for its duration in milliseconds.
    fn probe_duration_ms(&self, video: &Path) -> Result<u64, String>;
    /// Extracts an audio track suitable for transcription to `out_wav`.
    fn extract_audio(&self, video: &Path, out_wav: &Path) -> Result<(), String>;
}

/// whisper-backed transcription. Real impl shells out to Python whisper.
pub trait Transcriber {
    /// Transcribes `audio` with `model`, reporting fractional progress
    /// (`0.0..=1.0`) through `on_progress`, and returns the recognized cues.
    /// `total_duration_ms` is the audio length, used to turn a segment's
    /// timestamp into a fraction.
    fn transcribe(
        &self,
        audio: &Path,
        model: &str,
        total_duration_ms: u64,
        on_progress: &mut dyn FnMut(f32),
    ) -> Result<Vec<SubtitleCue>, String>;
}

/// A single progress update relayed to the frontend during transcription.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionProgress {
    /// Fraction complete, `0.0..=1.0`.
    pub fraction: f32,
    /// Estimated milliseconds remaining, `None` until there is enough signal.
    pub eta_ms: Option<u64>,
}

/// What the frontend observes as transcription proceeds.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptionEvent {
    StatusChanged(ProjectStatus),
    Progress(TranscriptionProgress),
}

/// Failure of the transcription pipeline, mapped from adapter errors.
#[derive(Debug)]
pub enum TranscriptionError {
    Media(String),
    Transcribe(String),
    Io(std::io::Error),
}

impl std::fmt::Display for TranscriptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranscriptionError::Media(m) => write!(f, "抽取音频失败：{m}"),
            TranscriptionError::Transcribe(m) => write!(f, "转写失败：{m}"),
            TranscriptionError::Io(e) => write!(f, "写入项目文件失败：{e}"),
        }
    }
}

impl std::error::Error for TranscriptionError {}

/// Estimates remaining milliseconds from elapsed time and fraction complete.
/// Returns `None` when there is no usable signal yet (fraction ≤ 0).
pub fn estimate_remaining_ms(elapsed_ms: u64, fraction: f32) -> Option<u64> {
    if fraction <= 0.0 {
        return None;
    }
    let fraction = fraction.min(1.0);
    let total = elapsed_ms as f64 / fraction as f64;
    let remaining = (total - elapsed_ms as f64).max(0.0);
    Some(remaining.round() as u64)
}

/// Runs the transcription pipeline for `project` in `project_dir`, persisting
/// each state transition to `project.json`, writing `original.srt`, and
/// relaying status/progress via `on_event`. On any failure the project is set
/// to `failed` (with the error recorded) before the error is returned.
pub fn run_transcription(
    media: &dyn MediaProcessor,
    transcriber: &dyn Transcriber,
    clock: &dyn Clock,
    project_dir: &Path,
    project: &mut Project,
    on_event: &mut dyn FnMut(TranscriptionEvent),
) -> Result<(), TranscriptionError> {
    set_status(project_dir, project, ProjectStatus::Transcribing, on_event)?;

    let started = clock.now_ms();
    let result = run_inner(
        media,
        transcriber,
        clock,
        project_dir,
        project,
        started,
        on_event,
    );

    match result {
        Ok(()) => {
            set_status(project_dir, project, ProjectStatus::Transcribed, on_event)?;
            Ok(())
        }
        Err(err) => {
            project.status = ProjectStatus::Failed;
            project.error = Some(err.to_string());
            // Best-effort persist of the failure; ignore a secondary IO error
            // so the original cause surfaces.
            let _ = project::save(project_dir, project);
            on_event(TranscriptionEvent::StatusChanged(ProjectStatus::Failed));
            Err(err)
        }
    }
}

/// The fallible core: extract audio, transcribe with progress, write SRT.
fn run_inner(
    media: &dyn MediaProcessor,
    transcriber: &dyn Transcriber,
    clock: &dyn Clock,
    project_dir: &Path,
    project: &Project,
    started_ms: u64,
    on_event: &mut dyn FnMut(TranscriptionEvent),
) -> Result<(), TranscriptionError> {
    let audio_path = project_dir.join("audio.wav");
    media
        .extract_audio(&project.video_path, &audio_path)
        .map_err(TranscriptionError::Media)?;

    let cues = {
        let mut on_progress = |fraction: f32| {
            let elapsed = clock.now_ms().saturating_sub(started_ms);
            on_event(TranscriptionEvent::Progress(TranscriptionProgress {
                fraction,
                eta_ms: estimate_remaining_ms(elapsed, fraction),
            }));
        };
        transcriber
            .transcribe(
                &audio_path,
                &project.model,
                project.duration_ms,
                &mut on_progress,
            )
            .map_err(TranscriptionError::Transcribe)?
    };

    std::fs::write(project_dir.join(ORIGINAL_SRT), to_srt(&cues))
        .map_err(TranscriptionError::Io)?;
    Ok(())
}

/// Transitions the project to `status`, persists it, and emits the event.
fn set_status(
    project_dir: &Path,
    project: &mut Project,
    status: ProjectStatus,
    on_event: &mut dyn FnMut(TranscriptionEvent),
) -> Result<(), TranscriptionError> {
    project.status = status;
    project::save(project_dir, project).map_err(TranscriptionError::Io)?;
    on_event(TranscriptionEvent::StatusChanged(status));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::PathBuf;

    // --- estimate_remaining_ms (pure) ---------------------------------------

    #[test]
    fn eta_is_none_before_any_progress() {
        assert_eq!(estimate_remaining_ms(1000, 0.0), None);
    }

    #[test]
    fn eta_extrapolates_from_fraction() {
        // Half done after 1s → about 1s left.
        assert_eq!(estimate_remaining_ms(1000, 0.5), Some(1000));
        // A quarter done after 1s → about 3s left.
        assert_eq!(estimate_remaining_ms(1000, 0.25), Some(3000));
    }

    #[test]
    fn eta_is_zero_when_complete() {
        assert_eq!(estimate_remaining_ms(2000, 1.0), Some(0));
    }

    // --- fakes for the orchestration seam -----------------------------------

    struct FakeMedia {
        extract: Result<(), String>,
    }

    impl MediaProcessor for FakeMedia {
        fn probe_duration_ms(&self, _video: &Path) -> Result<u64, String> {
            Ok(60_000)
        }
        fn extract_audio(&self, _video: &Path, out_wav: &Path) -> Result<(), String> {
            self.extract.clone()?;
            std::fs::write(out_wav, b"fake wav").unwrap();
            Ok(())
        }
    }

    struct FakeTranscriber {
        progresses: Vec<f32>,
        result: Result<Vec<SubtitleCue>, String>,
    }

    impl Transcriber for FakeTranscriber {
        fn transcribe(
            &self,
            _audio: &Path,
            _model: &str,
            _total_duration_ms: u64,
            on_progress: &mut dyn FnMut(f32),
        ) -> Result<Vec<SubtitleCue>, String> {
            for f in &self.progresses {
                on_progress(*f);
            }
            self.result.clone()
        }
    }

    struct FakeClock {
        times: Vec<u64>,
        idx: Cell<usize>,
    }

    impl FakeClock {
        fn new(times: Vec<u64>) -> Self {
            Self {
                times,
                idx: Cell::new(0),
            }
        }
    }

    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            let i = self.idx.get();
            let v = *self
                .times
                .get(i)
                .or_else(|| self.times.last())
                .unwrap_or(&0);
            self.idx.set(i + 1);
            v
        }
    }

    fn sample_project() -> Project {
        Project {
            id: "p1".to_string(),
            video_path: PathBuf::from("/videos/ep01.mkv"),
            video_file_name: "ep01.mkv".to_string(),
            duration_ms: 60_000,
            model: "base".to_string(),
            status: ProjectStatus::Imported,
            error: None,
        }
    }

    fn cue(index: u32, start_ms: u64, end_ms: u64, text: &str) -> SubtitleCue {
        SubtitleCue {
            index,
            start_ms,
            end_ms,
            text: text.to_string(),
        }
    }

    #[test]
    fn happy_path_transitions_writes_srt_and_relays_progress() {
        let dir = tempfile::tempdir().unwrap();
        let cues = vec![cue(1, 0, 1000, "Hi"), cue(2, 1000, 2000, "Bye")];
        let media = FakeMedia { extract: Ok(()) };
        let transcriber = FakeTranscriber {
            progresses: vec![0.5, 1.0],
            result: Ok(cues.clone()),
        };
        // now_ms calls: start, then once per progress callback.
        let clock = FakeClock::new(vec![0, 1000, 2000]);
        let mut project = sample_project();
        let mut events = Vec::new();

        run_transcription(
            &media,
            &transcriber,
            &clock,
            dir.path(),
            &mut project,
            &mut |e| events.push(e),
        )
        .unwrap();

        assert_eq!(
            events,
            vec![
                TranscriptionEvent::StatusChanged(ProjectStatus::Transcribing),
                TranscriptionEvent::Progress(TranscriptionProgress {
                    fraction: 0.5,
                    eta_ms: Some(1000),
                }),
                TranscriptionEvent::Progress(TranscriptionProgress {
                    fraction: 1.0,
                    eta_ms: Some(0),
                }),
                TranscriptionEvent::StatusChanged(ProjectStatus::Transcribed),
            ]
        );
        assert_eq!(project.status, ProjectStatus::Transcribed);

        let srt = std::fs::read_to_string(dir.path().join(ORIGINAL_SRT)).unwrap();
        assert_eq!(srt, to_srt(&cues));

        // Persisted to project.json too.
        assert_eq!(
            project::load(dir.path()).unwrap().status,
            ProjectStatus::Transcribed
        );
    }

    #[test]
    fn transcriber_failure_sets_failed_and_records_error() {
        let dir = tempfile::tempdir().unwrap();
        let media = FakeMedia { extract: Ok(()) };
        let transcriber = FakeTranscriber {
            progresses: vec![],
            result: Err("whisper crashed".to_string()),
        };
        let clock = FakeClock::new(vec![0]);
        let mut project = sample_project();
        let mut events = Vec::new();

        let err = run_transcription(
            &media,
            &transcriber,
            &clock,
            dir.path(),
            &mut project,
            &mut |e| events.push(e),
        )
        .unwrap_err();

        assert!(matches!(err, TranscriptionError::Transcribe(_)));
        assert_eq!(
            events,
            vec![
                TranscriptionEvent::StatusChanged(ProjectStatus::Transcribing),
                TranscriptionEvent::StatusChanged(ProjectStatus::Failed),
            ]
        );
        let loaded = project::load(dir.path()).unwrap();
        assert_eq!(loaded.status, ProjectStatus::Failed);
        assert!(loaded.error.unwrap().contains("whisper crashed"));
    }

    #[test]
    fn media_failure_sets_failed_before_transcribing() {
        let dir = tempfile::tempdir().unwrap();
        let media = FakeMedia {
            extract: Err("ffmpeg missing".to_string()),
        };
        let transcriber = FakeTranscriber {
            progresses: vec![1.0],
            result: Ok(vec![cue(1, 0, 1000, "unused")]),
        };
        let clock = FakeClock::new(vec![0]);
        let mut project = sample_project();
        let mut events = Vec::new();

        let err = run_transcription(
            &media,
            &transcriber,
            &clock,
            dir.path(),
            &mut project,
            &mut |e| events.push(e),
        )
        .unwrap_err();

        assert!(matches!(err, TranscriptionError::Media(_)));
        assert_eq!(
            events,
            vec![
                TranscriptionEvent::StatusChanged(ProjectStatus::Transcribing),
                TranscriptionEvent::StatusChanged(ProjectStatus::Failed),
            ]
        );
        // No original.srt should have been produced.
        assert!(!dir.path().join(ORIGINAL_SRT).exists());
    }
}
