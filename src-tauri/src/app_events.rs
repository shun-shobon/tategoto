use tauri::{AppHandle, Emitter};

use crate::{
    audio::list_input_devices,
    model::{AppSnapshot, SharedAppState, TranscriptionStatus},
    transcript::transcript_paths,
    tray::update_tray_status,
};

pub(crate) async fn snapshot(state: &SharedAppState) -> anyhow::Result<AppSnapshot> {
    let model = state.model.lock().await;
    let paths = transcript_paths(&state.paths.output_directory, chrono::Local::now());
    Ok(AppSnapshot {
        status: model.status.clone(),
        settings: model.settings.clone(),
        devices: list_input_devices()?,
        output_directory: state.paths.output_directory.to_string_lossy().to_string(),
        today_markdown_path: paths.markdown.to_string_lossy().to_string(),
        today_jsonl_path: paths.jsonl.to_string_lossy().to_string(),
        last_error: model.last_error.clone(),
        last_warning: model.last_warning.clone(),
    })
}

pub(crate) async fn update_status(
    app: &AppHandle,
    state: &SharedAppState,
    status: TranscriptionStatus,
    error: Option<String>,
) {
    {
        let mut model = state.model.lock().await;
        model.status = status;
        model.last_error = error;
    }
    update_tray_status(app, state).await;
    emit_snapshot(app, state, "transcription_state_changed").await;
}

pub(crate) async fn set_error(app: &AppHandle, state: &SharedAppState, error: String) {
    {
        let mut model = state.model.lock().await;
        model.status = TranscriptionStatus::StoppedWithError;
        model.last_error = Some(error);
        model.runtime = None;
    }
    update_tray_status(app, state).await;
    emit_snapshot(app, state, "transcription_error").await;
}

pub(crate) async fn set_warning(app: &AppHandle, state: &SharedAppState, warning: String) {
    {
        let mut model = state.model.lock().await;
        model.last_warning = Some(warning);
    }
    emit_snapshot(app, state, "transcription_state_changed").await;
}

pub(crate) async fn emit_snapshot(app: &AppHandle, state: &SharedAppState, event: &str) {
    update_tray_status(app, state).await;
    if let Ok(snapshot) = snapshot(state).await {
        let _ = app.emit(event, snapshot);
    }
}
