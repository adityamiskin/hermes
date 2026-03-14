use crate::config::AppConfig;
use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use serde::Serialize;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Clone)]
pub struct AudioChunkSink {
    sender: Arc<dyn Fn(Vec<f32>, u32) + Send + Sync>,
}

impl AudioChunkSink {
    pub fn new<F>(sender: F) -> Self
    where
        F: Fn(Vec<f32>, u32) + Send + Sync + 'static,
    {
        Self {
            sender: Arc::new(sender),
        }
    }

    fn send(&self, samples: Vec<f32>, sample_rate: u32) {
        (self.sender)(samples, sample_rate);
    }
}

pub struct AudioRecorder {
    stream: Stream,
    buffer: Arc<Mutex<Vec<f32>>>,
    level_bits: Arc<AtomicU32>,
    sample_rate: u32,
    stopped: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InputDeviceInfo {
    pub id: usize,
    pub name: String,
    pub is_default: bool,
}

impl AudioRecorder {
    pub fn start(config: &AppConfig) -> Result<Self> {
        Self::start_with_sink(config, None)
    }

    pub fn start_with_sink(config: &AppConfig, sink: Option<AudioChunkSink>) -> Result<Self> {
        let host = cpal::default_host();
        let device = pick_input_device(&host, config)?;
        let device_name = device
            .description()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|_| "Unknown input".to_string());
        let default_config = device
            .default_input_config()
            .context("failed to query default input config")?;
        let sample_format = default_config.sample_format();
        let stream_config: StreamConfig = default_config.config();

        let buffer = Arc::new(Mutex::new(Vec::new()));
        let level_bits = Arc::new(AtomicU32::new(0.0f32.to_bits()));
        let stopped = Arc::new(AtomicBool::new(false));

        let err_fn = |err| eprintln!("[audio] input stream error: {err}");
        let channels = stream_config.channels as usize;

        let stream = match sample_format {
            SampleFormat::F32 => build_stream_f32(
                &device,
                &stream_config,
                Arc::clone(&buffer),
                Arc::clone(&level_bits),
                channels,
                stream_config.sample_rate,
                sink.clone(),
                err_fn,
            )?,
            SampleFormat::I16 => build_stream_i16(
                &device,
                &stream_config,
                Arc::clone(&buffer),
                Arc::clone(&level_bits),
                channels,
                stream_config.sample_rate,
                sink.clone(),
                err_fn,
            )?,
            SampleFormat::U16 => build_stream_u16(
                &device,
                &stream_config,
                Arc::clone(&buffer),
                Arc::clone(&level_bits),
                channels,
                stream_config.sample_rate,
                sink,
                err_fn,
            )?,
            other => bail!("unsupported sample format: {other:?}"),
        };

        stream
            .play()
            .with_context(|| format!("failed to start capture from {device_name}"))?;

        Ok(Self {
            stream,
            buffer,
            level_bits,
            sample_rate: stream_config.sample_rate,
            stopped,
        })
    }

    pub fn stop(self) -> Result<Vec<f32>> {
        self.stopped.store(true, Ordering::Relaxed);
        drop(self.stream);
        let data = self
            .buffer
            .lock()
            .map_err(|_| anyhow!("audio buffer lock poisoned"))?
            .clone();
        if data.is_empty() {
            return Ok(Vec::new());
        }
        Ok(resample_linear(&data, self.sample_rate, 16_000))
    }

    pub fn current_level(&self) -> f32 {
        f32::from_bits(self.level_bits.load(Ordering::Relaxed))
    }
}

pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host.default_input_device().and_then(|device| {
        device
            .description()
            .map(|desc| desc.name().to_string())
            .ok()
    });

    let mut devices = Vec::new();
    for (index, device) in host.input_devices()?.enumerate() {
        let name = device
            .description()
            .map(|desc| desc.name().to_string())
            .unwrap_or_else(|_| format!("Input device {index}"));
        let is_default = default_name
            .as_ref()
            .is_some_and(|default| default == &name);
        devices.push(InputDeviceInfo {
            id: index,
            name,
            is_default,
        });
    }

    Ok(devices)
}

fn pick_input_device(host: &cpal::Host, config: &AppConfig) -> Result<cpal::Device> {
    if let Some(index) = config.audio_device_id {
        if let Some(device) = host.input_devices()?.nth(index) {
            return Ok(device);
        }
    }

    if let Some(name) = config
        .audio_device_name
        .as_ref()
        .or(config.selected_device_name.as_ref())
    {
        for device in host.input_devices()? {
            let device_name = device
                .description()
                .map(|desc| desc.name().to_string())
                .unwrap_or_default();
            if device_name.contains(name) {
                return Ok(device);
            }
        }
    }

    host.default_input_device()
        .context("no input audio device is available")
}

fn build_stream_f32(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    level_bits: Arc<AtomicU32>,
    channels: usize,
    sample_rate: u32,
    sink: Option<AudioChunkSink>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    device
        .build_input_stream(
            config,
            move |data: &[f32], _| {
                push_audio_f32(
                    data,
                    channels,
                    sample_rate,
                    &buffer,
                    &level_bits,
                    sink.as_ref(),
                )
            },
            err_fn,
            None,
        )
        .context("failed to build f32 input stream")
}

fn build_stream_i16(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    level_bits: Arc<AtomicU32>,
    channels: usize,
    sample_rate: u32,
    sink: Option<AudioChunkSink>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    device
        .build_input_stream(
            config,
            move |data: &[i16], _| {
                let data: Vec<f32> = data
                    .iter()
                    .map(|sample| *sample as f32 / i16::MAX as f32)
                    .collect();
                push_audio_f32(
                    &data,
                    channels,
                    sample_rate,
                    &buffer,
                    &level_bits,
                    sink.as_ref(),
                );
            },
            err_fn,
            None,
        )
        .context("failed to build i16 input stream")
}

fn build_stream_u16(
    device: &cpal::Device,
    config: &StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    level_bits: Arc<AtomicU32>,
    channels: usize,
    sample_rate: u32,
    sink: Option<AudioChunkSink>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    device
        .build_input_stream(
            config,
            move |data: &[u16], _| {
                let data: Vec<f32> = data
                    .iter()
                    .map(|sample| (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0)
                    .collect();
                push_audio_f32(
                    &data,
                    channels,
                    sample_rate,
                    &buffer,
                    &level_bits,
                    sink.as_ref(),
                );
            },
            err_fn,
            None,
        )
        .context("failed to build u16 input stream")
}

fn push_audio_f32(
    data: &[f32],
    channels: usize,
    sample_rate: u32,
    buffer: &Arc<Mutex<Vec<f32>>>,
    level_bits: &Arc<AtomicU32>,
    sink: Option<&AudioChunkSink>,
) {
    let mono = downmix_to_mono(data, channels);
    let level = rms(&mono);
    level_bits.store(level.to_bits(), Ordering::Relaxed);

    if let Some(sink) = sink {
        sink.send(mono.clone(), sample_rate);
    }

    if let Ok(mut shared) = buffer.lock() {
        shared.extend_from_slice(&mono);
    }
}

fn downmix_to_mono(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }

    data.chunks(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect()
}

fn rms(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let sum = data.iter().map(|sample| sample * sample).sum::<f32>() / data.len() as f32;
    sum.sqrt()
}

pub fn resample_linear(samples: &[f32], input_rate: u32, output_rate: u32) -> Vec<f32> {
    if input_rate == output_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = output_rate as f64 / input_rate as f64;
    let output_len = ((samples.len() as f64) * ratio).round() as usize;
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let position = index as f64 / ratio;
        let left = position.floor() as usize;
        let right = (left + 1).min(samples.len() - 1);
        let fraction = (position - left as f64) as f32;
        let value = samples[left] * (1.0 - fraction) + samples[right] * fraction;
        output.push(value);
    }

    output
}
