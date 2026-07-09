//! Subtitle domain logic (接缝 1) — pure, no IO.
//!
//! Owns the in-memory subtitle representation and SRT serialization/parsing.
//! Validation (译文 vs 原始) and Prompt generation land here in later tickets.

/// One timed subtitle segment. `index` is the 1-based SRT sequence number;
/// `text` may span multiple lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtitleCue {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Serializes cues to SRT text. Blocks are separated by a blank line and the
/// output ends with a trailing newline.
pub fn to_srt(cues: &[SubtitleCue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&cue.index.to_string());
        out.push('\n');
        out.push_str(&format_timestamp(cue.start_ms));
        out.push_str(" --> ");
        out.push_str(&format_timestamp(cue.end_ms));
        out.push('\n');
        out.push_str(&cue.text);
        out.push('\n');
    }
    out
}

/// Formats milliseconds as an SRT timestamp `HH:MM:SS,mmm`.
fn format_timestamp(total_ms: u64) -> String {
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let secs = total_secs % 60;
    let mins = (total_secs / 60) % 60;
    let hours = total_secs / 3600;
    format!("{hours:02}:{mins:02}:{secs:02},{ms:03}")
}

/// Why an SRT document could not be parsed. `block` is the 1-based ordinal of
/// the offending block within the file (not the SRT index), so callers can
/// point the user at the exact segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SrtParseError {
    InvalidIndex { block: usize, found: String },
    MissingTiming { block: usize },
    InvalidTiming { block: usize, found: String },
    MissingText { block: usize },
}

/// Parses SRT text into cues. Tolerant of CRLF line endings, a leading UTF-8
/// BOM, and blank lines between blocks; strict about the index → timing → text
/// block shape so malformed input is reported with its block ordinal.
pub fn parse_srt(input: &str) -> Result<Vec<SubtitleCue>, SrtParseError> {
    let normalized = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut lines = normalized.lines().peekable();
    let mut cues = Vec::new();
    let mut block = 0usize;

    loop {
        // Skip blank lines separating blocks (and any leading/trailing ones).
        while lines.peek().is_some_and(|l| l.trim().is_empty()) {
            lines.next();
        }
        let Some(index_line) = lines.next() else {
            break;
        };
        block += 1;

        let index = index_line
            .trim()
            .parse::<u32>()
            .map_err(|_| SrtParseError::InvalidIndex {
                block,
                found: index_line.trim().to_string(),
            })?;

        let timing_line = lines.next().ok_or(SrtParseError::MissingTiming { block })?;
        let (start_ms, end_ms) = parse_timing(timing_line.trim(), block)?;

        let mut text_lines = Vec::new();
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                break;
            }
            text_lines.push(lines.next().unwrap().trim_end_matches('\r'));
        }
        if text_lines.is_empty() {
            return Err(SrtParseError::MissingText { block });
        }

        cues.push(SubtitleCue {
            index,
            start_ms,
            end_ms,
            text: text_lines.join("\n"),
        });
    }

    Ok(cues)
}

/// Parses an SRT timing line `HH:MM:SS,mmm --> HH:MM:SS,mmm` into start/end ms.
fn parse_timing(line: &str, block: usize) -> Result<(u64, u64), SrtParseError> {
    let invalid = || SrtParseError::InvalidTiming {
        block,
        found: line.to_string(),
    };
    let (start, end) = line.split_once("-->").ok_or_else(invalid)?;
    let start_ms = parse_timestamp(start.trim()).ok_or_else(invalid)?;
    let end_ms = parse_timestamp(end.trim()).ok_or_else(invalid)?;
    Ok((start_ms, end_ms))
}

/// Parses a single SRT timestamp `HH:MM:SS,mmm` into milliseconds.
fn parse_timestamp(s: &str) -> Option<u64> {
    let (hms, ms) = s.split_once(',')?;
    let mut parts = hms.split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let mins: u64 = parts.next()?.parse().ok()?;
    let secs: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || mins >= 60 || secs >= 60 {
        return None;
    }
    let millis: u64 = ms.parse().ok()?;
    if ms.len() != 3 {
        return None;
    }
    Some(((hours * 60 + mins) * 60 + secs) * 1000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(index: u32, start_ms: u64, end_ms: u64, text: &str) -> SubtitleCue {
        SubtitleCue {
            index,
            start_ms,
            end_ms,
            text: text.to_string(),
        }
    }

    #[test]
    fn serializes_single_cue() {
        let cues = vec![cue(1, 0, 2000, "Hello world")];
        assert_eq!(
            to_srt(&cues),
            "1\n00:00:00,000 --> 00:00:02,000\nHello world\n"
        );
    }

    #[test]
    fn serializes_multiple_cues_separated_by_blank_line() {
        let cues = vec![
            cue(1, 0, 2000, "First"),
            cue(2, 2000, 4500, "Second line\nwrapped"),
        ];
        let expected = "1\n00:00:00,000 --> 00:00:02,000\nFirst\n\n\
2\n00:00:02,000 --> 00:00:04,500\nSecond line\nwrapped\n";
        assert_eq!(to_srt(&cues), expected);
    }

    #[test]
    fn formats_hours_minutes_and_millis() {
        let cues = vec![cue(1, 3_661_500, 3_723_010, "late")];
        assert_eq!(to_srt(&cues), "1\n01:01:01,500 --> 01:02:03,010\nlate\n");
    }

    #[test]
    fn parses_single_cue() {
        let input = "1\n00:00:00,000 --> 00:00:02,000\nHello world\n";
        assert_eq!(parse_srt(input), Ok(vec![cue(1, 0, 2000, "Hello world")]));
    }

    #[test]
    fn parses_multiple_cues_with_multiline_text() {
        let input = "1\n00:00:00,000 --> 00:00:02,000\nFirst\n\n\
2\n00:00:02,000 --> 00:00:04,500\nSecond line\nwrapped\n";
        assert_eq!(
            parse_srt(input),
            Ok(vec![
                cue(1, 0, 2000, "First"),
                cue(2, 2000, 4500, "Second line\nwrapped"),
            ])
        );
    }

    #[test]
    fn tolerates_crlf_and_leading_bom() {
        let input = "\u{feff}1\r\n00:00:00,000 --> 00:00:01,000\r\nHi\r\n";
        assert_eq!(parse_srt(input), Ok(vec![cue(1, 0, 1000, "Hi")]));
    }

    #[test]
    fn tolerates_extra_blank_lines_between_blocks() {
        let input = "1\n00:00:00,000 --> 00:00:01,000\nA\n\n\n\n\
2\n00:00:01,000 --> 00:00:02,000\nB\n";
        assert_eq!(
            parse_srt(input),
            Ok(vec![cue(1, 0, 1000, "A"), cue(2, 1000, 2000, "B")])
        );
    }

    #[test]
    fn round_trips_through_serialize_and_parse() {
        let cues = vec![
            cue(1, 0, 2000, "First"),
            cue(2, 2000, 4500, "Second line\nwrapped"),
        ];
        assert_eq!(parse_srt(&to_srt(&cues)), Ok(cues));
    }

    #[test]
    fn empty_input_parses_to_no_cues() {
        assert_eq!(parse_srt(""), Ok(vec![]));
        assert_eq!(parse_srt("\n\n  \n"), Ok(vec![]));
    }

    #[test]
    fn rejects_non_numeric_index() {
        let input = "x\n00:00:00,000 --> 00:00:01,000\nA\n";
        assert_eq!(
            parse_srt(input),
            Err(SrtParseError::InvalidIndex {
                block: 1,
                found: "x".to_string()
            })
        );
    }

    #[test]
    fn rejects_missing_timing_line() {
        let input = "1\nHello with no arrow\n";
        assert_eq!(
            parse_srt(input),
            Err(SrtParseError::InvalidTiming {
                block: 1,
                found: "Hello with no arrow".to_string()
            })
        );
    }

    #[test]
    fn rejects_block_without_text() {
        let input = "1\n00:00:00,000 --> 00:00:01,000\n";
        assert_eq!(
            parse_srt(input),
            Err(SrtParseError::MissingText { block: 1 })
        );
    }

    #[test]
    fn reports_block_ordinal_of_second_bad_block() {
        let input = "1\n00:00:00,000 --> 00:00:01,000\nGood\n\n\
2\nbad timing here\nText\n";
        assert_eq!(
            parse_srt(input),
            Err(SrtParseError::InvalidTiming {
                block: 2,
                found: "bad timing here".to_string()
            })
        );
    }
}
