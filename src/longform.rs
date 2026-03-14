use crate::paths::AppPaths;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct SegmentStore {
    base_dir: PathBuf,
    session_id: String,
    max_size_bytes: u64,
    next_index: usize,
    segments: Vec<PathBuf>,
}

impl SegmentStore {
    pub fn new(paths: &AppPaths, max_size_mb: u64) -> Result<Self> {
        fs::create_dir_all(&paths.long_form_segments_dir).with_context(|| {
            format!(
                "failed to create {}",
                paths.long_form_segments_dir.display()
            )
        })?;

        let max_size_bytes = max_size_mb.saturating_mul(1024 * 1024);
        cleanup_oldest_segments(&paths.long_form_segments_dir, max_size_bytes, None)?;

        Ok(Self {
            base_dir: paths.long_form_segments_dir.clone(),
            session_id: new_session_id(),
            max_size_bytes,
            next_index: 0,
            segments: Vec::new(),
        })
    }

    pub fn save_segment(&mut self, audio: &[f32]) -> Result<Option<PathBuf>> {
        if audio.is_empty() {
            return Ok(None);
        }

        let filename = format!(
            "{}_{:03}_{}.wav",
            self.session_id,
            self.next_index,
            timestamp_ms()
        );
        self.next_index += 1;

        let path = self.base_dir.join(filename);
        write_wav(&path, audio)?;
        self.segments.push(path.clone());
        cleanup_oldest_segments(&self.base_dir, self.max_size_bytes, Some(&self.session_id))?;
        Ok(Some(path))
    }

    pub fn concatenate(&self) -> Result<Vec<f32>> {
        let mut output = Vec::new();
        for segment in &self.segments {
            let samples = read_wav(segment)?;
            output.extend(samples);
        }
        Ok(output)
    }

    pub fn has_segments(&self) -> bool {
        !self.segments.is_empty()
    }

    pub fn clear_session(&mut self) {
        for segment in self.segments.drain(..) {
            let _ = fs::remove_file(segment);
        }
    }
}

impl Drop for SegmentStore {
    fn drop(&mut self) {
        self.clear_session();
    }
}

fn new_session_id() -> String {
    format!("{:x}", timestamp_ms())
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn write_wav(path: &PathBuf, audio: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("failed to create {}", path.display()))?;
    for sample in audio {
        let sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(sample)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }
    writer
        .finalize()
        .with_context(|| format!("failed to finalize {}", path.display()))?;
    Ok(())
}

fn read_wav(path: &PathBuf) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let samples = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(samples
        .into_iter()
        .map(|sample| sample as f32 / i16::MAX as f32)
        .collect())
}

fn cleanup_oldest_segments(
    dir: &PathBuf,
    max_size_bytes: u64,
    protected_session: Option<&str>,
) -> Result<()> {
    if max_size_bytes == 0 {
        return Ok(());
    }

    let mut files = fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("wav"))
        .filter_map(|path| {
            let metadata = fs::metadata(&path).ok()?;
            let modified = metadata.modified().ok()?;
            Some((path, metadata.len(), modified))
        })
        .collect::<Vec<_>>();

    let mut total_size = files.iter().map(|(_, size, _)| *size).sum::<u64>();
    if total_size <= max_size_bytes {
        return Ok(());
    }

    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, size, _) in files {
        if total_size <= max_size_bytes {
            break;
        }
        if let Some(session_id) = protected_session {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(session_id))
            {
                continue;
            }
        }
        if fs::remove_file(&path).is_ok() {
            total_size = total_size.saturating_sub(size);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_paths() -> AppPaths {
        let root = std::env::temp_dir().join(format!(
            "hermes-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        AppPaths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            config_file: root.join("config/config.json"),
            control_file: root.join("config/control"),
            daemon_heartbeat_file: root.join("config/heartbeat"),
            recording_status_file: root.join("config/recording_status"),
            audio_level_file: root.join("config/audio_level"),
            zero_volume_file: root.join("config/zero_volume"),
            long_form_segments_dir: root.join("data/segments"),
        }
    }

    #[test]
    fn segment_store_round_trips_audio() -> Result<()> {
        let paths = temp_paths();
        let mut store = SegmentStore::new(&paths, 10)?;
        store.save_segment(&[0.1, -0.2, 0.3, -0.4])?;
        store.save_segment(&[0.5, -0.6])?;

        let combined = store.concatenate()?;
        assert_eq!(combined.len(), 6);
        assert!(store.has_segments());

        store.clear_session();
        let _ = fs::remove_dir_all(paths.config_dir.parent().unwrap_or(&paths.config_dir));
        Ok(())
    }
}
