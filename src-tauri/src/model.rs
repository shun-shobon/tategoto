use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TranscriptionStatus {
    Idle,
    Recording,
    RotatingSession,
    StoppedWithError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputDeviceMode {
    SystemDefault,
    FixedDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InputDevice {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Settings {
    pub(crate) input_device_mode: InputDeviceMode,
    pub(crate) input_device_id: Option<String>,
    pub(crate) input_device_name: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device_mode: InputDeviceMode::SystemDefault,
            input_device_id: None,
            input_device_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AppSnapshot {
    pub(crate) status: TranscriptionStatus,
    pub(crate) settings: Settings,
    pub(crate) devices: Vec<InputDevice>,
    pub(crate) output_directory: String,
    pub(crate) today_markdown_path: String,
    pub(crate) today_jsonl_path: String,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug)]
pub(crate) struct RuntimeHandle {
    pub(crate) cancel: CancellationToken,
    pub(crate) join: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Debug)]
pub(crate) struct AppModel {
    pub(crate) status: TranscriptionStatus,
    pub(crate) settings: Settings,
    pub(crate) last_error: Option<String>,
    pub(crate) runtime: Option<RuntimeHandle>,
}

#[derive(Debug)]
pub(crate) struct AppPaths {
    pub(crate) config_file: PathBuf,
    pub(crate) output_directory: PathBuf,
}

#[derive(Debug)]
pub(crate) struct SharedState {
    pub(crate) model: Mutex<AppModel>,
    pub(crate) paths: AppPaths,
}

#[derive(Debug, Error)]
pub(crate) enum CommandError {
    #[error("{0}")]
    Message(String),
}

impl From<anyhow::Error> for CommandError {
    fn from(value: anyhow::Error) -> Self {
        Self::Message(format!("{value:#}"))
    }
}

impl Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub(crate) type SharedAppState = Arc<SharedState>;
