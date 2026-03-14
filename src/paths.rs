use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub config_file: PathBuf,
    pub control_file: PathBuf,
    pub daemon_heartbeat_file: PathBuf,
    pub recording_status_file: PathBuf,
    pub audio_level_file: PathBuf,
    pub zero_volume_file: PathBuf,
    pub long_form_segments_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "hermes")
            .context("failed to resolve platform-specific app directories")?;
        let config_dir = project_dirs.config_dir().to_path_buf();
        let data_dir = project_dirs.data_dir().to_path_buf();
        Ok(Self {
            config_file: config_dir.join("config.json"),
            control_file: config_dir.join("hermes_control"),
            daemon_heartbeat_file: config_dir.join("hermes_heartbeat"),
            recording_status_file: config_dir.join("hermes_recording_status"),
            audio_level_file: config_dir.join("hermes_audio_level"),
            zero_volume_file: config_dir.join(".hermes_mic_zero_volume"),
            long_form_segments_dir: data_dir.join("long-form-segments"),
            config_dir,
            data_dir,
        })
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config_dir)
            .with_context(|| format!("failed to create {}", self.config_dir.display()))?;
        std::fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("failed to create {}", self.data_dir.display()))?;
        Ok(())
    }

    pub fn model_search_paths(&self, model: &str) -> Vec<PathBuf> {
        let home = directories::BaseDirs::new().map(|base| base.home_dir().to_path_buf());
        let mut paths = Vec::new();

        if let Some(home) = home {
            let pywhisper_dir = home.join(".local/share/pywhispercpp/models");
            paths.push(pywhisper_dir.join(format!("ggml-{model}.bin")));
            paths.push(pywhisper_dir.join(format!("ggml-{model}.en.bin")));
        }

        paths.push(
            self.data_dir
                .join("models")
                .join(format!("ggml-{model}.bin")),
        );
        paths.push(
            self.data_dir
                .join("models")
                .join(format!("ggml-{model}.en.bin")),
        );
        paths
    }

    pub fn ensure_parent(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        Ok(())
    }
}
