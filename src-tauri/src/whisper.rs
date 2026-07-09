//! OpenAI whisper `Transcriber` (ADR-0005) — subprocess-backed real impl.
//!
//! Runs the bundled Python whisper as a child process, streaming its verbose
//! stdout to report progress, and reads back the `.srt` it writes. The verbose
//! line parser is a pure function tested against whisper's real output shape;
//! the subprocess wiring itself is covered by manual end-to-end runs.

use crate::subtitle::parse_srt;
use crate::subtitle::SubtitleCue;
use crate::transcribe::Transcriber;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `Transcriber` implemented by shelling out to the bundled OpenAI whisper CLI.
pub struct WhisperTranscriber {
    /// Path to the whisper executable (bundled Python entry point).
    whisper: PathBuf,
    /// Read-only bundle dir holding the built-in `base.pt`.
    bundled_model_dir: PathBuf,
    /// Writable app-data dir holding on-demand downloaded `<model>.pt` weights.
    downloads_model_dir: PathBuf,
}

impl WhisperTranscriber {
    pub fn new(
        whisper: PathBuf,
        bundled_model_dir: PathBuf,
        downloads_model_dir: PathBuf,
    ) -> Self {
        Self {
            whisper,
            bundled_model_dir,
            downloads_model_dir,
        }
    }

    /// The directory that actually holds `<model>.pt`: the downloads dir if the
    /// weight is there, else the bundle. whisper's `--model_dir` takes one path,
    /// so we resolve which one to hand it.
    fn model_dir_for(&self, model: &str) -> &Path {
        let file = format!("{model}.pt");
        if self.downloads_model_dir.join(&file).is_file() {
            &self.downloads_model_dir
        } else {
            &self.bundled_model_dir
        }
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &self,
        audio: &Path,
        model: &str,
        total_duration_ms: u64,
        on_progress: &mut dyn FnMut(f32),
    ) -> Result<Vec<SubtitleCue>, String> {
        let out_dir = audio
            .parent()
            .ok_or_else(|| "音频路径缺少上级目录".to_string())?;

        let mut child = Command::new(&self.whisper)
            .arg(audio)
            .args(["--model", model])
            .arg("--model_dir")
            .arg(self.model_dir_for(model))
            .args(["--output_format", "srt", "--verbose", "True"])
            .arg("--output_dir")
            .arg(out_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法启动 whisper：{e}"))?;

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if let Some(fraction) = parse_progress_fraction(&line, total_duration_ms) {
                    on_progress(fraction);
                }
            }
        }

        let status = child
            .wait()
            .map_err(|e| format!("等待 whisper 进程失败：{e}"))?;
        if !status.success() {
            return Err(format!("whisper 转写失败（退出码 {:?}）", status.code()));
        }
        on_progress(1.0);

        // whisper writes "<audio stem>.srt" into out_dir.
        let stem = audio
            .file_stem()
            .ok_or_else(|| "音频路径缺少文件名".to_string())?;
        let srt_path = out_dir.join(format!("{}.srt", stem.to_string_lossy()));
        let srt = std::fs::read_to_string(&srt_path)
            .map_err(|e| format!("读取 whisper 输出 SRT 失败：{e}"))?;
        parse_srt(&srt).map_err(|e| format!("解析 whisper 输出 SRT 失败：{e:?}"))
    }
}

/// Turns a whisper verbose stdout line `[mm:ss.fff --> mm:ss.fff] text` into a
/// completion fraction (`end / total`), or `None` for non-progress lines.
pub fn parse_progress_fraction(line: &str, total_duration_ms: u64) -> Option<f32> {
    if total_duration_ms == 0 {
        return None;
    }
    let line = line.trim();
    let inner = line.strip_prefix('[')?;
    let end = inner.find(']')?;
    let (_start, rest) = inner[..end].split_once("-->")?;
    let end_ms = parse_clock(rest.trim())?;
    let fraction = end_ms as f32 / total_duration_ms as f32;
    Some(fraction.clamp(0.0, 1.0))
}

/// Parses a whisper timestamp `MM:SS.fff` or `HH:MM:SS.fff` into milliseconds.
fn parse_clock(s: &str) -> Option<u64> {
    let (hms, frac) = s.split_once('.')?;
    let mut nums = Vec::new();
    for part in hms.split(':') {
        nums.push(part.parse::<u64>().ok()?);
    }
    let (hours, mins, secs) = match nums.as_slice() {
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    if mins >= 60 || secs >= 60 {
        return None;
    }
    let millis: u64 = {
        let padded: String = frac.chars().chain(std::iter::repeat('0')).take(3).collect();
        padded.parse().ok()?
    };
    Some(((hours * 60 + mins) * 60 + secs) * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress_from_real_verbose_line() {
        // Real whisper verbose format: "[00:07.000 --> 00:15.000] text"
        let line = "[00:07.000 --> 00:15.000] This demonstrates the verbose parameter.";
        // 15s of a 30s clip → 0.5.
        assert_eq!(parse_progress_fraction(line, 30_000), Some(0.5));
    }

    #[test]
    fn parses_progress_with_hour_component() {
        let line = "[01:00:00.000 --> 01:00:30.000] later text";
        // 3630s of a 7260s clip → 0.5.
        assert_eq!(parse_progress_fraction(line, 7_260_000), Some(0.5));
    }

    #[test]
    fn clamps_fraction_to_one_when_past_end() {
        let line = "[00:29.000 --> 00:40.000] overruns estimate";
        assert_eq!(parse_progress_fraction(line, 30_000), Some(1.0));
    }

    #[test]
    fn ignores_non_progress_lines() {
        assert_eq!(
            parse_progress_fraction("Detecting language...", 30_000),
            None
        );
        assert_eq!(parse_progress_fraction("", 30_000), None);
    }

    #[test]
    fn none_when_duration_unknown() {
        let line = "[00:07.000 --> 00:15.000] text";
        assert_eq!(parse_progress_fraction(line, 0), None);
    }
}
