use std::time::Duration;

use anyhow::Context;
use reqwest::Client;
use tauri::AppHandle;
use tokio::{
    sync::mpsc,
    time::{Instant, timeout},
};
use tokio_util::sync::CancellationToken;

use crate::{
    ROTATE_AFTER,
    app_events::{emit_snapshot, set_error, set_warning, update_status},
    audio::{AudioBlock, AudioCapture, start_audio_capture},
    model::{RuntimeHandle, Settings, SharedAppState, TranscriptionStatus},
    realtime::{RealtimeConnection, RealtimeEvent, RealtimeTimeline, read_chatgpt_token},
    transcript::append_transcript_segment,
};

const FLUSH_TIMEOUT: Duration = Duration::from_secs(10);
const PREFLUSH_EVENT_DRAIN_TIMEOUT: Duration = Duration::from_millis(50);

pub(crate) async fn start_recording(app: AppHandle, state: SharedAppState) -> anyhow::Result<()> {
    let settings = {
        let mut model = state.model.lock().await;
        if model.runtime.is_some() {
            return Ok(());
        }

        model.status = TranscriptionStatus::Recording;
        model.last_error = None;
        model.last_warning = None;
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
    let token = read_chatgpt_token()?;
    let (audio_tx, mut audio_rx) = mpsc::channel::<AudioBlock>(64);
    let mut audio_capture = Some(start_audio_capture(settings.clone(), audio_tx)?);
    let http = Client::new();
    let mut ws = RealtimeConnection::connect(&http, &token, &settings.transcription).await?;
    let mut timeline = RealtimeTimeline::new();
    let mut session_started_at = Instant::now();

    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                stop_audio_capture(&mut audio_capture);
                drain_audio_queue(&mut audio_rx, &mut ws, &mut timeline).await?;
                if let Some(warning) = flush_current_session(
                    &app,
                    &state,
                    &mut ws,
                    &mut timeline,
                    &mut audio_rx,
                    None,
                ).await? {
                    set_warning(&app, &state, warning).await;
                }
                ws.close().await;
                return Ok(());
            }
            maybe_block = audio_rx.recv() => {
                let block = maybe_block.context("audio stream closed")?;
                append_audio_block(&mut ws, &mut timeline, block).await?;
            }
            maybe_event = ws.next_event(&mut timeline) => {
                handle_realtime_event(&app, &state, maybe_event?).await?;
            }
            () = tokio::time::sleep_until(next_rotation_deadline(session_started_at)) => {
                update_status(&app, &state, TranscriptionStatus::RotatingSession, None).await;
                drain_audio_queue(&mut audio_rx, &mut ws, &mut timeline).await?;
                let mut deferred_audio = Vec::new();
                if let Some(warning) = flush_current_session(
                    &app,
                    &state,
                    &mut ws,
                    &mut timeline,
                    &mut audio_rx,
                    Some(&mut deferred_audio),
                ).await? {
                    set_warning(&app, &state, warning).await;
                }
                while let Ok(block) = audio_rx.try_recv() {
                    deferred_audio.push(block);
                }
                ws.close().await;
                ws = RealtimeConnection::connect(&http, &token, &settings.transcription).await?;
                timeline = RealtimeTimeline::new();
                for block in deferred_audio {
                    append_audio_block(&mut ws, &mut timeline, block).await?;
                }
                session_started_at = Instant::now();
                update_status(&app, &state, TranscriptionStatus::Recording, None).await;
            }
        }
    }
}

fn next_rotation_deadline(session_started_at: Instant) -> Instant {
    session_started_at + ROTATE_AFTER
}

fn stop_audio_capture(audio_capture: &mut Option<AudioCapture>) {
    if let Some(capture) = audio_capture.take() {
        capture.stop();
    }
}

async fn append_audio_block(
    ws: &mut RealtimeConnection,
    timeline: &mut RealtimeTimeline,
    block: AudioBlock,
) -> anyhow::Result<()> {
    if block.pcm.is_empty() {
        return Ok(());
    }

    timeline.record_audio_block(&block);
    ws.append_audio(block.pcm).await
}

async fn drain_audio_queue(
    audio_rx: &mut mpsc::Receiver<AudioBlock>,
    ws: &mut RealtimeConnection,
    timeline: &mut RealtimeTimeline,
) -> anyhow::Result<()> {
    while let Ok(block) = audio_rx.try_recv() {
        append_audio_block(ws, timeline, block).await?;
    }
    Ok(())
}

async fn flush_current_session(
    app: &AppHandle,
    state: &SharedAppState,
    ws: &mut RealtimeConnection,
    timeline: &mut RealtimeTimeline,
    audio_rx: &mut mpsc::Receiver<AudioBlock>,
    mut deferred_audio: Option<&mut Vec<AudioBlock>>,
) -> anyhow::Result<Option<String>> {
    drain_realtime_events(app, state, ws, timeline, PREFLUSH_EVENT_DRAIN_TIMEOUT).await?;

    let should_commit = timeline.has_committable_audio();
    if !should_commit && !timeline.has_pending_turns() {
        return Ok(None);
    }

    if should_commit && let Err(error) = ws.commit_audio().await {
        return Ok(Some(format!(
            "最後の音声を確定できませんでした。未保存の発話がある可能性があります: {error:#}"
        )));
    }

    let deadline = tokio::time::sleep(FLUSH_TIMEOUT);
    tokio::pin!(deadline);
    let mut saw_commit = !should_commit;
    let collect_deferred_audio = deferred_audio.is_some();

    loop {
        if saw_commit && !timeline.has_pending_turns() {
            return Ok(None);
        }

        tokio::select! {
            event = ws.next_event(timeline) => {
                match event? {
                    RealtimeEvent::Committed => {
                        saw_commit = true;
                    }
                    RealtimeEvent::TranscriptSegment(segment) => {
                        append_transcript_segment(&state.paths.output_directory, &segment)?;
                        emit_snapshot(app, state, "transcript_segment_written").await;
                    }
                    RealtimeEvent::CommitRejected(_) => {
                        if !timeline.has_pending_turns() {
                            return Ok(None);
                        }
                        saw_commit = true;
                    }
                    RealtimeEvent::ApiError(message) => {
                        return Ok(Some(format!(
                            "最後の音声の保存確認中にRealtime APIエラーが発生しました。未保存の発話がある可能性があります: {message}"
                        )));
                    }
                    RealtimeEvent::Ignored => {}
                }
            }
            maybe_block = audio_rx.recv(), if collect_deferred_audio => {
                if let Some(block) = maybe_block
                    && let Some(buffer) = deferred_audio.as_deref_mut()
                {
                    buffer.push(block);
                }
            }
            () = &mut deadline => {
                return Ok(Some(format!(
                    "最後の音声の保存確認が{}秒以内に完了しませんでした。未保存の発話がある可能性があります。",
                    FLUSH_TIMEOUT.as_secs()
                )));
            }
        }
    }
}

async fn handle_realtime_event(
    app: &AppHandle,
    state: &SharedAppState,
    event: RealtimeEvent,
) -> anyhow::Result<()> {
    match event {
        RealtimeEvent::TranscriptSegment(segment) => {
            append_transcript_segment(&state.paths.output_directory, &segment)?;
            emit_snapshot(app, state, "transcript_segment_written").await;
        }
        RealtimeEvent::ApiError(message) => {
            anyhow::bail!("Realtime API error: {message}");
        }
        RealtimeEvent::CommitRejected(message) => {
            anyhow::bail!("Realtime API rejected audio commit: {message}");
        }
        RealtimeEvent::Committed | RealtimeEvent::Ignored => {}
    }
    Ok(())
}

async fn drain_realtime_events(
    app: &AppHandle,
    state: &SharedAppState,
    ws: &mut RealtimeConnection,
    timeline: &mut RealtimeTimeline,
    idle_timeout: Duration,
) -> anyhow::Result<()> {
    loop {
        match timeout(idle_timeout, ws.next_event(timeline)).await {
            Ok(event) => handle_realtime_event(app, state, event?).await?,
            Err(_) => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_deadline_is_50_minutes_after_session_start() {
        let started_at = Instant::now();
        assert_eq!(
            next_rotation_deadline(started_at).duration_since(started_at),
            ROTATE_AFTER
        );
    }
}
