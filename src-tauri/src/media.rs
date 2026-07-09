//! ffmpeg-backed `MediaProcessor` (接缝 2 的真实实现).
//!
//! Shelling out to the bundled ffmpeg is a thin wrapper; the parsing of its
//! banner is a pure function tested against a real ffmpeg sample. The end-to-end
//! extract/probe path is verified manually with a short fixture.

use crate::transcribe::MediaProcessor;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `MediaProcessor` implemented by shelling out to the bundled ffmpeg binary.
pub struct FfmpegMediaProcessor {
    ffmpeg: PathBuf,
    /// Directory of bundled fonts made available to the subtitles filter so
    /// burn-in renders CJK glyphs. `None` falls back to system fonts.
    fonts_dir: Option<PathBuf>,
}

impl FfmpegMediaProcessor {
    pub fn new(ffmpeg: PathBuf) -> Self {
        Self {
            ffmpeg,
            fonts_dir: None,
        }
    }

    /// Points the subtitles filter at a bundled fonts directory (CJK burn-in).
    pub fn with_fonts_dir(mut self, fonts_dir: PathBuf) -> Self {
        self.fonts_dir = Some(fonts_dir);
        self
    }
}

impl MediaProcessor for FfmpegMediaProcessor {
    fn probe_duration_ms(&self, video: &Path) -> Result<u64, String> {
        // `ffmpeg -i <file>` prints the container banner (incl. Duration) on
        // stderr and exits non-zero because no output was requested; that's
        // expected, so we parse stderr regardless of exit status.
        let output = Command::new(&self.ffmpeg)
            .arg("-i")
            .arg(video)
            .output()
            .map_err(|e| format!("无法启动 ffmpeg：{e}"))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        parse_duration_ms(&stderr).ok_or_else(|| "无法从 ffmpeg 输出解析视频时长".to_string())
    }

    fn extract_audio(&self, video: &Path, out_wav: &Path) -> Result<(), String> {
        // 16 kHz mono PCM — what Whisper expects.
        let status = Command::new(&self.ffmpeg)
            .arg("-y")
            .arg("-i")
            .arg(video)
            .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "wav"])
            .arg(out_wav)
            .status()
            .map_err(|e| format!("无法启动 ffmpeg：{e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("ffmpeg 抽取音频失败（退出码 {:?}）", status.code()))
        }
    }

    fn mux_subtitle(
        &self,
        video: &Path,
        subtitle: &Path,
        out_video: &Path,
    ) -> Result<(), String> {
        // Copy A/V streams untouched and add the SRT as a switchable soft
        // subtitle track. The subtitle codec depends on the output container
        // (MP4/MOV want `mov_text`; Matroska wants `srt`), so a bad pairing
        // doesn't silently fail on MKV inputs.
        let codec = soft_subtitle_codec(out_video);
        let status = Command::new(&self.ffmpeg)
            .arg("-y")
            .arg("-i")
            .arg(video)
            .arg("-i")
            .arg(subtitle)
            .args(["-map", "0", "-map", "1", "-c", "copy", "-c:s", codec])
            .args(["-disposition:s:0", "default"])
            .arg(out_video)
            .status()
            .map_err(|e| format!("无法启动 ffmpeg：{e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "ffmpeg 封装软字幕失败（退出码 {:?}）",
                status.code()
            ))
        }
    }

    fn burn_in(
        &self,
        video: &Path,
        subtitle: &Path,
        out_video: &Path,
        total_duration_ms: u64,
        on_progress: &mut dyn FnMut(f32),
    ) -> Result<(), String> {
        // Render the SRT into the picture with the `subtitles` filter. A bundled
        // fonts dir + a forced CJK-capable family keeps 中文/日文 from tofu.
        let mut filter = format!("subtitles={}", escape_filter_path(subtitle));
        if let Some(dir) = &self.fonts_dir {
            filter.push_str(&format!(":fontsdir={}", escape_filter_path(dir)));
        }
        filter.push_str(":force_style='FontName=Noto Sans CJK SC'");

        // `-progress pipe:1` prints machine-readable `out_time_ms=…` lines to
        // stdout; we translate those into a completion fraction.
        let mut child = Command::new(&self.ffmpeg)
            .arg("-y")
            .arg("-i")
            .arg(video)
            .args(["-vf", &filter])
            .args(["-progress", "pipe:1", "-nostats"])
            .arg(out_video)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("无法启动 ffmpeg：{e}"))?;

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
            .map_err(|e| format!("等待 ffmpeg 进程失败：{e}"))?;
        if status.success() {
            on_progress(1.0);
            Ok(())
        } else {
            Err(format!("ffmpeg 烧录字幕失败（退出码 {:?}）", status.code()))
        }
    }
}

/// The ffmpeg soft-subtitle codec for an output container: MP4/MOV need
/// `mov_text`; everything else (Matroska/AVI) takes SRT (`srt`). Chosen by the
/// output file's extension so a soft-subtitle mux doesn't fail on MKV inputs.
fn soft_subtitle_codec(out_video: &Path) -> &'static str {
    match out_video
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp4") | Some("mov") | Some("m4v") => "mov_text",
        _ => "srt",
    }
}

/// Escapes a path for use inside an ffmpeg filtergraph argument, where `\`,
/// `:`, and `'` are special. Windows drive colons and spaces are the usual
/// offenders; on macOS this mostly guards `:` and `'` in file names.
fn escape_filter_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ':' => out.push_str("\\:"),
            '\'' => out.push_str("\\'"),
            _ => out.push(ch),
        }
    }
    out
}

/// Turns an ffmpeg `-progress` line into a completion fraction. Recognizes
/// `out_time_ms=<micros>` (ffmpeg reports microseconds despite the name) and
/// `out_time_us=<micros>`; returns `None` for other lines or unknown duration.
pub fn parse_progress_fraction(line: &str, total_duration_ms: u64) -> Option<f32> {
    if total_duration_ms == 0 {
        return None;
    }
    let value = line
        .strip_prefix("out_time_ms=")
        .or_else(|| line.strip_prefix("out_time_us="))?;
    let micros: u64 = value.trim().parse().ok()?;
    let done_ms = micros / 1000;
    let fraction = done_ms as f32 / total_duration_ms as f32;
    Some(fraction.clamp(0.0, 1.0))
}

/// Extracts the media duration in milliseconds from an ffmpeg banner line of
/// the form `Duration: HH:MM:SS.cc, start: ...`. Returns `None` if absent.
pub fn parse_duration_ms(ffmpeg_stderr: &str) -> Option<u64> {
    let idx = ffmpeg_stderr.find("Duration:")?;
    let after = &ffmpeg_stderr[idx + "Duration:".len()..];
    let value = after.trim_start();
    let end = value.find(',').unwrap_or(value.len());
    let stamp = value[..end].trim();
    if stamp.starts_with("N/A") {
        return None;
    }
    parse_hhmmss_cc(stamp)
}

/// Parses `HH:MM:SS.cc` (centiseconds) into milliseconds.
fn parse_hhmmss_cc(stamp: &str) -> Option<u64> {
    let (hms, frac) = stamp.split_once('.')?;
    let mut parts = hms.split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let mins: u64 = parts.next()?.parse().ok()?;
    let secs: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    // ffmpeg prints two fractional digits (centiseconds); be lenient anyway.
    let frac_ms: u64 = match frac.len() {
        2 => frac.parse::<u64>().ok()? * 10,
        3 => frac.parse::<u64>().ok()?,
        _ => {
            let padded: String = frac.chars().chain(std::iter::repeat('0')).take(3).collect();
            padded.parse::<u64>().ok()?
        }
    };
    Some(((hours * 60 + mins) * 60 + secs) * 1000 + frac_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real banner captured from the bundled ffmpeg 6.0.
    const REAL_BANNER: &str =
        "  Duration: 00:00:03.50, start: 0.000000, bitrate: 84 kb/s\n  Stream #0:0";

    #[test]
    fn parses_real_ffmpeg_banner() {
        assert_eq!(parse_duration_ms(REAL_BANNER), Some(3500));
    }

    #[test]
    fn parses_multi_component_duration() {
        let banner = "  Duration: 01:02:03.25, start: 0.0";
        assert_eq!(parse_duration_ms(banner), Some(3_723_250));
    }

    #[test]
    fn returns_none_when_duration_absent() {
        assert_eq!(parse_duration_ms("no duration here"), None);
    }

    #[test]
    fn returns_none_for_na_duration() {
        assert_eq!(parse_duration_ms("  Duration: N/A, start: 0.0"), None);
    }

    #[test]
    fn parses_progress_from_out_time_ms() {
        // ffmpeg's `out_time_ms` is actually microseconds: 15_000_000µs = 15s.
        assert_eq!(parse_progress_fraction("out_time_ms=15000000", 30_000), Some(0.5));
    }

    #[test]
    fn parses_progress_from_out_time_us() {
        assert_eq!(parse_progress_fraction("out_time_us=7500000", 30_000), Some(0.25));
    }

    #[test]
    fn progress_clamps_past_end_and_ignores_other_lines() {
        assert_eq!(parse_progress_fraction("out_time_ms=40000000", 30_000), Some(1.0));
        assert_eq!(parse_progress_fraction("frame=123", 30_000), None);
        assert_eq!(parse_progress_fraction("progress=continue", 30_000), None);
    }

    #[test]
    fn progress_none_when_duration_unknown() {
        assert_eq!(parse_progress_fraction("out_time_ms=1000000", 0), None);
    }

    #[test]
    fn escapes_filter_special_characters() {
        assert_eq!(
            escape_filter_path(Path::new("/tmp/a:b'c.srt")),
            "/tmp/a\\:b\\'c.srt"
        );
        assert_eq!(
            escape_filter_path(Path::new("/plain/path.srt")),
            "/plain/path.srt"
        );
    }

    #[test]
    fn soft_subtitle_codec_matches_the_container() {
        assert_eq!(soft_subtitle_codec(Path::new("out.mp4")), "mov_text");
        assert_eq!(soft_subtitle_codec(Path::new("out.MOV")), "mov_text");
        assert_eq!(soft_subtitle_codec(Path::new("out.mkv")), "srt");
        assert_eq!(soft_subtitle_codec(Path::new("out.avi")), "srt");
    }
}
