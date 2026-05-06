use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_char, c_void},
    ptr::NonNull,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{audio::AudioBlock, model::TranscriptionSettings};

static NEXT_CALLBACK_ID: AtomicUsize = AtomicUsize::new(1);
static CALLBACKS: OnceLock<
    Mutex<HashMap<usize, mpsc::UnboundedSender<anyhow::Result<AppleSpeechEvent>>>>,
> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
struct AppleSpeechConfig {
    locale_identifier: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum AppleSpeechEvent {
    Ready,
    Segment(AppleSpeechSegment),
    Error { message: String },
    Stopped,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AppleSpeechSegment {
    pub(crate) session_id: String,
    pub(crate) item_id: String,
    pub(crate) previous_item_id: Option<String>,
    pub(crate) text: String,
    pub(crate) start_offset_secs: f64,
    pub(crate) end_offset_secs: f64,
}

pub(crate) struct AppleSpeechConnection {
    handle: NonNull<c_void>,
    callback_id: usize,
    stopped: bool,
}

// The opaque Swift session is only touched through thread-safe exported bridge
// functions, and callbacks cross back into Rust through a tokio channel.
unsafe impl Send for AppleSpeechConnection {}

impl AppleSpeechConnection {
    pub(crate) fn start(
        settings: &TranscriptionSettings,
    ) -> anyhow::Result<(
        Self,
        mpsc::UnboundedReceiver<anyhow::Result<AppleSpeechEvent>>,
    )> {
        let config = AppleSpeechConfig {
            locale_identifier: trimmed_non_empty(settings.locale_identifier.as_deref())
                .map(ToOwned::to_owned),
        };
        let config = CString::new(serde_json::to_string(&config)?)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let callback_id = register_callback(sender)?;

        let handle = unsafe {
            tategoto_speech_start(
                config.as_ptr(),
                Some(handle_speech_event),
                encode_callback_id(callback_id),
            )
        };
        let Some(handle) = NonNull::new(handle) else {
            unregister_callback(callback_id);
            bail!("Apple SpeechTranscriber を開始できませんでした");
        };

        Ok((
            Self {
                handle,
                callback_id,
                stopped: false,
            },
            receiver,
        ))
    }

    pub(crate) fn append_audio(&self, block: &AudioBlock) {
        if block.pcm.is_empty() {
            return;
        }
        unsafe {
            tategoto_speech_append_pcm16(self.handle.as_ptr(), block.pcm.as_ptr(), block.pcm.len());
        }
    }

    pub(crate) fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        unsafe {
            tategoto_speech_stop(self.handle.as_ptr());
        }
    }
}

impl Drop for AppleSpeechConnection {
    fn drop(&mut self) {
        self.stop();
        unregister_callback(self.callback_id);
    }
}

extern "C" fn handle_speech_event(message: *const c_char, user_data: *mut c_void) {
    let callback_id = decode_callback_id(user_data);
    let Some(sender) = callback_sender(callback_id) else {
        return;
    };
    let event = parse_event(message);
    let _ = sender.send(event);
}

fn register_callback(
    sender: mpsc::UnboundedSender<anyhow::Result<AppleSpeechEvent>>,
) -> anyhow::Result<usize> {
    let callback_id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed);
    if callback_id == 0 {
        bail!("Apple SpeechTranscriber callback id overflowed");
    }
    callback_registry()
        .lock()
        .map_err(|_| anyhow::anyhow!("Apple SpeechTranscriber callback registry is poisoned"))?
        .insert(callback_id, sender);
    Ok(callback_id)
}

fn unregister_callback(callback_id: usize) {
    if let Ok(mut callbacks) = callback_registry().lock() {
        callbacks.remove(&callback_id);
    }
}

fn callback_sender(
    callback_id: usize,
) -> Option<mpsc::UnboundedSender<anyhow::Result<AppleSpeechEvent>>> {
    callback_registry()
        .lock()
        .ok()
        .and_then(|callbacks| callbacks.get(&callback_id).cloned())
}

fn callback_registry()
-> &'static Mutex<HashMap<usize, mpsc::UnboundedSender<anyhow::Result<AppleSpeechEvent>>>> {
    CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn encode_callback_id(callback_id: usize) -> *mut c_void {
    (callback_id << 3) as *mut c_void
}

fn decode_callback_id(user_data: *mut c_void) -> usize {
    (user_data as usize) >> 3
}

fn parse_event(message: *const c_char) -> anyhow::Result<AppleSpeechEvent> {
    if message.is_null() {
        bail!("Apple SpeechTranscriber から空のイベントを受信しました");
    }
    let message = unsafe { CStr::from_ptr(message) }
        .to_str()
        .context("Apple SpeechTranscriber event is not UTF-8")?;
    Ok(serde_json::from_str(message)?)
}

fn trimmed_non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(not(tategoto_stub_speech))]
unsafe extern "C" {
    fn tategoto_speech_start(
        config_json: *const c_char,
        callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
        user_data: *mut c_void,
    ) -> *mut c_void;
    fn tategoto_speech_append_pcm16(handle: *mut c_void, pcm: *const u8, len: usize);
    fn tategoto_speech_stop(handle: *mut c_void);
}

#[cfg(tategoto_stub_speech)]
unsafe fn tategoto_speech_start(
    _config_json: *const c_char,
    _callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
    _user_data: *mut c_void,
) -> *mut c_void {
    std::ptr::dangling_mut::<c_void>()
}

#[cfg(tategoto_stub_speech)]
unsafe fn tategoto_speech_append_pcm16(_handle: *mut c_void, _pcm: *const u8, _len: usize) {}

#[cfg(tategoto_stub_speech)]
unsafe fn tategoto_speech_stop(_handle: *mut c_void) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_segment_event_from_swift_bridge() {
        let payload = CString::new(
            r#"{"type":"segment","session_id":"apple-session","item_id":"item_1","previous_item_id":null,"text":"こんにちは","start_offset_secs":0.5,"end_offset_secs":1.25}"#,
        )
        .unwrap();

        let AppleSpeechEvent::Segment(segment) = parse_event(payload.as_ptr()).unwrap() else {
            panic!("expected segment event");
        };

        assert_eq!(segment.session_id, "apple-session");
        assert_eq!(segment.item_id, "item_1");
        assert_eq!(segment.text, "こんにちは");
        assert!((segment.start_offset_secs - 0.5).abs() < f64::EPSILON);
        assert!((segment.end_offset_secs - 1.25).abs() < f64::EPSILON);
    }

    #[test]
    fn callback_ignores_events_after_registration_is_removed() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let callback_id = register_callback(sender).unwrap();
        unregister_callback(callback_id);

        let payload = CString::new(r#"{"type":"ready"}"#).unwrap();
        handle_speech_event(payload.as_ptr(), encode_callback_id(callback_id));

        assert!(receiver.try_recv().is_err());
    }
}
