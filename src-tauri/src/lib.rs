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
use tauri::Manager;

const MODEL: &str = "gpt-4o-mini-transcribe";
const TRAY_ID: &str = "main";
const TARGET_SAMPLE_RATE: u32 = 24_000;
const TARGET_CHANNELS: u16 = 1;
const CHUNK_SECONDS: i64 = 15;
const ROTATE_AFTER: Duration = Duration::from_secs(50 * 60);
const COMPLETION_WAIT: Duration = Duration::from_secs(60);

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = app_paths::build_paths(app.handle())?;
            let settings = settings::load_settings(&paths.config_file)?;
            let shared = SharedState {
                model: tokio::sync::Mutex::new(AppModel {
                    status: TranscriptionStatus::Idle,
                    settings,
                    last_error: None,
                    runtime: None,
                }),
                paths,
            };
            app.manage(Arc::new(shared));
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
