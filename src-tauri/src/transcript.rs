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
    pub(crate) model: String,
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

pub(crate) fn ensure_daily_files(paths: &TranscriptPaths) -> anyhow::Result<()> {
    if let Some(parent) = paths.markdown.parent() {
        fs::create_dir_all(parent)?;
    }
    if !paths.markdown.exists() {
        let date = Local::now().format("%Y-%m-%d");
        fs::write(&paths.markdown, format!("# {date}\n\n"))?;
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
    let paths = transcript_paths(output_directory, segment.local_start);
    ensure_daily_files(&paths)?;

    if !segment.text.is_empty() {
        let mut markdown = OpenOptions::new().append(true).open(&paths.markdown)?;
        let heading = format!("## {}\n", segment.local_start.format("%H:%M"));
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
    use crate::MODEL;

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
            model: MODEL.to_string(),
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
    }
}
