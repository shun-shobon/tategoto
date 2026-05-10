use std::fs;

use anyhow::anyhow;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::{
    app_events::{emit_snapshot, snapshot},
    model::{AppSnapshot, CommandError, Settings, SharedAppState},
    settings::save_settings,
    transcript::{ensure_daily_files, transcript_paths},
    transcription::{start_recording, stop_recording},
};

#[tauri::command]
pub(crate) async fn get_snapshot(
    state: State<'_, SharedAppState>,
) -> Result<AppSnapshot, CommandError> {
    Ok(snapshot(&state).await?)
}

#[tauri::command]
pub(crate) async fn refresh_input_devices(
    state: State<'_, SharedAppState>,
) -> Result<AppSnapshot, CommandError> {
    Ok(snapshot(&state).await?)
}

#[tauri::command]
pub(crate) async fn update_settings(
    settings: Settings,
    state: State<'_, SharedAppState>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
    save_settings(&state.paths.config_file, &settings)?;
    {
        let mut model = state.model.lock().await;
        model.settings = settings;
    }
    emit_snapshot(&app, &state, "transcription_state_changed").await;
    Ok(snapshot(&state).await?)
}

#[tauri::command]
pub(crate) async fn start_transcription(
    state: State<'_, SharedAppState>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
    start_recording(app.clone(), state.inner().clone()).await?;
    Ok(snapshot(&state).await?)
}

#[tauri::command]
pub(crate) async fn stop_transcription(
    state: State<'_, SharedAppState>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
    stop_recording(app.clone(), state.inner().clone()).await;
    Ok(snapshot(&state).await?)
}

#[tauri::command]
pub(crate) async fn open_today_markdown(
    state: State<'_, SharedAppState>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
    let today = chrono::Local::now();
    let paths = transcript_paths(&state.paths.output_directory, today);
    ensure_daily_files(&paths, today)?;
    app.opener()
        .open_path(paths.markdown.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| anyhow!("{error}"))?;
    Ok(snapshot(&state).await?)
}

#[tauri::command]
pub(crate) async fn open_output_directory(
    state: State<'_, SharedAppState>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
    fs::create_dir_all(&state.paths.output_directory).map_err(anyhow::Error::from)?;
    app.opener()
        .open_path(
            state.paths.output_directory.to_string_lossy().to_string(),
            None::<&str>,
        )
        .map_err(|error| anyhow!("{error}"))?;
    Ok(snapshot(&state).await?)
}
