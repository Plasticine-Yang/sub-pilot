//! OpenAI whisper `Transcriber` (ADR-0005) — subprocess-backed real impl.
//!
//! Runs the bundled Python whisper as a child process, streaming its verbose
//! stdout to report progress, and reads back the `.srt` it writes. The verbose
//! line parser is a pure function tested against whisper's real output shape;
//! the subprocess wiring itself is covered by manual end-to-end runs.

use crate::subtitle::parse_srt;
use crate::subtitle::SubtitleCue;
use crate::transcribe::Transcriber;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `Transcriber` implemented by shelling out to the bundled OpenAI whisper CLI.
pub struct WhisperTranscriber {
    /// Path to the whisper executable (bundled Python entry point).
    whisper: PathBuf,
    /// Path to the bundled ffmpeg binary used by OpenAI whisper internally.
    ffmpeg: PathBuf,
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
        ffmpeg: PathBuf,
    ) -> Self {
        Self {
            whisper,
            ffmpeg,
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
        let audio_dir = audio
            .parent()
            .ok_or_else(|| "音频路径缺少上级目录".to_string())?;
        let out_dir = audio_dir.join("whisper-output");
        reset_output_dir(&out_dir)?;
        let child_path = path_with_bundled_ffmpeg(&self.ffmpeg)?;

        let mut child = Command::new(&self.whisper)
            .arg(audio)
            .args(["--model", model])
            .arg("--model_dir")
            .arg(self.model_dir_for(model))
            .args(["--output_format", "srt", "--verbose", "True"])
            .arg("--output_dir")
            .arg(&out_dir)
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUNBUFFERED", "1")
            .env("PATH", child_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("无法启动 whisper：{e}"))?;

        let stderr = child.stderr.take().map(|stderr| {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let mut reader = BufReader::new(stderr);
                let _ = reader.read_to_string(&mut buf);
                buf
            })
        });

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
        let stderr = stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        if !status.success() {
            let detail = stderr.trim();
            if detail.is_empty() {
                return Err(format!("whisper 转写失败（退出码 {:?}）", status.code()));
            }
            return Err(format!(
                "whisper 转写失败（退出码 {:?}）：{detail}",
                status.code()
            ));
        }
        on_progress(1.0);

        let srt_path =
            whisper_output_srt_path(audio, &out_dir).map_err(|e| with_stderr(e, &stderr))?;
        let srt = std::fs::read_to_string(&srt_path)
            .map_err(|e| with_stderr(format!("读取 whisper 输出 SRT 失败：{e}"), &stderr))?;
        let cues = parse_srt(&srt).map_err(|e| format!("解析 whisper 输出 SRT 失败：{e:?}"))?;
        let _ = std::fs::remove_dir_all(&out_dir);
        Ok(cues)
    }
}

fn reset_output_dir(out_dir: &Path) -> Result<(), String> {
    if out_dir.is_dir() {
        std::fs::remove_dir_all(out_dir).map_err(|e| format!("清理 whisper 输出目录失败：{e}"))?;
    } else if out_dir.exists() {
        std::fs::remove_file(out_dir).map_err(|e| format!("清理 whisper 输出文件失败：{e}"))?;
    }
    std::fs::create_dir_all(out_dir).map_err(|e| format!("创建 whisper 输出目录失败：{e}"))
}

fn path_with_bundled_ffmpeg(ffmpeg: &Path) -> Result<std::ffi::OsString, String> {
    let ffmpeg_dir = ffmpeg
        .parent()
        .ok_or_else(|| "ffmpeg 路径缺少上级目录".to_string())?;
    let mut paths = vec![ffmpeg_dir.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).map_err(|e| format!("构造 whisper PATH 失败：{e}"))
}

fn whisper_output_srt_path(audio: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    // OpenAI Whisper normally writes "<audio stem>.srt". Some bundled runtimes
    // may choose a different basename, so fall back to the sole SRT in the
    // fresh output directory before giving up.
    let stem = audio
        .file_stem()
        .ok_or_else(|| "音频路径缺少文件名".to_string())?;
    let expected = out_dir.join(format!("{}.srt", stem.to_string_lossy()));
    if expected.is_file() {
        return Ok(expected);
    }

    let mut candidates = Vec::new();
    for entry in
        std::fs::read_dir(out_dir).map_err(|e| format!("读取 whisper 输出目录失败：{e}"))?
    {
        let path = entry
            .map_err(|e| format!("读取 whisper 输出目录项失败：{e}"))?
            .path();
        let is_srt = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"));
        if path.is_file() && is_srt {
            candidates.push(path);
        }
    }
    candidates.sort();

    match candidates.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(format!(
            "读取 whisper 输出 SRT 失败：找不到 SRT 文件（预期路径：{}；目录内容：{}）",
            expected.display(),
            describe_dir(out_dir)
        )),
        _ => Err(format!(
            "读取 whisper 输出 SRT 失败：找到多个 SRT 文件，无法判断使用哪一个：{}",
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn describe_dir(dir: &Path) -> String {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return "<无法读取目录>".to_string();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        names.push(entry.file_name().to_string_lossy().to_string());
    }
    names.sort();
    if names.is_empty() {
        "<空目录>".to_string()
    } else {
        names.join(", ")
    }
}

fn with_stderr(message: String, stderr: &str) -> String {
    let detail = stderr.trim();
    if detail.is_empty() {
        message
    } else {
        format!("{message}；whisper stderr：{detail}")
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

    // End-to-end guard for the "stuck at 0%" bug: with the whisper runtime
    // provisioned, spawning the real CLI against a real 16 kHz wav must emit a
    // verbose progress line (fraction > 0) and produce an SRT. Ignored by
    // default because it shells out to the bundled model + venv; run with:
    //   cargo test --lib whisper::tests::real_whisper_emits_progress_and_srt \
    //     -- --ignored --nocapture
    #[test]
    #[ignore = "shells out to the whisper venv + base model; run with --ignored"]
    fn real_whisper_emits_progress_and_srt() {
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf();
        let fixture = repo.join("resources/whisper/fixture_audio.wav");
        assert!(
            fixture.is_file(),
            "fixture wav missing at {fixture:?}; extract one with ffmpeg -i demo.mp4 -vn -ac 1 -ar 16000"
        );
        // Copy into a tempdir so the test stays hermetic and never dirties the
        // tracked resources/ tree.
        let work = tempfile::tempdir().unwrap();
        let audio = work.path().join("fixture_audio.wav");
        std::fs::copy(&fixture, &audio).unwrap();

        let transcriber = WhisperTranscriber::new(
            repo.join("resources/whisper/whisper"),
            repo.join("resources/models"),
            repo.join("resources/models"),
            repo.join("resources/ffmpeg/ffmpeg"),
        );
        let mut fractions = Vec::new();
        let cues = transcriber
            .transcribe(&audio, "base", 7300, &mut |f| fractions.push(f))
            .expect("real whisper should transcribe once provisioned");

        // The freeze symptom was "no progress ever arrives". A mid-run fraction
        // in (0,1) proves verbose lines were parsed — not just the terminal 1.0.
        assert!(
            fractions.iter().any(|&f| f > 0.0 && f < 1.0),
            "expected a mid-run progress fraction in (0,1) (the 0% freeze symptom), got {fractions:?}"
        );
        assert!(!cues.is_empty(), "expected at least one cue");
    }

    #[cfg(unix)]
    #[test]
    fn transcribe_accepts_srt_written_under_unexpected_runtime_name() {
        use std::os::unix::fs::PermissionsExt;

        let work = tempfile::tempdir().unwrap();
        let audio = work.path().join("audio.wav");
        std::fs::write(&audio, b"fake wav").unwrap();

        let fake_whisper = work.path().join("fake-whisper");
        std::fs::write(
            &fake_whisper,
            r#"#!/usr/bin/env bash
set -euo pipefail
out_dir=""
while (($#)); do
  if [[ "$1" == "--output_dir" ]]; then
    shift
    out_dir="$1"
    break
  fi
  shift
done
mkdir -p "$out_dir"
cat > "$out_dir/windows-runtime-output.srt" <<'SRT'
1
00:00:00,000 --> 00:00:01,000
hello from fake whisper

SRT
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&fake_whisper).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_whisper, perms).unwrap();

        let transcriber = WhisperTranscriber::new(
            fake_whisper,
            work.path().into(),
            work.path().into(),
            work.path().join("ffmpeg"),
        );
        let cues = transcriber
            .transcribe(&audio, "base", 1_000, &mut |_| {})
            .expect("should read the SRT actually produced by the runtime");

        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "hello from fake whisper");
    }

    #[cfg(unix)]
    #[test]
    fn transcribe_prepends_bundled_ffmpeg_dir_to_child_path() {
        use std::os::unix::fs::PermissionsExt;

        let work = tempfile::tempdir().unwrap();
        let audio = work.path().join("audio.wav");
        std::fs::write(&audio, b"fake wav").unwrap();

        let ffmpeg_dir = work.path().join("ffmpeg-bin");
        std::fs::create_dir_all(&ffmpeg_dir).unwrap();
        let fake_ffmpeg = ffmpeg_dir.join("ffmpeg");
        std::fs::write(&fake_ffmpeg, "#!/bin/sh\nexit 0\n").unwrap();
        let mut ffmpeg_perms = std::fs::metadata(&fake_ffmpeg).unwrap().permissions();
        ffmpeg_perms.set_mode(0o755);
        std::fs::set_permissions(&fake_ffmpeg, ffmpeg_perms).unwrap();

        let fake_whisper = work.path().join("fake-whisper");
        std::fs::write(
            &fake_whisper,
            r#"#!/bin/sh
set -eu

script_dir=$(dirname "$0")
ffmpeg_dir="$script_dir/ffmpeg-bin"
first_path=${PATH%%:*}
if [ "$first_path" != "$ffmpeg_dir" ]; then
  printf '%s\n' "Traceback (most recent call last):" \
    "FileNotFoundError: [Errno 2] No such file or directory: 'ffmpeg'" >&2
  exit 0
fi
ffmpeg --self-test >/dev/null

out_dir=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--output_dir" ]; then
    shift
    out_dir="$1"
    break
  fi
  shift
done
mkdir -p "$out_dir"
cat > "$out_dir/audio.srt" <<'SRT'
1
00:00:00,000 --> 00:00:01,000
ffmpeg was available to whisper

SRT
"#,
        )
        .unwrap();
        let mut whisper_perms = std::fs::metadata(&fake_whisper).unwrap().permissions();
        whisper_perms.set_mode(0o755);
        std::fs::set_permissions(&fake_whisper, whisper_perms).unwrap();

        let transcriber = WhisperTranscriber::new(
            fake_whisper,
            work.path().into(),
            work.path().into(),
            fake_ffmpeg,
        );
        let cues = transcriber
            .transcribe(&audio, "base", 1_000, &mut |_| {})
            .expect("bundled ffmpeg directory should be visible to whisper");

        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "ffmpeg was available to whisper");
    }

    #[cfg(unix)]
    #[test]
    fn transcribe_streams_python_progress_before_child_exits() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::mpsc;
        use std::time::Duration;

        let work = tempfile::tempdir().unwrap();
        let audio = work.path().join("audio.wav");
        std::fs::write(&audio, b"fake wav").unwrap();

        let fake_whisper = work.path().join("fake-whisper");
        std::fs::write(
            &fake_whisper,
            r#"#!/usr/bin/env python3
import pathlib
import sys
import time

out_dir = pathlib.Path(sys.argv[sys.argv.index("--output_dir") + 1])
out_dir.mkdir(parents=True, exist_ok=True)
print("[00:00.000 --> 00:01.000] buffered progress")
time.sleep(2.0)
(out_dir / "audio.srt").write_text(
    "1\n00:00:00,000 --> 00:00:01,000\nstreamed progress\n\n",
    encoding="utf-8",
)
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&fake_whisper).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_whisper, perms).unwrap();

        let transcriber = WhisperTranscriber::new(
            fake_whisper,
            work.path().into(),
            work.path().into(),
            work.path().join("ffmpeg"),
        );
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            transcriber.transcribe(&audio, "base", 2_000, &mut |fraction| {
                tx.send(fraction).unwrap();
            })
        });

        let first_progress = rx.recv_timeout(Duration::from_millis(1500));
        let result = handle.join().unwrap();
        assert!(result.is_ok(), "fake whisper should complete: {result:?}");
        assert_eq!(
            first_progress,
            Ok(0.5),
            "progress should be relayed while the whisper child is still running"
        );
    }

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
