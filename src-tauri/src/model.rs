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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputDeviceMode {
    #[default]
    SystemDefault,
    FixedDevice,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum TranscriptionModel {
    #[serde(rename = "gpt-4o-transcribe")]
    #[default]
    Gpt4oTranscribe,
    #[serde(rename = "gpt-4o-mini-transcribe")]
    Gpt4oMiniTranscribe,
}

impl TranscriptionModel {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Gpt4oTranscribe => "gpt-4o-transcribe",
            Self::Gpt4oMiniTranscribe => "gpt-4o-mini-transcribe",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoiseReductionType {
    NearField,
    FarField,
}

impl NoiseReductionType {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::NearField => "near_field",
            Self::FarField => "far_field",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct TurnDetectionSettings {
    #[serde(default = "default_vad_threshold")]
    pub(crate) threshold: f64,
    #[serde(default = "default_prefix_padding_ms")]
    pub(crate) prefix_padding_ms: u32,
    #[serde(default = "default_silence_duration_ms")]
    pub(crate) silence_duration_ms: u32,
}

const fn default_vad_threshold() -> f64 {
    0.5
}

const fn default_prefix_padding_ms() -> u32 {
    300
}

const fn default_silence_duration_ms() -> u32 {
    700
}

impl Default for TurnDetectionSettings {
    fn default() -> Self {
        Self {
            threshold: default_vad_threshold(),
            prefix_padding_ms: default_prefix_padding_ms(),
            silence_duration_ms: default_silence_duration_ms(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct TranscriptionSettings {
    #[serde(default)]
    pub(crate) model: TranscriptionModel,
    #[serde(default)]
    pub(crate) language: Option<String>,
    #[serde(default)]
    pub(crate) prompt: Option<String>,
    #[serde(default)]
    pub(crate) noise_reduction: Option<NoiseReductionType>,
    #[serde(default)]
    pub(crate) turn_detection: TurnDetectionSettings,
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
    pub(crate) last_warning: Option<String>,
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
    pub(crate) last_warning: Option<String>,
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
