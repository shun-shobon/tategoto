use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Duration as ChronoDuration, Local};
use tauri::AppHandle;
use tokio::{sync::mpsc, time::timeout};
use tokio_util::sync::CancellationToken;

use crate::{
    app_events::{emit_snapshot, set_error},
    apple_speech::{AppleSpeechConnection, AppleSpeechEvent, AppleSpeechSegment},
    audio::{AudioBlock, AudioCapture, start_audio_capture},
    model::{RuntimeHandle, Settings, SharedAppState, TranscriptionStatus},
    transcript::{TranscriptSegment, append_transcript_segment},
};

const FLUSH_TIMEOUT: Duration = Duration::from_secs(10);
const CONTIGUOUS_AUDIO_GAP_TOLERANCE_MILLIS: i64 = 2_000;

pub(crate) async fn start_recording(app: AppHandle, state: SharedAppState) -> anyhow::Result<()> {
    let settings = {
        let mut model = state.model.lock().await;
        if model.runtime.is_some() {
            return Ok(());
        }

        model.status = TranscriptionStatus::Recording;
        model.last_error = None;
        model.settings.clone()
    };

    let cancel = CancellationToken::new();
    let task_state = state.clone();
    let task_app = app.clone();
    let task_cancel = cancel.clone();
    let join = tauri::async_runtime::spawn(async move {
        if let Err(error) =
            run_transcription(task_app.clone(), task_state.clone(), settings, task_cancel).await
        {
            set_error(&task_app, &task_state, format!("{error:#}")).await;
        }
    });

    {
        let mut model = state.model.lock().await;
        model.runtime = Some(RuntimeHandle { cancel, join });
    }

    emit_snapshot(&app, &state, "transcription_state_changed").await;
    Ok(())
}

pub(crate) async fn stop_recording(app: AppHandle, state: SharedAppState) {
    let runtime = {
        let mut model = state.model.lock().await;
        model.runtime.take()
    };

    if let Some(runtime) = runtime {
        runtime.cancel.cancel();
        let _ = timeout(Duration::from_secs(20), runtime.join).await;
    }

    {
        let mut model = state.model.lock().await;
        if model.status != TranscriptionStatus::StoppedWithError {
            model.status = TranscriptionStatus::Idle;
        }
    }

    emit_snapshot(&app, &state, "transcription_state_changed").await;
}

async fn run_transcription(
    app: AppHandle,
    state: SharedAppState,
    settings: Settings,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let (mut speech, mut speech_rx) = AppleSpeechConnection::start(&settings.transcription)?;
    wait_until_speech_ready(&mut speech_rx).await?;

    let (audio_tx, mut audio_rx) = mpsc::channel::<AudioBlock>(64);
    let mut audio_capture = Some(start_audio_capture(settings, audio_tx)?);
    let mut audio_timeline = AudioTimeline::default();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                stop_audio_capture(&mut audio_capture);
                drain_audio_queue(&mut audio_rx, &speech, &mut audio_timeline);
                speech.stop();
                drain_speech_events_until_stopped(&app, &state, &mut speech_rx, &audio_timeline).await?;
                return Ok(());
            }
            maybe_block = audio_rx.recv() => {
                let block = maybe_block.context("audio stream closed")?;
                audio_timeline.record_block(&block);
                speech.append_audio(&block);
            }
            maybe_event = speech_rx.recv() => {
                let event = maybe_event.context("Apple SpeechTranscriber event stream closed")??;
                handle_speech_event(&app, &state, event, &audio_timeline).await?;
            }
        }
    }
}

async fn wait_until_speech_ready(
    speech_rx: &mut mpsc::UnboundedReceiver<anyhow::Result<AppleSpeechEvent>>,
) -> anyhow::Result<()> {
    loop {
        match speech_rx
            .recv()
            .await
            .context("Apple SpeechTranscriber event stream closed before ready")??
        {
            AppleSpeechEvent::Ready => return Ok(()),
            AppleSpeechEvent::Error { message } => anyhow::bail!(message),
            AppleSpeechEvent::Stopped => {
                anyhow::bail!("Apple SpeechTranscriber stopped before ready")
            }
            AppleSpeechEvent::Segment(_) => {}
        }
    }
}

fn stop_audio_capture(audio_capture: &mut Option<AudioCapture>) {
    if let Some(capture) = audio_capture.take() {
        capture.stop();
    }
}

fn drain_audio_queue(
    audio_rx: &mut mpsc::Receiver<AudioBlock>,
    speech: &AppleSpeechConnection,
    audio_timeline: &mut AudioTimeline,
) {
    while let Ok(block) = audio_rx.try_recv() {
        audio_timeline.record_block(&block);
        speech.append_audio(&block);
    }
}

async fn drain_speech_events_until_stopped(
    app: &AppHandle,
    state: &SharedAppState,
    speech_rx: &mut mpsc::UnboundedReceiver<anyhow::Result<AppleSpeechEvent>>,
    audio_timeline: &AudioTimeline,
) -> anyhow::Result<()> {
    let deadline = tokio::time::sleep(FLUSH_TIMEOUT);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            maybe_event = speech_rx.recv() => {
                let event = maybe_event.context("Apple SpeechTranscriber event stream closed while stopping")??;
                if matches!(event, AppleSpeechEvent::Stopped) {
                    return Ok(());
                }
                handle_speech_event(app, state, event, audio_timeline).await?;
            }
            () = &mut deadline => {
                anyhow::bail!(
                    "Apple SpeechTranscriber の停止処理が{}秒以内に完了しませんでした。未保存の発話がある可能性があります。",
                    FLUSH_TIMEOUT.as_secs()
                );
            }
        }
    }
}

async fn handle_speech_event(
    app: &AppHandle,
    state: &SharedAppState,
    event: AppleSpeechEvent,
    audio_timeline: &AudioTimeline,
) -> anyhow::Result<()> {
    match event {
        AppleSpeechEvent::Segment(segment) => {
            let segment = transcript_segment_from_apple(segment, audio_timeline)?;
            append_transcript_segment(&state.paths.output_directory, &segment)?;
            emit_snapshot(app, state, "transcript_segment_written").await;
        }
        AppleSpeechEvent::Error { message } => {
            anyhow::bail!("Apple SpeechTranscriber error: {message}");
        }
        AppleSpeechEvent::Ready | AppleSpeechEvent::Stopped => {}
    }
    Ok(())
}

fn transcript_segment_from_apple(
    segment: AppleSpeechSegment,
    audio_timeline: &AudioTimeline,
) -> anyhow::Result<TranscriptSegment> {
    let local_start = audio_timeline.local_time_for_offset(segment.start_offset_secs)?;
    let local_end = audio_timeline.local_time_for_offset(segment.end_offset_secs)?;

    Ok(TranscriptSegment {
        segment_type: "transcript_segment",
        local_start,
        local_end,
        session_id: segment.session_id,
        item_id: segment.item_id,
        previous_item_id: segment.previous_item_id,
        text: segment.text,
        received_at: Local::now(),
    })
}

fn chrono_duration_from_secs(seconds: f64) -> anyhow::Result<ChronoDuration> {
    if !seconds.is_finite() || seconds.is_sign_negative() {
        anyhow::bail!("invalid Apple SpeechTranscriber audio offset: {seconds}");
    }
    Ok(ChronoDuration::from_std(Duration::from_secs_f64(seconds))?)
}

#[derive(Debug, Default)]
struct AudioTimeline {
    ranges: Vec<AudioTimestampRange>,
    audio_end: ChronoDuration,
}

#[derive(Debug)]
struct AudioTimestampRange {
    audio_start: ChronoDuration,
    audio_end: ChronoDuration,
    local_start: DateTime<Local>,
    local_end: DateTime<Local>,
}

impl AudioTimeline {
    fn record_block(&mut self, block: &AudioBlock) {
        if block.duration <= ChronoDuration::zero() {
            return;
        }

        let audio_start = self.audio_end;
        let audio_end = audio_start + block.duration;
        let local_end = block.captured_at;
        let local_start = local_end - block.duration;

        if let Some(last) = self.ranges.last_mut() {
            let local_gap = local_start - last.local_end;
            if audio_start == last.audio_end
                && local_gap.num_milliseconds().abs() <= CONTIGUOUS_AUDIO_GAP_TOLERANCE_MILLIS
            {
                last.audio_end = audio_end;
                last.local_end = local_end;
                self.audio_end = audio_end;
                return;
            }
        }

        self.ranges.push(AudioTimestampRange {
            audio_start,
            audio_end,
            local_start,
            local_end,
        });
        self.audio_end = audio_end;
    }

    fn local_time_for_offset(&self, offset_secs: f64) -> anyhow::Result<DateTime<Local>> {
        let offset = chrono_duration_from_secs(offset_secs)?;

        for range in &self.ranges {
            if offset >= range.audio_start && offset <= range.audio_end {
                return Ok(range.local_start + (offset - range.audio_start));
            }
        }

        if let Some(first) = self.ranges.first()
            && offset < first.audio_start
        {
            return Ok(first.local_start);
        }
        if let Some(last) = self.ranges.last()
            && offset > last.audio_end
        {
            return Ok(last.local_end + (offset - last.audio_end));
        }

        anyhow::bail!(
            "Apple SpeechTranscriber audio offset has no matching captured audio range: {offset_secs}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_apple_segment_offsets_to_local_timestamps() {
        let origin = DateTime::parse_from_rfc3339("2026-05-05T09:15:00+09:00")
            .unwrap()
            .with_timezone(&Local);
        let mut audio_timeline = AudioTimeline::default();
        audio_timeline.record_block(&AudioBlock {
            pcm: Vec::new(),
            captured_at: origin + ChronoDuration::seconds(5),
            duration: ChronoDuration::seconds(5),
        });
        let segment = transcript_segment_from_apple(
            AppleSpeechSegment {
                session_id: "apple-session".to_string(),
                item_id: "apple_1".to_string(),
                previous_item_id: None,
                text: "今日の作業を始めます。".to_string(),
                start_offset_secs: 2.0,
                end_offset_secs: 4.5,
            },
            &audio_timeline,
        )
        .unwrap();

        assert_eq!(segment.local_start, origin + ChronoDuration::seconds(2));
        assert_eq!(
            segment.local_end,
            origin + ChronoDuration::milliseconds(4_500)
        );
    }

    #[test]
    fn rejects_invalid_apple_segment_offset() {
        let error = chrono_duration_from_secs(f64::NAN).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("invalid Apple SpeechTranscriber")
        );
    }

    #[test]
    fn maps_offsets_after_wall_clock_gap_to_post_gap_timestamps() {
        let origin = DateTime::parse_from_rfc3339("2026-05-05T09:15:00+09:00")
            .unwrap()
            .with_timezone(&Local);
        let resumed = DateTime::parse_from_rfc3339("2026-05-05T10:15:00+09:00")
            .unwrap()
            .with_timezone(&Local);
        let mut audio_timeline = AudioTimeline::default();
        audio_timeline.record_block(&AudioBlock {
            pcm: Vec::new(),
            captured_at: origin + ChronoDuration::seconds(1),
            duration: ChronoDuration::seconds(1),
        });
        audio_timeline.record_block(&AudioBlock {
            pcm: Vec::new(),
            captured_at: resumed + ChronoDuration::seconds(1),
            duration: ChronoDuration::seconds(1),
        });

        let segment = transcript_segment_from_apple(
            AppleSpeechSegment {
                session_id: "apple-session".to_string(),
                item_id: "apple_1".to_string(),
                previous_item_id: None,
                text: "復帰後の発話です。".to_string(),
                start_offset_secs: 1.2,
                end_offset_secs: 1.8,
            },
            &audio_timeline,
        )
        .unwrap();

        assert_eq!(
            segment.local_start,
            resumed + ChronoDuration::milliseconds(200)
        );
        assert_eq!(
            segment.local_end,
            resumed + ChronoDuration::milliseconds(800)
        );
    }
}
