# ChatGPT Auth Realtime Transcription

This note summarizes how transcription can be exercised with the ChatGPT bearer
token stored in `~/.codex/auth.json`.

Status as of 2026-05-05:

- The old REST endpoint `https://chatgpt.com/backend-api/transcribe` no longer
  worked in local testing. It returned Cloudflare HTML `403` for the historical
  request shape.
- The current working path is the Realtime transcription session API:
  `POST https://api.openai.com/v1/realtime/transcription_sessions`.
- Both `gpt-4o-transcribe` and `gpt-4o-mini-transcribe` accepted the ChatGPT
  bearer token and created transcription sessions.
- `gpt-4o-transcribe-diarize` did not work with this ChatGPT bearer token in
  local testing. Realtime session creation returned `400 invalid_parameter`
  with `Your organization does not have access to this transcription model`.
- The transcription session endpoint returns a beta Realtime client secret. When
  connecting over WebSocket, include the `openai-beta.realtime-v1` subprotocol.

## Auth Material

Read the ChatGPT token and account ID from `~/.codex/auth.json`:

```sh
token=$(jq -r '.tokens.access_token' ~/.codex/auth.json)
account=$(jq -r '.tokens.account_id // .tokens.id_token.chatgpt_account_id // empty' ~/.codex/auth.json)
```

Do not log either value. The `account` value is not needed for the public
Realtime transcription session endpoint, but it is useful when comparing against
ChatGPT backend endpoints.

## Historical Endpoint

The removed TUI voice transcription feature used:

```text
POST https://chatgpt.com/backend-api/transcribe
Authorization: Bearer <ChatGPT token>
ChatGPT-Account-Id: <account id>
Content-Type: multipart/form-data

file=@audio.wav
```

For API-key auth, the same old implementation used
`https://api.openai.com/v1/audio/transcriptions` with a `model` form field.

Local testing with the ChatGPT bearer token returned Cloudflare HTML `403` for:

- no `model` field
- `model=gpt-4o-transcribe`
- `model=gpt-4o-mini-transcribe`

The same token reached `https://chatgpt.com/backend-api/codex/models`, so the
failure was specific to the old transcription route rather than basic auth.

## Long Audio Limits

There are two different transcription paths with different constraints:

- HTTP file upload: `POST https://api.openai.com/v1/audio/transcriptions`
- Realtime transcription: `POST /v1/realtime/transcription_sessions`, then
  stream audio over WebSocket or WebRTC

For HTTP file upload, the documented hard limit is file size, not duration. The
Transcriptions API accepts files under 25 MB. For longer recordings, the docs
recommend either using a compressed format or splitting the audio into chunks of
25 MB or less, preferably without cutting mid-sentence. A one-hour uncompressed
PCM WAV file is far above this limit, but a compressed one-hour file can fit if
the bitrate is low enough. For example, 48 kbps is about 21.6 MB per hour, while
64 kbps is about 28.8 MB per hour.

For the ChatGPT bearer token path tested here, HTTP file upload should not be
treated as working: the old ChatGPT REST route returned `403`, and the public
`/v1/audio/transcriptions` route returned `500 internal_error` with this bearer.

For Realtime transcription, the documented Realtime session duration limit is 60
minutes. That means one hour is the upper boundary of a single connected session,
not a comfortable target. For robust handling, create a new session before the
limit is reached, such as every 50 to 55 minutes, and stitch transcripts by
local timestamps.

Do not send a one-hour recording as a single committed audio item. The
`gpt-4o-transcribe` model card lists a 16,000 token context window and 2,000 max
output tokens, and one hour of ordinary speech can exceed that output size. Use
VAD or manual chunking so each `input_audio_buffer.commit` covers a short
utterance or a few minutes at most. This keeps each
`conversation.item.input_audio_transcription.completed` event small and preserves
ordering through `item_id` and `previous_item_id`.

Practical recommendation for one-hour prerecorded audio with the ChatGPT bearer
token:

1. Use Realtime transcription, because it is the path verified with this auth.
2. Decode the source file locally and stream PCM16 mono 24 kHz chunks.
3. Commit frequently, either at VAD-detected pauses or fixed windows with overlap.
4. Start a replacement Realtime session before 60 minutes if the source may run
   longer than the session limit.
5. Store local offsets per committed item, then concatenate completed transcripts
   in `previous_item_id` order.

## Create A Realtime Transcription Session

Create a session with the desired transcription model:

```sh
curl -sS https://api.openai.com/v1/realtime/transcription_sessions \
  -H "Authorization: Bearer $token" \
  -H "Content-Type: application/json" \
  -d '{
    "input_audio_format": "pcm16",
    "input_audio_transcription": {
      "model": "gpt-4o-mini-transcribe"
    },
    "turn_detection": null
  }'
```

`gpt-4o-transcribe` can be used in the same field:

```json
{
  "input_audio_transcription": {
    "model": "gpt-4o-transcribe"
  }
}
```

Successful responses have `object: "realtime.transcription_session"` and include
`client_secret.value`. Treat that value as sensitive. It is an ephemeral key for
the WebSocket client connection.

### Diarization Model Check

`gpt-4o-transcribe-diarize` is the speaker diarization transcription model. The
public API documentation describes speaker annotations on the HTTP
`/v1/audio/transcriptions` endpoint when `response_format=diarized_json` is used;
for inputs longer than 30 seconds, `chunking_strategy=auto` is required.

With the ChatGPT bearer token from `~/.codex/auth.json`, the Realtime session API
rejected this model before any audio could be sent:

```text
POST https://api.openai.com/v1/realtime/transcription_sessions
model: gpt-4o-transcribe-diarize

HTTP 400
error.param: input_audio_transcription.model
error.code: invalid_parameter
error.message: Your organization does not have access to this transcription model.
```

The same audio and WebSocket code path completed successfully with
`gpt-4o-transcribe`, so the failure was not caused by the sample audio or the
client-side streaming flow.

The HTTP transcription endpoint was also checked with the documented diarization
request shape:

```text
POST https://api.openai.com/v1/audio/transcriptions
model: gpt-4o-transcribe-diarize
response_format: diarized_json
chunking_strategy: auto
file: <wav>
```

That returned `HTTP 500 internal_error` with the ChatGPT bearer token. The same
endpoint also returned `HTTP 500 internal_error` for `gpt-4o-transcribe` and
`gpt-4o-mini-transcribe`, so this REST path should not be treated as validated
for ChatGPT bearer auth.

## Stream Audio Over WebSocket

The session API is not a file-upload transcription endpoint. After creating the
session, connect to Realtime and stream audio chunks:

```text
wss://api.openai.com/v1/realtime?intent=transcription
```

Use WebSocket subprotocols:

```text
realtime
openai-beta.realtime-v1
openai-insecure-api-key.<client_secret.value>
```

The beta subprotocol is required for secrets created by
`/v1/realtime/transcription_sessions`. Without it, the server returns an API
version mismatch error.

Send PCM16 mono 24 kHz audio as base64 chunks:

```json
{ "type": "input_audio_buffer.append", "audio": "<base64 pcm16 audio>" }
```

Commit the current buffer when a segment is ready:

```json
{ "type": "input_audio_buffer.commit" }
```

Expected server events:

```text
transcription_session.created
input_audio_buffer.committed
conversation.item.created
conversation.item.input_audio_transcription.delta
conversation.item.input_audio_transcription.completed
```

`delta` events provide partial text. The `completed` event contains the final
transcript for the committed audio item.

## Minimal Node Check

Node 24 has a built-in `WebSocket`, so this can run without installing `ws`.
The example expects a WAV file containing PCM16 mono 24 kHz audio. It strips the
44-byte WAV header before sending the PCM payload.

```js
const fs = require("fs");

const auth = JSON.parse(fs.readFileSync(`${process.env.HOME}/.codex/auth.json`, "utf8"));
const bearer = auth.tokens.access_token;
const wav = fs.readFileSync("/private/tmp/audio.wav");
const pcmBase64 = wav.subarray(44).toString("base64");

async function createSession(model) {
  const res = await fetch("https://api.openai.com/v1/realtime/transcription_sessions", {
    method: "POST",
    headers: {
      authorization: `Bearer ${bearer}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      input_audio_format: "pcm16",
      input_audio_transcription: { model },
      turn_detection: null,
    }),
  });

  if (!res.ok) {
    throw new Error(`session create failed: ${res.status} ${await res.text()}`);
  }

  return (await res.json()).client_secret.value;
}

async function transcribe(model) {
  const secret = await createSession(model);
  const ws = new WebSocket("wss://api.openai.com/v1/realtime?intent=transcription", [
    "realtime",
    "openai-beta.realtime-v1",
    `openai-insecure-api-key.${secret}`,
  ]);

  ws.addEventListener("open", () => {
    ws.send(JSON.stringify({ type: "input_audio_buffer.append", audio: pcmBase64 }));
    ws.send(JSON.stringify({ type: "input_audio_buffer.commit" }));
  });

  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    if (msg.type === "conversation.item.input_audio_transcription.delta") {
      process.stdout.write(msg.delta);
    }
    if (msg.type === "conversation.item.input_audio_transcription.completed") {
      console.log(`\nfinal: ${msg.transcript}`);
      ws.close();
    }
    if (msg.type === "error") {
      console.error(msg.error);
      ws.close();
    }
  });
}

transcribe("gpt-4o-mini-transcribe").catch((error) => {
  console.error(error);
  process.exit(1);
});
```

## Practical Notes

- This path is stream-oriented. It can process prerecorded audio, but the client
  must split or append audio data over WebSocket and wait for transcript events.
- `input_audio_buffer.commit` requires at least 100 ms of audio.
- The tested format was PCM16, mono, 24 kHz, little-endian.
- `gpt-4o-transcribe` and `gpt-4o-mini-transcribe` both worked for session
  creation and WebSocket transcription flow. Test output depends on the audio
  content; non-speech audio may return an empty transcript.
- `gpt-4o-transcribe-diarize` could not be validated with this bearer token. To
  verify speaker labels, the expected successful response shape is
  `diarized_json` with `segments[]` containing `speaker`, `start`, `end`, and
  `text`, but local testing did not reach that response.
