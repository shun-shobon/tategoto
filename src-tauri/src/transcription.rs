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
    app_events::{emit_snapshot, set_error, update_status},
    audio::{AudioBlock, start_audio_capture},
    model::{RuntimeHandle, Settings, SharedAppState, TranscriptionStatus},
    realtime::{RealtimeConnection, RealtimeTimeline, read_chatgpt_token},
    transcript::append_transcript_segment,
};

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
    let token = read_chatgpt_token()?;
    let (audio_tx, mut audio_rx) = mpsc::channel::<AudioBlock>(64);
    let audio_capture = start_audio_capture(settings, audio_tx)?;
    let http = Client::new();
    let mut ws = RealtimeConnection::connect(&http, &token).await?;
    let mut timeline = RealtimeTimeline::new();
    let mut session_started_at = Instant::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                ws.close().await;
                audio_capture.stop();
                return Ok(());
            }
            maybe_block = audio_rx.recv() => {
                let block = maybe_block.context("audio stream closed")?;
                ws.append_audio(block.pcm).await?;
            }
            maybe_segment = ws.next_transcript_segment(&mut timeline) => {
                if let Some(segment) = maybe_segment? {
                    append_transcript_segment(&state.paths.output_directory, &segment)?;
                    emit_snapshot(&app, &state, "transcript_segment_written").await;
                }
            }
            _ = tokio::time::sleep_until(next_rotation_deadline(session_started_at)) => {
                update_status(&app, &state, TranscriptionStatus::RotatingSession, None).await;
                ws.close().await;
                ws = RealtimeConnection::connect(&http, &token).await?;
                timeline = RealtimeTimeline::new();
                session_started_at = Instant::now();
                update_status(&app, &state, TranscriptionStatus::Recording, None).await;
            }
        }
    }
}

fn next_rotation_deadline(session_started_at: Instant) -> Instant {
    session_started_at + ROTATE_AFTER
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
