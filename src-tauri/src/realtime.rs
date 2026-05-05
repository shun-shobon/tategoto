use std::{collections::HashMap, fs, time::Duration};

use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Duration as ChronoDuration, Local};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

use crate::{MODEL, audio::AudioBlock, transcript::TranscriptSegment};

pub(crate) const MIN_COMMIT_AUDIO_DURATION: Duration = Duration::from_millis(100);

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

#[derive(Debug, Deserialize)]
struct ChatGptAuth {
    tokens: ChatGptTokens,
}

#[derive(Debug, Deserialize)]
struct ChatGptTokens {
    access_token: String,
}

pub(crate) struct RealtimeConnection {
    session: SessionCredentials,
    socket: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
}

#[derive(Debug)]
pub(crate) enum RealtimeEvent {
    TranscriptSegment(TranscriptSegment),
    Committed,
    CommitRejected(String),
    ApiError(String),
    Ignored,
}

#[derive(Debug)]
pub(crate) struct RealtimeTimeline {
    audio_origin: Option<DateTime<Local>>,
    pending_audio_duration: Duration,
    turns: HashMap<String, TurnTiming>,
}

impl RealtimeTimeline {
    pub(crate) fn new() -> Self {
        Self {
            audio_origin: None,
            pending_audio_duration: Duration::ZERO,
            turns: HashMap::new(),
        }
    }

    pub(crate) fn record_audio_block(&mut self, block: &AudioBlock) {
        if self.audio_origin.is_none() {
            self.audio_origin = Some(block.captured_at);
        }
        self.pending_audio_duration += block.duration;
    }

    pub(crate) fn has_committable_audio(&self) -> bool {
        self.pending_audio_duration >= MIN_COMMIT_AUDIO_DURATION
    }

    pub(crate) fn has_pending_turns(&self) -> bool {
        !self.turns.is_empty()
    }

    fn reset_pending_audio(&mut self) {
        self.pending_audio_duration = Duration::ZERO;
    }

    fn audio_origin(&self) -> DateTime<Local> {
        self.audio_origin.unwrap_or_else(Local::now)
    }
}

#[derive(Debug, Default)]
struct TurnTiming {
    start_ms: Option<i64>,
    end_ms: Option<i64>,
    previous_item_id: Option<String>,
}

impl RealtimeConnection {
    pub(crate) async fn connect(http: &Client, token: &str) -> anyhow::Result<Self> {
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

    pub(crate) async fn append_audio(&mut self, pcm: Vec<u8>) -> anyhow::Result<()> {
        let audio = STANDARD.encode(&pcm);
        self.socket
            .send(
                json!({ "type": "input_audio_buffer.append", "audio": audio })
                    .to_string()
                    .into(),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn commit_audio(&mut self) -> anyhow::Result<()> {
        self.socket
            .send(
                json!({ "type": "input_audio_buffer.commit" })
                    .to_string()
                    .into(),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn close(&mut self) {
        let _ = self.socket.close(None).await;
    }

    pub(crate) async fn next_event(
        &mut self,
        timeline: &mut RealtimeTimeline,
    ) -> anyhow::Result<RealtimeEvent> {
        let Some(message) = self.socket.next().await else {
            bail!("Realtime WebSocket closed")
        };
        let message = message?;
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => bail!("Realtime WebSocket closed"),
            _ => return Ok(RealtimeEvent::Ignored),
        };
        let value: serde_json::Value = serde_json::from_str(&text)?;
        handle_server_event(value, &self.session.id, timeline)
    }
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
            "turn_detection": {
                "type": "server_vad",
                "threshold": 0.5,
                "prefix_padding_ms": 300,
                "silence_duration_ms": 700
            }
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

pub(crate) fn read_chatgpt_token() -> anyhow::Result<String> {
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

fn event_item_id(value: &serde_json::Value) -> anyhow::Result<String> {
    Ok(value
        .get("item_id")
        .and_then(serde_json::Value::as_str)
        .context("Realtime event did not include item_id")?
        .to_string())
}

fn audio_offset_to_local_time(
    session_started_at: DateTime<Local>,
    offset_ms: Option<i64>,
) -> DateTime<Local> {
    offset_ms
        .and_then(ChronoDuration::try_milliseconds)
        .and_then(|offset| session_started_at.checked_add_signed(offset))
        .unwrap_or_else(Local::now)
}

fn handle_server_event(
    value: serde_json::Value,
    session_id: &str,
    timeline: &mut RealtimeTimeline,
) -> anyhow::Result<RealtimeEvent> {
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("input_audio_buffer.speech_started") => {
            let item_id = event_item_id(&value)?;
            timeline.turns.entry(item_id).or_default().start_ms = value
                .get("audio_start_ms")
                .and_then(serde_json::Value::as_i64);
            Ok(RealtimeEvent::Ignored)
        }
        Some("input_audio_buffer.speech_stopped") => {
            let item_id = event_item_id(&value)?;
            timeline.turns.entry(item_id).or_default().end_ms = value
                .get("audio_end_ms")
                .and_then(serde_json::Value::as_i64);
            Ok(RealtimeEvent::Ignored)
        }
        Some("input_audio_buffer.committed") => {
            let item_id = event_item_id(&value)?;
            timeline.reset_pending_audio();
            timeline.turns.entry(item_id).or_default().previous_item_id = value
                .get("previous_item_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            Ok(RealtimeEvent::Committed)
        }
        Some("conversation.item.input_audio_transcription.completed") => {
            let item_id = event_item_id(&value)?;
            let timing = timeline.turns.remove(&item_id).unwrap_or_default();
            let text = value
                .get("transcript")
                .or_else(|| value.get("text"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();

            Ok(RealtimeEvent::TranscriptSegment(TranscriptSegment {
                segment_type: "transcript_segment",
                local_start: audio_offset_to_local_time(timeline.audio_origin(), timing.start_ms),
                local_end: audio_offset_to_local_time(timeline.audio_origin(), timing.end_ms),
                session_id: session_id.to_string(),
                item_id,
                previous_item_id: timing.previous_item_id,
                model: MODEL.to_string(),
                text,
                received_at: Local::now(),
            }))
        }
        Some("error") => {
            let message = realtime_error_message(&value);
            if is_commit_rejected_error(&message) {
                Ok(RealtimeEvent::CommitRejected(message))
            } else {
                Ok(RealtimeEvent::ApiError(message))
            }
        }
        _ => Ok(RealtimeEvent::Ignored),
    }
}

fn realtime_error_message(value: &serde_json::Value) -> String {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

fn is_commit_rejected_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("buffer")
        && (message.contains("empty")
            || message.contains("too small")
            || message.contains("100 ms")
            || message.contains("100ms")
            || message.contains("at least 100"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_transcript_segment_from_server_vad_events() {
        let mut timeline = RealtimeTimeline {
            audio_origin: Some(
                DateTime::parse_from_rfc3339("2026-05-05T09:00:00+09:00")
                    .unwrap()
                    .with_timezone(&Local),
            ),
            pending_audio_duration: Duration::from_millis(2500),
            turns: HashMap::new(),
        };

        assert!(matches!(
            handle_server_event(
                json!({
                    "type": "input_audio_buffer.speech_started",
                    "item_id": "item_test",
                    "audio_start_ms": 1_000
                }),
                "sess_test",
                &mut timeline,
            )
            .unwrap(),
            RealtimeEvent::Ignored
        ));
        assert!(matches!(
            handle_server_event(
                json!({
                    "type": "input_audio_buffer.speech_stopped",
                    "item_id": "item_test",
                    "audio_end_ms": 2_500
                }),
                "sess_test",
                &mut timeline,
            )
            .unwrap(),
            RealtimeEvent::Ignored
        ));
        assert!(matches!(
            handle_server_event(
                json!({
                    "type": "input_audio_buffer.committed",
                    "item_id": "item_test",
                    "previous_item_id": "item_previous"
                }),
                "sess_test",
                &mut timeline,
            )
            .unwrap(),
            RealtimeEvent::Committed
        ));
        assert_eq!(timeline.pending_audio_duration, Duration::ZERO);

        let RealtimeEvent::TranscriptSegment(segment) = handle_server_event(
            json!({
                "type": "conversation.item.input_audio_transcription.completed",
                "item_id": "item_test",
                "transcript": "  こんにちは  "
            }),
            "sess_test",
            &mut timeline,
        )
        .unwrap() else {
            panic!("expected transcript segment");
        };

        assert_eq!(segment.session_id, "sess_test");
        assert_eq!(segment.item_id, "item_test");
        assert_eq!(segment.previous_item_id, Some("item_previous".to_string()));
        assert_eq!(segment.text, "こんにちは");
        assert_eq!(
            segment.local_start.format("%H:%M:%S").to_string(),
            "09:00:01"
        );
        assert_eq!(segment.local_end.format("%H:%M:%S").to_string(), "09:00:02");
    }

    #[test]
    fn tracks_committable_audio_duration() {
        let mut timeline = RealtimeTimeline::new();
        let block = AudioBlock {
            pcm: vec![0; 4_800],
            captured_at: DateTime::parse_from_rfc3339("2026-05-05T09:00:00+09:00")
                .unwrap()
                .with_timezone(&Local),
            duration: Duration::from_millis(100),
        };

        timeline.record_audio_block(&block);

        assert!(timeline.has_committable_audio());
        assert_eq!(
            timeline.audio_origin().format("%H:%M:%S").to_string(),
            "09:00:00"
        );
    }

    #[test]
    fn classifies_too_short_commit_error_as_rejected_commit() {
        let mut timeline = RealtimeTimeline::new();

        let event = handle_server_event(
            json!({
                "type": "error",
                "error": {
                    "message": "Input audio buffer is too small. Expected at least 100ms of audio."
                }
            }),
            "sess_test",
            &mut timeline,
        )
        .unwrap();

        assert!(matches!(event, RealtimeEvent::CommitRejected(_)));
    }
}
