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
    StoppedWithError,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputDeviceMode {
    #[default]
    SystemDefault,
    FixedDevice,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct TranscriptionSettings {
    #[serde(default)]
    pub(crate) locale_identifier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InputDevice {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Settings {
    #[serde(default)]
    pub(crate) input_device_mode: InputDeviceMode,
    #[serde(default)]
    pub(crate) input_device_id: Option<String>,
    #[serde(default)]
    pub(crate) input_device_name: Option<String>,
    #[serde(default)]
    pub(crate) transcription: TranscriptionSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device_mode: InputDeviceMode::SystemDefault,
            input_device_id: None,
            input_device_name: None,
            transcription: TranscriptionSettings::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_settings_use_locale_identifier_contract() {
        let settings = TranscriptionSettings {
            locale_identifier: Some("ja-JP".to_string()),
        };

        let json = serde_json::to_string(&settings).unwrap();

        assert_eq!(json, r#"{"locale_identifier":"ja-JP"}"#);
    }
}
