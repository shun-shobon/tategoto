use std::{sync::mpsc as std_mpsc, thread, time::Duration};

use anyhow::{Context, bail};
use chrono::{DateTime, Local};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::{
    TARGET_CHANNELS, TARGET_SAMPLE_RATE,
    model::{InputDevice, InputDeviceMode, Settings},
};

#[derive(Debug)]
pub(crate) struct AudioBlock {
    pub(crate) pcm: Vec<u8>,
    pub(crate) captured_at: DateTime<Local>,
    pub(crate) duration: Duration,
}

impl AudioBlock {
    fn new(pcm: Vec<u8>, captured_at: DateTime<Local>) -> Self {
        Self {
            duration: pcm16_duration(&pcm),
            pcm,
            captured_at,
        }
    }
}

#[derive(Debug)]
pub(crate) struct AudioCapture {
    stop: std_mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
}

impl AudioCapture {
    pub(crate) fn stop(mut self) {
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

pub(crate) fn start_audio_capture(
    settings: Settings,
    sender: mpsc::Sender<AudioBlock>,
) -> anyhow::Result<AudioCapture> {
    let (ready_tx, ready_rx) = std_mpsc::channel::<Result<(), String>>();
    let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
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

pub(crate) fn list_input_devices() -> anyhow::Result<Vec<InputDevice>> {
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

fn stable_device_id(index: usize, device: &cpal::Device) -> String {
    let name = device.name().unwrap_or_else(|_| "unknown".to_string());
    let mut hasher = Sha256::new();
    hasher.update(index.to_le_bytes());
    hasher.update(name.as_bytes());
    format!("{:x}", hasher.finalize())
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
    if pcm.is_empty() {
        return;
    }
    let _ = sender.try_send(AudioBlock::new(pcm, Local::now()));
}

fn pcm16_duration(pcm: &[u8]) -> Duration {
    let frames = pcm.len() / 2;
    Duration::from_secs_f64(frames as f64 / TARGET_SAMPLE_RATE as f64)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_stereo_f32_to_pcm16_mono_24khz() {
        let source = vec![0.5_f32, 0.5, -0.5, -0.5, 0.25, 0.25, -0.25, -0.25];
        let pcm = resample_to_pcm16_mono(&source, 48_000, 2);
        assert_eq!(pcm.len(), 4);
    }

    #[test]
    fn audio_block_tracks_pcm16_duration() {
        let pcm = vec![0_u8; 4_800];
        let block = AudioBlock::new(pcm, Local::now());
        assert_eq!(block.duration, Duration::from_millis(100));
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
    }
}
