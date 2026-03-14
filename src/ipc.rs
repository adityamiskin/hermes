use crate::paths::AppPaths;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn send_control(paths: &AppPaths, command: &str) -> Result<()> {
    AppPaths::ensure_parent(&paths.control_file)?;
    let temp = paths.control_file.with_extension("tmp");
    fs::write(&temp, format!("{command}\n"))
        .with_context(|| format!("failed to write {}", temp.display()))?;
    fs::rename(&temp, &paths.control_file).with_context(|| {
        format!(
            "failed to move command into {}",
            paths.control_file.display()
        )
    })?;
    Ok(())
}

pub fn take_control(paths: &AppPaths) -> Result<Option<String>> {
    take_last_line(&paths.control_file)
}

pub fn set_recording_status(paths: &AppPaths, recording: bool) -> Result<()> {
    if recording {
        write_atomic_text(&paths.recording_status_file, "true\n")?;
    } else if paths.recording_status_file.exists() {
        let _ = fs::remove_file(&paths.recording_status_file);
    }
    Ok(())
}

pub fn is_recording(paths: &AppPaths) -> bool {
    fs::read_to_string(&paths.recording_status_file)
        .map(|content| content.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

pub fn write_daemon_heartbeat(paths: &AppPaths) -> Result<()> {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    write_atomic_text(&paths.daemon_heartbeat_file, &format!("{timestamp_ms}\n"))
}

pub fn heartbeat_is_fresh(paths: &AppPaths, max_age: Duration) -> bool {
    let Ok(raw) = fs::read_to_string(&paths.daemon_heartbeat_file) else {
        return false;
    };
    let Ok(timestamp_ms) = raw.trim().parse::<u128>() else {
        return false;
    };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    now_ms.saturating_sub(timestamp_ms) <= max_age.as_millis()
}

pub fn write_audio_level(paths: &AppPaths, level: f32) -> Result<()> {
    write_atomic_text(&paths.audio_level_file, &format!("{level:.3}\n"))
}

pub fn read_audio_level(paths: &AppPaths) -> f32 {
    fs::read_to_string(&paths.audio_level_file)
        .ok()
        .and_then(|content| content.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
}

pub fn clear_audio_level(paths: &AppPaths) {
    let _ = fs::remove_file(&paths.audio_level_file);
}

pub fn mark_zero_volume(paths: &AppPaths, message: &str) -> Result<()> {
    write_atomic_text(&paths.zero_volume_file, &format!("{message}\n"))
}

pub fn clear_zero_volume(paths: &AppPaths) {
    let _ = fs::remove_file(&paths.zero_volume_file);
}

pub fn reset_runtime_state(paths: &AppPaths) {
    let _ = fs::remove_file(&paths.control_file);
    let _ = fs::remove_file(&paths.daemon_heartbeat_file);
    let _ = fs::remove_file(&paths.recording_status_file);
    let _ = fs::remove_file(&paths.audio_level_file);
    let _ = fs::remove_file(&paths.zero_volume_file);
}

fn take_last_line(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let _ = fs::remove_file(path);
    let line = raw
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string);
    Ok(line)
}

fn write_atomic_text(path: &Path, contents: &str) -> Result<()> {
    AppPaths::ensure_parent(path)?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, contents).with_context(|| format!("failed to write {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("failed to move data into {}", path.display()))
}
