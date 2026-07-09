//! ffmpeg-backed `MediaProcessor` (接缝 2 的真实实现).
//!
//! Shelling out to the bundled ffmpeg is a thin wrapper; the parsing of its
//! banner is a pure function tested against a real ffmpeg sample. The end-to-end
//! extract/probe path is verified manually with a short fixture.

use crate::transcribe::MediaProcessor;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `MediaProcessor` implemented by shelling out to the bundled ffmpeg binary.
pub struct FfmpegMediaProcessor {
    ffmpeg: PathBuf,
}

impl FfmpegMediaProcessor {
    pub fn new(ffmpeg: PathBuf) -> Self {
        Self { ffmpeg }
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
}
