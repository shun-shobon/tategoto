use std::fs;

use anyhow::{Context, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Local;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

use crate::{COMPLETION_WAIT, MODEL, model::PendingChunk, transcript::TranscriptSegment};

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

    pub(crate) async fn transcribe_chunk(
        &mut self,
        chunk: PendingChunk,
    ) -> anyhow::Result<TranscriptSegment> {
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

        self.wait_completed(chunk).await
    }

    pub(crate) async fn close(&mut self) {
        let _ = self.socket.close(None).await;
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
