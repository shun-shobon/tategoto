use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Arc,
    thread,
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Local};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{
    AppHandle, Emitter, Manager, Runtime, State,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_opener::OpenerExt;
use thiserror::Error;
use tokio::{
    sync::{Mutex, mpsc},
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;

const MODEL: &str = "gpt-4o-mini-transcribe";
const TRAY_ID: &str = "main";
const TARGET_SAMPLE_RATE: u32 = 24_000;
const TARGET_CHANNELS: u16 = 1;
const CHUNK_SECONDS: i64 = 15;
const ROTATE_AFTER: Duration = Duration::from_secs(50 * 60);
const COMPLETION_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TranscriptionStatus {
    Idle,
    Recording,
    RotatingSession,
    StoppedWithError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum InputDeviceMode {
    SystemDefault,
    FixedDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InputDevice {
    id: String,
    name: String,
    is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    input_device_mode: InputDeviceMode,
    input_device_id: Option<String>,
    input_device_name: Option<String>,
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
struct AppSnapshot {
    status: TranscriptionStatus,
    settings: Settings,
    devices: Vec<InputDevice>,
    output_directory: String,
    today_markdown_path: String,
    today_jsonl_path: String,
    last_error: Option<String>,
}

#[derive(Debug)]
struct RuntimeHandle {
    cancel: CancellationToken,
    join: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Debug)]
struct AppModel {
    status: TranscriptionStatus,
    settings: Settings,
    last_error: Option<String>,
    runtime: Option<RuntimeHandle>,
}

#[derive(Debug)]
struct AppPaths {
    config_file: PathBuf,
    output_directory: PathBuf,
}

#[derive(Debug)]
struct SharedState {
    model: Mutex<AppModel>,
    paths: AppPaths,
}

#[derive(Debug, Error)]
enum CommandError {
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

#[derive(Debug)]
struct AudioBlock {
    pcm: Vec<u8>,
    duration: Duration,
}

#[derive(Debug)]
struct AudioCapture {
    stop: std::sync::mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
}

impl AudioCapture {
    fn stop(mut self) {
        let _ = self.stop.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[derive(Debug)]
struct PendingChunk {
    pcm: Vec<u8>,
    local_start: DateTime<Local>,
    local_end: DateTime<Local>,
}

#[derive(Debug, Clone)]
struct SessionCredentials {
    id: String,
    client_secret: String,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    id: String,
    client_secret: ClientSecret,
}

#[derive(Debug, Deserialize)]
struct ClientSecret {
    value: String,
}

#[derive(Debug, Clone, Serialize)]
struct TranscriptSegment {
    #[serde(rename = "type")]
    segment_type: &'static str,
    local_start: DateTime<Local>,
    local_end: DateTime<Local>,
    session_id: String,
    item_id: String,
    previous_item_id: Option<String>,
    model: String,
    text: String,
    received_at: DateTime<Local>,
}

#[derive(Debug, Deserialize)]
struct ChatGptAuth {
    tokens: ChatGptTokens,
}

#[derive(Debug, Deserialize)]
struct ChatGptTokens {
    access_token: String,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = build_paths(app.handle())?;
            let settings = load_settings(&paths.config_file)?;
            let shared = SharedState {
                model: Mutex::new(AppModel {
                    status: TranscriptionStatus::Idle,
                    settings,
                    last_error: None,
                    runtime: None,
                }),
                paths,
            };
            app.manage(Arc::new(shared));
            setup_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            start_transcription,
            stop_transcription,
            refresh_input_devices,
            update_settings,
            open_today_markdown,
            open_output_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[tauri::command]
async fn get_snapshot(state: State<'_, Arc<SharedState>>) -> Result<AppSnapshot, CommandError> {
    Ok(snapshot(&state).await?)
}

#[tauri::command]
async fn refresh_input_devices(
    state: State<'_, Arc<SharedState>>,
) -> Result<AppSnapshot, CommandError> {
    Ok(snapshot(&state).await?)
}

#[tauri::command]
async fn update_settings(
    settings: Settings,
    state: State<'_, Arc<SharedState>>,
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
async fn start_transcription(
    state: State<'_, Arc<SharedState>>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
    let settings = {
        let mut model = state.model.lock().await;
        if model.runtime.is_some() {
            return Ok(snapshot(&state).await?);
        }

        model.status = TranscriptionStatus::Recording;
        model.last_error = None;
        model.settings.clone()
    };

    let cancel = CancellationToken::new();
    let task_state = state.inner().clone();
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
    Ok(snapshot(&state).await?)
}

#[tauri::command]
async fn stop_transcription(
    state: State<'_, Arc<SharedState>>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
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
    Ok(snapshot(&state).await?)
}

#[tauri::command]
async fn open_today_markdown(
    state: State<'_, Arc<SharedState>>,
    app: AppHandle,
) -> Result<AppSnapshot, CommandError> {
    let paths = transcript_paths(&state.paths.output_directory, Local::now());
    ensure_daily_files(&paths)?;
    app.opener()
        .open_path(paths.markdown.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| anyhow!("{error}"))?;
    Ok(snapshot(&state).await?)
}

#[tauri::command]
async fn open_output_directory(
    state: State<'_, Arc<SharedState>>,
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

async fn run_transcription(
    app: AppHandle,
    state: Arc<SharedState>,
    settings: Settings,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let token = read_chatgpt_token()?;
    let (audio_tx, mut audio_rx) = mpsc::channel::<AudioBlock>(64);
    let audio_capture = start_audio_capture(settings, audio_tx)?;
    let http = Client::new();
    let mut ws = RealtimeConnection::connect(&http, &token).await?;
    let mut session_started_at = Instant::now();
    let mut chunk = ChunkBuilder::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                if let Some(pending) = chunk.finish() {
                    ws.commit_chunk(pending, &state, &app).await?;
                }
                ws.close().await;
                audio_capture.stop();
                return Ok(());
            }
            maybe_block = audio_rx.recv() => {
                let block = maybe_block.context("audio stream closed")?;
                if let Some(pending) = chunk.push(block)? {
                    ws.commit_chunk(pending, &state, &app).await?;
                }
            }
            _ = tokio::time::sleep_until(next_rotation_deadline(session_started_at)) => {
                update_status(&app, &state, TranscriptionStatus::RotatingSession, None).await;
                if let Some(pending) = chunk.finish() {
                    ws.commit_chunk(pending, &state, &app).await?;
                }
                ws.close().await;
                ws = RealtimeConnection::connect(&http, &token).await?;
                session_started_at = Instant::now();
                update_status(&app, &state, TranscriptionStatus::Recording, None).await;
            }
        }
    }
}

fn next_rotation_deadline(session_started_at: Instant) -> Instant {
    session_started_at + ROTATE_AFTER
}

struct RealtimeConnection {
    session: SessionCredentials,
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

impl RealtimeConnection {
    async fn connect(http: &Client, token: &str) -> anyhow::Result<Self> {
        let session = create_transcription_session(http, token).await?;
        let mut request =
            "wss://api.openai.com/v1/realtime?intent=transcription".into_client_request()?;
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            format!(
                "realtime, openai-beta.realtime-v1, openai-insecure-api-key.{}",
                session.client_secret
            )
            .parse()?,
        );
        let (socket, _) = connect_async(request).await?;
        Ok(Self { session, socket })
    }

    async fn commit_chunk(
        &mut self,
        chunk: PendingChunk,
        state: &Arc<SharedState>,
        app: &AppHandle,
    ) -> anyhow::Result<()> {
        let audio = STANDARD.encode(&chunk.pcm);
        self.socket
            .send(
                json!({ "type": "input_audio_buffer.append", "audio": audio })
                    .to_string()
                    .into(),
            )
            .await?;
        self.socket
            .send(
                json!({ "type": "input_audio_buffer.commit" })
                    .to_string()
                    .into(),
            )
            .await?;

        let segment = self.wait_completed(chunk).await?;
        append_transcript_segment(&state.paths.output_directory, &segment)?;
        emit_snapshot(app, state, "transcript_segment_written").await;
        Ok(())
    }

    async fn wait_completed(&mut self, chunk: PendingChunk) -> anyhow::Result<TranscriptSegment> {
        let wait = async {
            while let Some(message) = self.socket.next().await {
                let message = message?;
                let text = match message {
                    Message::Text(text) => text,
                    Message::Close(_) => {
                        bail!("Realtime WebSocket closed before transcription completed")
                    }
                    _ => continue,
                };
                let value: serde_json::Value = serde_json::from_str(&text)?;
                match value.get("type").and_then(serde_json::Value::as_str) {
                    Some("conversation.item.input_audio_transcription.completed") => {
                        let item_id = value
                            .get("item_id")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let previous_item_id = value
                            .get("previous_item_id")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string);
                        let text = value
                            .get("transcript")
                            .or_else(|| value.get("text"))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();

                        return Ok(TranscriptSegment {
                            segment_type: "transcript_segment",
                            local_start: chunk.local_start,
                            local_end: chunk.local_end,
                            session_id: self.session.id.clone(),
                            item_id,
                            previous_item_id,
                            model: MODEL.to_string(),
                            text,
                            received_at: Local::now(),
                        });
                    }
                    Some("error") => {
                        bail!("Realtime API error while waiting for transcript: {}", value)
                    }
                    _ => {}
                }
            }

            bail!("Realtime WebSocket closed before transcription completed")
        };

        timeout(COMPLETION_WAIT, wait).await.with_context(|| {
            format!(
                "timed out waiting for completed transcript after {} seconds for chunk {}-{}",
                COMPLETION_WAIT.as_secs(),
                chunk.local_start.format("%H:%M:%S"),
                chunk.local_end.format("%H:%M:%S")
            )
        })?
    }

    async fn close(&mut self) {
        let _ = self.socket.close(None).await;
    }
}

struct ChunkBuilder {
    pcm: Vec<u8>,
    started_at: Option<DateTime<Local>>,
    duration: Duration,
}

impl ChunkBuilder {
    fn new() -> Self {
        Self {
            pcm: Vec::new(),
            started_at: None,
            duration: Duration::ZERO,
        }
    }

    fn push(&mut self, block: AudioBlock) -> anyhow::Result<Option<PendingChunk>> {
        if self.started_at.is_none() {
            self.started_at = Some(Local::now());
        }
        self.duration += block.duration;
        self.pcm.extend(block.pcm);

        if self.duration >= Duration::from_secs(CHUNK_SECONDS as u64) {
            return Ok(self.finish());
        }

        Ok(None)
    }

    fn finish(&mut self) -> Option<PendingChunk> {
        if self.pcm.is_empty() {
            return None;
        }

        let local_start = self.started_at.take()?;
        let local_end = Local::now();
        let pcm = std::mem::take(&mut self.pcm);
        self.duration = Duration::ZERO;
        Some(PendingChunk {
            pcm,
            local_start,
            local_end,
        })
    }
}

fn start_audio_capture(
    settings: Settings,
    sender: mpsc::Sender<AudioBlock>,
) -> anyhow::Result<AudioCapture> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let join = thread::spawn(move || {
        let result = (|| -> anyhow::Result<cpal::Stream> {
            let device = resolve_input_device(&settings)?;
            build_audio_stream(device, sender)
        })();

        match result {
            Ok(stream) => {
                let _ = ready_tx.send(Ok(()));
                let _stream = stream;
                let _ = stop_rx.recv();
            }
            Err(error) => {
                let _ = ready_tx.send(Err(format!("{error:#}")));
            }
        }
    });

    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(AudioCapture {
            stop: stop_tx,
            join: Some(join),
        }),
        Ok(Err(error)) => {
            let _ = join.join();
            bail!(error)
        }
        Err(error) => {
            let _ = stop_tx.send(());
            let _ = join.join();
            bail!("audio capture did not start: {error}")
        }
    }
}

fn build_audio_stream(
    device: cpal::Device,
    sender: mpsc::Sender<AudioBlock>,
) -> anyhow::Result<cpal::Stream> {
    let config = device.default_input_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let stream_config = config.config();
    let error_callback = |error| eprintln!("audio stream error: {error}");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| send_audio_block(data, sample_rate, channels, &sender),
            error_callback,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                let samples = data
                    .iter()
                    .map(|sample| *sample as f32 / i16::MAX as f32)
                    .collect::<Vec<_>>();
                send_audio_block(&samples, sample_rate, channels, &sender);
            },
            error_callback,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                let samples = data
                    .iter()
                    .map(|sample| (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0)
                    .collect::<Vec<_>>();
                send_audio_block(&samples, sample_rate, channels, &sender);
            },
            error_callback,
            None,
        )?,
        sample_format => bail!("unsupported input sample format: {sample_format:?}"),
    };

    stream.play()?;
    Ok(stream)
}

fn send_audio_block(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    sender: &mpsc::Sender<AudioBlock>,
) {
    let pcm = resample_to_pcm16_mono(samples, sample_rate, channels);
    let frames = pcm.len() / 2;
    let duration = Duration::from_secs_f64(frames as f64 / TARGET_SAMPLE_RATE as f64);
    let _ = sender.try_send(AudioBlock { pcm, duration });
}

fn resample_to_pcm16_mono(samples: &[f32], source_rate: u32, source_channels: u16) -> Vec<u8> {
    let channels = usize::from(source_channels.max(TARGET_CHANNELS));
    let frames = samples.len() / channels;
    if frames == 0 {
        return Vec::new();
    }

    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let start = frame * channels;
        let sum = samples[start..start + channels].iter().sum::<f32>();
        mono.push(sum / channels as f32);
    }

    let target_frames =
        ((frames as u64 * TARGET_SAMPLE_RATE as u64) / source_rate as u64).max(1) as usize;
    let mut pcm = Vec::with_capacity(target_frames * 2);
    for index in 0..target_frames {
        let source_index = (index as u64 * source_rate as u64 / TARGET_SAMPLE_RATE as u64) as usize;
        let sample = mono[source_index.min(mono.len() - 1)].clamp(-1.0, 1.0);
        let int_sample = (sample * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&int_sample.to_le_bytes());
    }
    pcm
}

async fn create_transcription_session(
    http: &Client,
    token: &str,
) -> anyhow::Result<SessionCredentials> {
    let response = http
        .post("https://api.openai.com/v1/realtime/transcription_sessions")
        .bearer_auth(token)
        .json(&json!({
            "input_audio_format": "pcm16",
            "input_audio_transcription": {
                "model": MODEL
            },
            "turn_detection": null
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("failed to create transcription session: HTTP {status}: {body}");
    }

    let session = response.json::<SessionResponse>().await?;
    Ok(SessionCredentials {
        id: session.id,
        client_secret: session.client_secret.value,
    })
}

fn read_chatgpt_token() -> anyhow::Result<String> {
    let path = dirs::home_dir()
        .context("home directory not found")?
        .join(".codex")
        .join("auth.json");
    let auth =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let auth: ChatGptAuth = serde_json::from_str(&auth)?;
    if auth.tokens.access_token.trim().is_empty() {
        bail!("~/.codex/auth.json does not contain tokens.access_token");
    }
    Ok(auth.tokens.access_token)
}

fn resolve_input_device(settings: &Settings) -> anyhow::Result<cpal::Device> {
    let host = cpal::default_host();
    match settings.input_device_mode {
        InputDeviceMode::SystemDefault => host
            .default_input_device()
            .context("default input device not found"),
        InputDeviceMode::FixedDevice => {
            let expected = settings
                .input_device_id
                .as_deref()
                .context("fixed input device is not selected")?;
            for (index, device) in host.input_devices()?.enumerate() {
                if stable_device_id(index, &device) == expected {
                    return Ok(device);
                }
            }
            bail!("fixed input device was not found: {expected}");
        }
    }
}

fn list_input_devices() -> anyhow::Result<Vec<InputDevice>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let devices = host
        .input_devices()?
        .enumerate()
        .map(|(index, device)| {
            let id = stable_device_id(index, &device);
            let name = device
                .name()
                .unwrap_or_else(|_| "Unknown input device".to_string());
            let is_default = default_name.as_ref() == Some(&name);
            InputDevice {
                id,
                name,
                is_default,
            }
        })
        .collect();
    Ok(devices)
}

fn stable_device_id(index: usize, device: &cpal::Device) -> String {
    let name = device.name().unwrap_or_else(|_| "unknown".to_string());
    let mut hasher = Sha256::new();
    hasher.update(index.to_le_bytes());
    hasher.update(name.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn build_paths<R: Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<AppPaths> {
    let config_dir = app.path().app_config_dir()?;
    let output_directory = dirs::document_dir()
        .or_else(dirs::home_dir)
        .context("Documents directory not found")?
        .join("Tategoto");
    Ok(AppPaths {
        config_file: config_dir.join("settings.json"),
        output_directory,
    })
}

fn load_settings(path: &PathBuf) -> anyhow::Result<Settings> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn save_settings(path: &PathBuf, settings: &Settings) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}

struct TranscriptPaths {
    markdown: PathBuf,
    jsonl: PathBuf,
}

fn transcript_paths(output_directory: &PathBuf, date: DateTime<Local>) -> TranscriptPaths {
    let file_stem = date.format("%Y-%m-%d").to_string();
    TranscriptPaths {
        markdown: output_directory.join(format!("{file_stem}.md")),
        jsonl: output_directory.join(format!("{file_stem}.jsonl")),
    }
}

fn ensure_daily_files(paths: &TranscriptPaths) -> anyhow::Result<()> {
    if let Some(parent) = paths.markdown.parent() {
        fs::create_dir_all(parent)?;
    }
    if !paths.markdown.exists() {
        let date = Local::now().format("%Y-%m-%d");
        fs::write(&paths.markdown, format!("# {date}\n\n"))?;
    }
    if !paths.jsonl.exists() {
        fs::write(&paths.jsonl, "")?;
    }
    Ok(())
}

fn append_transcript_segment(
    output_directory: &PathBuf,
    segment: &TranscriptSegment,
) -> anyhow::Result<()> {
    let paths = transcript_paths(output_directory, segment.local_start);
    ensure_daily_files(&paths)?;

    if !segment.text.is_empty() {
        let mut markdown = OpenOptions::new().append(true).open(&paths.markdown)?;
        let heading = format!("## {}\n", segment.local_start.format("%H:%M"));
        let markdown_content = fs::read_to_string(&paths.markdown)?;
        if !markdown_content.contains(&heading) {
            writeln!(markdown, "\n{}", heading.trim_end())?;
        }
        writeln!(
            markdown,
            "- [{}-{}] {}",
            segment.local_start.format("%H:%M:%S"),
            segment.local_end.format("%H:%M:%S"),
            segment.text
        )?;
    }

    let mut jsonl = OpenOptions::new().append(true).open(&paths.jsonl)?;
    writeln!(jsonl, "{}", serde_json::to_string(segment)?)?;
    Ok(())
}

async fn snapshot(state: &Arc<SharedState>) -> anyhow::Result<AppSnapshot> {
    let model = state.model.lock().await;
    let paths = transcript_paths(&state.paths.output_directory, Local::now());
    Ok(AppSnapshot {
        status: model.status.clone(),
        settings: model.settings.clone(),
        devices: list_input_devices()?,
        output_directory: state.paths.output_directory.to_string_lossy().to_string(),
        today_markdown_path: paths.markdown.to_string_lossy().to_string(),
        today_jsonl_path: paths.jsonl.to_string_lossy().to_string(),
        last_error: model.last_error.clone(),
    })
}

async fn update_status(
    app: &AppHandle,
    state: &Arc<SharedState>,
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

async fn set_error(app: &AppHandle, state: &Arc<SharedState>, error: String) {
    {
        let mut model = state.model.lock().await;
        model.status = TranscriptionStatus::StoppedWithError;
        model.last_error = Some(error);
        model.runtime = None;
    }
    update_tray_status(app, state).await;
    emit_snapshot(app, state, "transcription_error").await;
}

async fn emit_snapshot(app: &AppHandle, state: &Arc<SharedState>, event: &str) {
    update_tray_status(app, state).await;
    if let Ok(snapshot) = snapshot(state).await {
        let _ = app.emit(event, snapshot);
    }
}

async fn update_tray_status(app: &AppHandle, state: &Arc<SharedState>) {
    let status = {
        let model = state.model.lock().await;
        model.status.clone()
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let (marker, label) = tray_status_label(&status);
        let _ = tray.set_title(Some(format!("{marker} {label}")));
        let _ = tray.set_tooltip(Some(format!("Tategoto: {label}")));
    }
}

fn tray_status_label(status: &TranscriptionStatus) -> (&'static str, &'static str) {
    match status {
        TranscriptionStatus::Idle => ("○", "待機中"),
        TranscriptionStatus::Recording => ("●", "録音中"),
        TranscriptionStatus::RotatingSession => ("◐", "更新中"),
        TranscriptionStatus::StoppedWithError => ("!", "エラー"),
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start", "Start", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &start, &stop, &quit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .title("○ 待機中")
        .tooltip("Tategoto")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "start" => {
                if let Some(state) = app.try_state::<Arc<SharedState>>() {
                    let app = app.clone();
                    let state = state.inner().clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = start_from_tray(app, state).await;
                    });
                }
            }
            "stop" => {
                if let Some(state) = app.try_state::<Arc<SharedState>>() {
                    let app = app.clone();
                    let state = state.inner().clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = stop_from_tray(app, state).await;
                    });
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

async fn start_from_tray(app: AppHandle, state: Arc<SharedState>) -> anyhow::Result<()> {
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

async fn stop_from_tray(app: AppHandle, state: Arc<SharedState>) -> anyhow::Result<()> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_markdown_and_jsonl_transcript_segment() {
        let directory = tempfile::tempdir().expect("tempdir");
        let segment = TranscriptSegment {
            segment_type: "transcript_segment",
            local_start: DateTime::parse_from_rfc3339("2026-05-05T09:15:02+09:00")
                .unwrap()
                .with_timezone(&Local),
            local_end: DateTime::parse_from_rfc3339("2026-05-05T09:15:18+09:00")
                .unwrap()
                .with_timezone(&Local),
            session_id: "sess_test".to_string(),
            item_id: "item_test".to_string(),
            previous_item_id: None,
            model: MODEL.to_string(),
            text: "今日の作業を始めます。".to_string(),
            received_at: DateTime::parse_from_rfc3339("2026-05-05T09:15:19+09:00")
                .unwrap()
                .with_timezone(&Local),
        };

        append_transcript_segment(&directory.path().to_path_buf(), &segment)
            .expect("write segment");

        let markdown =
            fs::read_to_string(directory.path().join("2026-05-05.md")).expect("read markdown");
        let jsonl =
            fs::read_to_string(directory.path().join("2026-05-05.jsonl")).expect("read jsonl");
        assert!(markdown.contains("## 09:15"));
        assert!(markdown.contains("- [09:15:02-09:15:18] 今日の作業を始めます。"));
        assert!(jsonl.contains("\"session_id\":\"sess_test\""));
        assert!(jsonl.contains("\"previous_item_id\":null"));
    }

    #[test]
    fn resamples_stereo_f32_to_pcm16_mono_24khz() {
        let source = vec![0.5_f32, 0.5, -0.5, -0.5, 0.25, 0.25, -0.25, -0.25];
        let pcm = resample_to_pcm16_mono(&source, 48_000, 2);
        assert_eq!(pcm.len(), 4);
    }

    #[test]
    fn fixed_device_does_not_fallback_to_default_when_missing() {
        let settings = Settings {
            input_device_mode: InputDeviceMode::FixedDevice,
            input_device_id: Some("missing-device-id".to_string()),
            input_device_name: Some("Missing Device".to_string()),
        };

        let error = match resolve_input_device(&settings) {
            Ok(_) => panic!("fixed missing device should fail"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("fixed input device was not found")
        );
    }

    #[test]
    fn rotation_deadline_is_50_minutes_after_session_start() {
        let started_at = Instant::now();
        assert_eq!(
            next_rotation_deadline(started_at).duration_since(started_at),
            ROTATE_AFTER
        );
    }

    #[test]
    #[ignore = "requires a macOS input device and microphone permission"]
    fn captures_default_input_device_audio_block() {
        let (sender, mut receiver) = mpsc::channel::<AudioBlock>(8);
        let capture =
            start_audio_capture(Settings::default(), sender).expect("start audio capture");
        let block = receiver
            .blocking_recv()
            .expect("audio callback should produce a block");
        capture.stop();

        assert!(!block.pcm.is_empty());
        assert!(block.duration > Duration::ZERO);
    }
}
