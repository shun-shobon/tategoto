use std::collections::HashMap;
use std::fs;

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

use crate::{MODEL, transcript::TranscriptSegment};

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
pub(crate) struct RealtimeTimeline {
    session_started_at: DateTime<Local>,
    turns: HashMap<String, TurnTiming>,
}

impl RealtimeTimeline {
    pub(crate) fn new() -> Self {
        Self {
            session_started_at: Local::now(),
            turns: HashMap::new(),
        }
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

    pub(crate) async fn close(&mut self) {
        let _ = self.socket.close(None).await;
    }

    pub(crate) async fn next_transcript_segment(
        &mut self,
        timeline: &mut RealtimeTimeline,
    ) -> anyhow::Result<Option<TranscriptSegment>> {
        let Some(message) = self.socket.next().await else {
            bail!("Realtime WebSocket closed")
        };
        let message = message?;
        let text = match message {
            Message::Text(text) => text,
            Message::Close(_) => bail!("Realtime WebSocket closed"),
            _ => return Ok(None),
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
) -> anyhow::Result<Option<TranscriptSegment>> {
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some("input_audio_buffer.speech_started") => {
            let item_id = event_item_id(&value)?;
            timeline.turns.entry(item_id).or_default().start_ms = value
                .get("audio_start_ms")
                .and_then(serde_json::Value::as_i64);
            Ok(None)
        }
        Some("input_audio_buffer.speech_stopped") => {
            let item_id = event_item_id(&value)?;
            timeline.turns.entry(item_id).or_default().end_ms = value
                .get("audio_end_ms")
                .and_then(serde_json::Value::as_i64);
            Ok(None)
        }
        Some("input_audio_buffer.committed") => {
            let item_id = event_item_id(&value)?;
            timeline.turns.entry(item_id).or_default().previous_item_id = value
                .get("previous_item_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            Ok(None)
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

            Ok(Some(TranscriptSegment {
                segment_type: "transcript_segment",
                local_start: audio_offset_to_local_time(
                    timeline.session_started_at,
                    timing.start_ms,
                ),
                local_end: audio_offset_to_local_time(timeline.session_started_at, timing.end_ms),
                session_id: session_id.to_string(),
                item_id,
                previous_item_id: timing.previous_item_id,
                model: MODEL.to_string(),
                text,
                received_at: Local::now(),
            }))
        }
        Some("error") => bail!("Realtime API error: {}", value),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_transcript_segment_from_server_vad_events() {
        let mut timeline = RealtimeTimeline {
            session_started_at: DateTime::parse_from_rfc3339("2026-05-05T09:00:00+09:00")
                .unwrap()
                .with_timezone(&Local),
            turns: HashMap::new(),
        };

        assert!(
            handle_server_event(
                json!({
                    "type": "input_audio_buffer.speech_started",
                    "item_id": "item_test",
                    "audio_start_ms": 1_000
                }),
                "sess_test",
                &mut timeline,
            )
            .unwrap()
            .is_none()
        );
        assert!(
            handle_server_event(
                json!({
                    "type": "input_audio_buffer.speech_stopped",
                    "item_id": "item_test",
                    "audio_end_ms": 2_500
                }),
                "sess_test",
                &mut timeline,
            )
            .unwrap()
            .is_none()
        );
        assert!(
            handle_server_event(
                json!({
                    "type": "input_audio_buffer.committed",
                    "item_id": "item_test",
                    "previous_item_id": "item_previous"
                }),
                "sess_test",
                &mut timeline,
            )
            .unwrap()
            .is_none()
        );
        let segment = handle_server_event(
            json!({
                "type": "conversation.item.input_audio_transcription.completed",
                "item_id": "item_test",
                "transcript": "  こんにちは  "
            }),
            "sess_test",
            &mut timeline,
        )
        .unwrap()
        .expect("segment");

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
}
