#![warn(clippy::all, clippy::pedantic)]

mod app_events;
mod app_paths;
mod audio;
mod commands;
mod model;
mod realtime;
mod settings;
mod transcript;
mod transcription;
mod tray;

use std::{sync::Arc, time::Duration};

use model::{AppModel, SharedState, TranscriptionStatus};
use tauri::{ActivationPolicy, Manager, WindowEvent};

const TRAY_ID: &str = "main";
const TARGET_SAMPLE_RATE: u32 = 24_000;
const TARGET_CHANNELS: u16 = 1;
const ROTATE_AFTER: Duration = Duration::from_secs(50 * 60);

/// Starts the Tategoto Tauri application.
///
/// # Panics
///
/// Panics when Tauri fails to initialize or run the application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.set_activation_policy(ActivationPolicy::Accessory);
            let paths = app_paths::build_paths(app.handle())?;
            let settings = settings::load_settings(&paths.config_file)?;
            let shared = SharedState {
                model: tokio::sync::Mutex::new(AppModel {
                    status: TranscriptionStatus::Idle,
                    settings,
                    last_error: None,
                    last_warning: None,
                    runtime: None,
                }),
                paths,
            };
            app.manage(Arc::new(shared));
            keep_main_window_in_tray(app.handle());
            tray::setup_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::start_transcription,
            commands::stop_transcription,
            commands::refresh_input_devices,
            commands::update_settings,
            commands::open_today_markdown,
            commands::open_output_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn keep_main_window_in_tray(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let window_to_hide = window.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window_to_hide.hide();
            }
        });
    }
}
