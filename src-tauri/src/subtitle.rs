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

/// Builds the translation Prompt (接缝 1) users paste into any external AI.
/// Bakes in the segment count and index range so the AI is anchored to the
/// original subtitle's structure — the invariants Validation later enforces
/// (段数一致、编号连续、时间轴不动、UTF-8 SRT).
pub fn build_translation_prompt(cues: &[SubtitleCue]) -> String {
    let count = cues.len();
    format!(
        "你是专业的字幕翻译，请把下面的 SRT 字幕翻译成简体中文。\n\
\n\
严格遵守以下规则：\n\
1. 保持 SRT 结构：每段为「序号 / 时间轴 / 文本」三行，段间空一行。\n\
2. 段数必须与原文一致，共 {count} 段，不要合并、拆分或增删任何一段。\n\
3. 序号保持从 1 到 {count} 连续，不要改动。\n\
4. 时间轴（含 --> 的一行）原样保留，不要改动任何时间戳。\n\
5. 只翻译文本行，每段译文与原文一一对应。\n\
6. 输出为合法的 UTF-8 SRT 纯文本，不要添加解释说明、代码块标记或其它多余内容。\n"
    )
}

/// A hard validation error (ADR-0004) blocking export of a Translated Subtitle.
/// Each variant carries enough to point the user at the offending segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// Raw bytes were not valid UTF-8. Not locatable to a segment.
    NotUtf8,
    /// The SRT itself is malformed. `block` is the 1-based block ordinal.
    Syntax { block: usize },
    /// Segment count differs from the original. `expected`/`found` are counts.
    SegmentCountMismatch { expected: usize, found: usize },
    /// A block's SRT index is not the expected consecutive number.
    IndexMismatch {
        block: usize,
        expected: u32,
        found: u32,
    },
}

impl ValidationError {
    /// The 1-based segment number to highlight in the UI, if the error is
    /// locatable. `NotUtf8` is a whole-file property, so it has none; a count
    /// mismatch points at the first segment that goes missing.
    pub fn segment(&self) -> Option<usize> {
        match self {
            ValidationError::NotUtf8 => None,
            ValidationError::Syntax { block } => Some(*block),
            ValidationError::IndexMismatch { block, .. } => Some(*block),
            ValidationError::SegmentCountMismatch { expected, found } => {
                Some(found.min(expected) + 1)
            }
        }
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::NotUtf8 => write!(f, "译文字幕不是合法的 UTF-8 编码"),
            ValidationError::Syntax { block } => {
                write!(f, "第 {block} 段 SRT 语法非法")
            }
            ValidationError::SegmentCountMismatch { expected, found } => write!(
                f,
                "译文段数（{found}）与原始字幕段数（{expected}）不一致"
            ),
            ValidationError::IndexMismatch {
                block,
                expected,
                found,
            } => write!(f, "第 {block} 段编号应为 {expected}，实际为 {found}"),
        }
    }
}

impl std::error::Error for ValidationError {}

/// Validates a Translated Subtitle (raw bytes) against the original cues and,
/// on success, returns cues that adopt the original timeline while taking text
/// from the translation (ADR-0004: 时间轴以原始字幕为准，译文只贡献文本).
///
/// Hard errors, checked in order of coarseness: non-UTF-8 → SRT syntax → 段数
/// 不一致 → 编号不连续. Timestamps in the translation are intentionally ignored.
pub fn validate_translation(
    original: &[SubtitleCue],
    translated_bytes: &[u8],
) -> Result<Vec<SubtitleCue>, ValidationError> {
    let text = std::str::from_utf8(translated_bytes).map_err(|_| ValidationError::NotUtf8)?;
    let translated = parse_srt(text).map_err(|e| ValidationError::Syntax {
        block: srt_error_block(&e),
    })?;

    if translated.len() != original.len() {
        return Err(ValidationError::SegmentCountMismatch {
            expected: original.len(),
            found: translated.len(),
        });
    }

    let mut merged = Vec::with_capacity(original.len());
    for (i, (orig, trans)) in original.iter().zip(&translated).enumerate() {
        let expected_index = (i + 1) as u32;
        if trans.index != expected_index {
            return Err(ValidationError::IndexMismatch {
                block: i + 1,
                expected: expected_index,
                found: trans.index,
            });
        }
        merged.push(SubtitleCue {
            index: orig.index,
            start_ms: orig.start_ms,
            end_ms: orig.end_ms,
            text: trans.text.clone(),
        });
    }
    Ok(merged)
}

/// The 1-based block ordinal an `SrtParseError` points at.
fn srt_error_block(err: &SrtParseError) -> usize {
    match err {
        SrtParseError::InvalidIndex { block, .. }
        | SrtParseError::MissingTiming { block }
        | SrtParseError::InvalidTiming { block, .. }
        | SrtParseError::MissingText { block } => *block,
    }
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

    // --- build_translation_prompt (接缝 1，纯函数) ---------------------------

    #[test]
    fn prompt_matches_expected_text_for_two_cues() {
        let cues = vec![cue(1, 0, 2000, "First"), cue(2, 2000, 4500, "Second")];
        let expected = "你是专业的字幕翻译，请把下面的 SRT 字幕翻译成简体中文。\n\
\n\
严格遵守以下规则：\n\
1. 保持 SRT 结构：每段为「序号 / 时间轴 / 文本」三行，段间空一行。\n\
2. 段数必须与原文一致，共 2 段，不要合并、拆分或增删任何一段。\n\
3. 序号保持从 1 到 2 连续，不要改动。\n\
4. 时间轴（含 --> 的一行）原样保留，不要改动任何时间戳。\n\
5. 只翻译文本行，每段译文与原文一一对应。\n\
6. 输出为合法的 UTF-8 SRT 纯文本，不要添加解释说明、代码块标记或其它多余内容。\n";
        assert_eq!(build_translation_prompt(&cues), expected);
    }

    #[test]
    fn prompt_reflects_the_segment_count() {
        let five: Vec<SubtitleCue> = (1..=5).map(|i| cue(i, 0, 1000, "x")).collect();
        let text = build_translation_prompt(&five);
        assert!(text.contains("共 5 段"), "got: {text}");
        assert!(text.contains("从 1 到 5 连续"), "got: {text}");
    }

    #[test]
    fn prompt_handles_empty_cues() {
        let text = build_translation_prompt(&[]);
        assert!(text.contains("共 0 段"), "got: {text}");
    }

    // --- validate_translation (接缝 1，纯函数) ------------------------------

    fn original() -> Vec<SubtitleCue> {
        vec![cue(1, 0, 1000, "Hello"), cue(2, 1000, 2000, "World")]
    }

    #[test]
    fn valid_translation_adopts_original_timeline_and_translated_text() {
        // Translated deliberately carries different timestamps; per ADR-0004 the
        // original timeline wins and only the text is taken from the translation.
        let translated = "1\n00:09:09,900 --> 00:09:10,900\n你好\n\n\
2\n00:00:05,000 --> 00:00:06,000\n世界\n";
        assert_eq!(
            validate_translation(&original(), translated.as_bytes()),
            Ok(vec![cue(1, 0, 1000, "你好"), cue(2, 1000, 2000, "世界")])
        );
    }

    #[test]
    fn rejects_missing_segment_as_count_mismatch() {
        let translated = "1\n00:00:00,000 --> 00:00:01,000\n你好\n";
        assert_eq!(
            validate_translation(&original(), translated.as_bytes()),
            Err(ValidationError::SegmentCountMismatch {
                expected: 2,
                found: 1
            })
        );
    }

    #[test]
    fn rejects_extra_segment_as_count_mismatch() {
        let translated = "1\n00:00:00,000 --> 00:00:01,000\n你好\n\n\
2\n00:00:01,000 --> 00:00:02,000\n世界\n\n\
3\n00:00:02,000 --> 00:00:03,000\n多余\n";
        assert_eq!(
            validate_translation(&original(), translated.as_bytes()),
            Err(ValidationError::SegmentCountMismatch {
                expected: 2,
                found: 3
            })
        );
    }

    #[test]
    fn rejects_discontinuous_index_and_locates_the_block() {
        // Correct count, but the second block is numbered 3 instead of 2.
        let translated = "1\n00:00:00,000 --> 00:00:01,000\n你好\n\n\
3\n00:00:01,000 --> 00:00:02,000\n世界\n";
        assert_eq!(
            validate_translation(&original(), translated.as_bytes()),
            Err(ValidationError::IndexMismatch {
                block: 2,
                expected: 2,
                found: 3
            })
        );
    }

    #[test]
    fn rejects_non_utf8_bytes() {
        // 0xFF is never valid in UTF-8.
        let translated = b"1\n00:00:00,000 --> 00:00:01,000\n\xff\xfe\n";
        assert_eq!(
            validate_translation(&original(), translated),
            Err(ValidationError::NotUtf8)
        );
    }

    #[test]
    fn rejects_invalid_srt_syntax_and_locates_the_block() {
        let translated = "1\nnot a timing line\n你好\n";
        assert_eq!(
            validate_translation(&original(), translated.as_bytes()),
            Err(ValidationError::Syntax { block: 1 })
        );
    }

    #[test]
    fn count_mismatch_takes_priority_over_index_mismatch() {
        // Both wrong count and wrong indices: count is the coarser error.
        let translated = "5\n00:00:00,000 --> 00:00:01,000\n只有一段\n";
        assert_eq!(
            validate_translation(&original(), translated.as_bytes()),
            Err(ValidationError::SegmentCountMismatch {
                expected: 2,
                found: 1
            })
        );
    }

    #[test]
    fn validation_error_locates_segment_for_the_ui() {
        assert_eq!(ValidationError::NotUtf8.segment(), None);
        assert_eq!(ValidationError::Syntax { block: 3 }.segment(), Some(3));
        assert_eq!(
            ValidationError::IndexMismatch {
                block: 2,
                expected: 2,
                found: 9
            }
            .segment(),
            Some(2)
        );
        assert_eq!(
            ValidationError::SegmentCountMismatch {
                expected: 10,
                found: 8
            }
            .segment(),
            // Locate to the first segment that goes missing.
            Some(9)
        );
    }
}
