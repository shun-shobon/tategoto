use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Local};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TranscriptSegment {
    #[serde(rename = "type")]
    pub(crate) segment_type: &'static str,
    pub(crate) local_start: DateTime<Local>,
    pub(crate) local_end: DateTime<Local>,
    pub(crate) session_id: String,
    pub(crate) item_id: String,
    pub(crate) previous_item_id: Option<String>,
    pub(crate) text: String,
    pub(crate) received_at: DateTime<Local>,
}

pub(crate) struct TranscriptPaths {
    pub(crate) markdown: PathBuf,
    pub(crate) jsonl: PathBuf,
}

pub(crate) fn transcript_paths(output_directory: &Path, date: DateTime<Local>) -> TranscriptPaths {
    let file_stem = date.format("%Y-%m-%d").to_string();
    TranscriptPaths {
        markdown: output_directory.join(format!("{file_stem}.md")),
        jsonl: output_directory.join(format!("{file_stem}.jsonl")),
    }
}

pub(crate) fn ensure_daily_files(
    paths: &TranscriptPaths,
    date: DateTime<Local>,
) -> anyhow::Result<()> {
    if let Some(parent) = paths.markdown.parent() {
        fs::create_dir_all(parent)?;
    }
    if !paths.markdown.exists() {
        let date_label = date.format("%Y-%m-%d");
        fs::write(&paths.markdown, format!("# {date_label}\n\n"))?;
    }
    if !paths.jsonl.exists() {
        fs::write(&paths.jsonl, "")?;
    }
    Ok(())
}

pub(crate) fn append_transcript_segment(
    output_directory: &Path,
    segment: &TranscriptSegment,
) -> anyhow::Result<()> {
    let output_date = segment.local_end;
    let paths = transcript_paths(output_directory, output_date);
    ensure_daily_files(&paths, output_date)?;

    if !segment.text.is_empty() {
        let mut markdown = OpenOptions::new().append(true).open(&paths.markdown)?;
        let heading = format!("## {}\n", output_date.format("%H:%M"));
        let markdown_content = fs::read_to_string(&paths.markdown)?;
        if !markdown_content.contains(&heading) {
            writeln!(markdown, "\n{}", heading.trim_end())?;
        }
        writeln!(
            markdown,
            "- [{}-{}] {}",
            segment.local_start.format("%H:%M:%S"),
            segment.local_end.format("%H:%M:%S"),
            segment.text
        )?;
    }

    let mut jsonl = OpenOptions::new().append(true).open(&paths.jsonl)?;
    writeln!(jsonl, "{}", serde_json::to_string(segment)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_markdown_and_jsonl_transcript_segment() {
        let directory = tempfile::tempdir().expect("tempdir");
        let segment = TranscriptSegment {
            segment_type: "transcript_segment",
            local_start: DateTime::parse_from_rfc3339("2026-05-05T09:15:02+09:00")
                .unwrap()
                .with_timezone(&Local),
            local_end: DateTime::parse_from_rfc3339("2026-05-05T09:15:18+09:00")
                .unwrap()
                .with_timezone(&Local),
            session_id: "sess_test".to_string(),
            item_id: "item_test".to_string(),
            previous_item_id: None,
            text: "今日の作業を始めます。".to_string(),
            received_at: DateTime::parse_from_rfc3339("2026-05-05T09:15:19+09:00")
                .unwrap()
                .with_timezone(&Local),
        };

        append_transcript_segment(directory.path(), &segment).expect("write segment");

        let markdown =
            fs::read_to_string(directory.path().join("2026-05-05.md")).expect("read markdown");
        let jsonl =
            fs::read_to_string(directory.path().join("2026-05-05.jsonl")).expect("read jsonl");
        assert!(markdown.contains("## 09:15"));
        assert!(markdown.contains("- [09:15:02-09:15:18] 今日の作業を始めます。"));
        assert!(jsonl.contains("\"session_id\":\"sess_test\""));
        assert!(jsonl.contains("\"previous_item_id\":null"));
        assert!(!jsonl.contains("\"model\""));
    }

    #[test]
    fn writes_cross_day_segment_to_end_date_files() {
        let directory = tempfile::tempdir().expect("tempdir");
        let segment = TranscriptSegment {
            segment_type: "transcript_segment",
            local_start: DateTime::parse_from_rfc3339("2026-05-10T23:59:58+09:00")
                .unwrap()
                .with_timezone(&Local),
            local_end: DateTime::parse_from_rfc3339("2026-05-11T00:00:04+09:00")
                .unwrap()
                .with_timezone(&Local),
            session_id: "sess_test".to_string(),
            item_id: "item_test".to_string(),
            previous_item_id: None,
            text: "日付をまたいだ発話です。".to_string(),
            received_at: DateTime::parse_from_rfc3339("2026-05-11T00:00:05+09:00")
                .unwrap()
                .with_timezone(&Local),
        };

        append_transcript_segment(directory.path(), &segment).expect("write segment");

        let next_day_markdown = directory.path().join("2026-05-11.md");
        let previous_day_markdown = directory.path().join("2026-05-10.md");
        assert!(next_day_markdown.exists());
        assert!(!previous_day_markdown.exists());

        let markdown = fs::read_to_string(next_day_markdown).expect("read markdown");
        let jsonl =
            fs::read_to_string(directory.path().join("2026-05-11.jsonl")).expect("read jsonl");
        assert!(markdown.contains("# 2026-05-11"));
        assert!(markdown.contains("## 00:00"));
        assert!(markdown.contains("- [23:59:58-00:00:04] 日付をまたいだ発話です。"));
        assert!(jsonl.contains("\"local_end\":\"2026-05-11T00:00:04+09:00\""));
    }
}
