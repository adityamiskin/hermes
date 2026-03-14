use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use std::f32::consts::{E, TAU};
use std::thread;
use std::time::Duration;

const COMPLETION_FREQUENCY_HZ: f32 = 210.0;
const COMPLETION_OVERTONE_HZ: f32 = 470.0;
const COMPLETION_VOLUME: f32 = 0.19;
const COMPLETION_DURATION_MS: u64 = 125;
const ATTACK_DURATION_MS: u64 = 6;
const RELEASE_DURATION_MS: u64 = 72;

pub fn play_completion_tone() {
    thread::spawn(|| {
        if let Err(error) = play_completion_tone_blocking() {
            eprintln!("[feedback] completion tone failed: {error}");
        }
    });
}

fn play_completion_tone_blocking() -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default output device is available")?;
    let default_config = device
        .default_output_config()
        .context("failed to query default output config")?;
    let sample_format = default_config.sample_format();
    let config: StreamConfig = default_config.config();
    let sample_rate = config.sample_rate;
    let channels = config.channels as usize;
    let total_frames = duration_frames(sample_rate, COMPLETION_DURATION_MS);

    let err_fn = |err| eprintln!("[feedback] output stream error: {err}");
    let stream = match sample_format {
        SampleFormat::F32 => build_output_stream_f32(
            &device,
            &config,
            sample_rate,
            channels,
            total_frames,
            err_fn,
        )?,
        SampleFormat::I16 => build_output_stream_i16(
            &device,
            &config,
            sample_rate,
            channels,
            total_frames,
            err_fn,
        )?,
        SampleFormat::U16 => build_output_stream_u16(
            &device,
            &config,
            sample_rate,
            channels,
            total_frames,
            err_fn,
        )?,
        other => anyhow::bail!("unsupported output sample format: {other:?}"),
    };

    stream.play().context("failed to start completion tone")?;
    thread::sleep(Duration::from_millis(
        COMPLETION_DURATION_MS + RELEASE_DURATION_MS + 24,
    ));
    drop(stream);
    Ok(())
}

fn duration_frames(sample_rate: u32, duration_ms: u64) -> usize {
    ((sample_rate as u64 * duration_ms) / 1_000).max(1) as usize
}

fn build_output_stream_f32(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_rate: u32,
    channels: usize,
    total_frames: usize,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let mut frame_index = 0usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [f32], _| {
                write_tone_frames_f32(data, channels, &mut frame_index, sample_rate, total_frames)
            },
            err_fn,
            None,
        )
        .context("failed to build f32 output stream")
}

fn build_output_stream_i16(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_rate: u32,
    channels: usize,
    total_frames: usize,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let mut frame_index = 0usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [i16], _| {
                write_tone_frames_i16(data, channels, &mut frame_index, sample_rate, total_frames)
            },
            err_fn,
            None,
        )
        .context("failed to build i16 output stream")
}

fn build_output_stream_u16(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_rate: u32,
    channels: usize,
    total_frames: usize,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<Stream> {
    let mut frame_index = 0usize;
    device
        .build_output_stream(
            config,
            move |data: &mut [u16], _| {
                write_tone_frames_u16(data, channels, &mut frame_index, sample_rate, total_frames)
            },
            err_fn,
            None,
        )
        .context("failed to build u16 output stream")
}

fn write_tone_frames_f32(
    data: &mut [f32],
    channels: usize,
    frame_index: &mut usize,
    sample_rate: u32,
    total_frames: usize,
) {
    for frame in data.chunks_mut(channels) {
        let sample = tone_sample(*frame_index, sample_rate, total_frames);
        for channel in frame {
            *channel = sample;
        }
        *frame_index += 1;
    }
}

fn write_tone_frames_i16(
    data: &mut [i16],
    channels: usize,
    frame_index: &mut usize,
    sample_rate: u32,
    total_frames: usize,
) {
    for frame in data.chunks_mut(channels) {
        let sample = (tone_sample(*frame_index, sample_rate, total_frames) * i16::MAX as f32)
            .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        for channel in frame {
            *channel = sample;
        }
        *frame_index += 1;
    }
}

fn write_tone_frames_u16(
    data: &mut [u16],
    channels: usize,
    frame_index: &mut usize,
    sample_rate: u32,
    total_frames: usize,
) {
    for frame in data.chunks_mut(channels) {
        let normalized = tone_sample(*frame_index, sample_rate, total_frames).clamp(-1.0, 1.0);
        let sample =
            (((normalized + 1.0) * 0.5) * u16::MAX as f32).clamp(0.0, u16::MAX as f32) as u16;
        for channel in frame {
            *channel = sample;
        }
        *frame_index += 1;
    }
}

fn tone_sample(frame_index: usize, sample_rate: u32, total_frames: usize) -> f32 {
    if frame_index >= total_frames {
        return 0.0;
    }

    let time = frame_index as f32 / sample_rate as f32;
    let envelope = tone_envelope(frame_index, sample_rate, total_frames);
    let body = (TAU * COMPLETION_FREQUENCY_HZ * time).sin();
    let overtone = (TAU * COMPLETION_OVERTONE_HZ * time).sin() * 0.30;
    let knock = (TAU * 96.0 * time).sin() * 0.18;
    (body + overtone + knock) * COMPLETION_VOLUME * envelope
}

fn tone_envelope(frame_index: usize, sample_rate: u32, total_frames: usize) -> f32 {
    if total_frames <= 1 {
        return 1.0;
    }

    let attack_frames = duration_frames(sample_rate, ATTACK_DURATION_MS).max(1) as f32;
    let release_frames = duration_frames(sample_rate, RELEASE_DURATION_MS).max(1) as f32;
    let frame = frame_index as f32;
    let attack = (frame / attack_frames).clamp(0.0, 1.0);
    let decay = E.powf(-(frame / release_frames) * 4.2);
    let tail =
        ((total_frames.saturating_sub(frame_index + 1)) as f32 / release_frames).clamp(0.0, 1.0);
    attack * decay * tail
}

#[cfg(test)]
mod tests {
    use super::{tone_envelope, tone_sample};

    #[test]
    fn tone_envelope_fades_in_and_out() {
        let sample_rate = 48_000;
        let total_frames = 6_000;

        assert_eq!(tone_envelope(0, sample_rate, total_frames), 0.0);
        assert!(tone_envelope(100, sample_rate, total_frames) > 0.0);
        assert!(
            tone_envelope(400, sample_rate, total_frames)
                > tone_envelope(2_000, sample_rate, total_frames)
        );
        assert_eq!(
            tone_envelope(total_frames - 1, sample_rate, total_frames),
            0.0
        );
    }

    #[test]
    fn tone_sample_returns_silence_after_duration() {
        assert_eq!(tone_sample(10_000, 48_000, 100), 0.0);
    }
}
