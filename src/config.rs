use crate::paths::AppPaths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

const CONFIG_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/goodroot/hyprwhspr/main/share/config.schema.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppConfig {
    pub primary_shortcut: String,
    pub secondary_shortcut: Option<String>,
    pub secondary_language: Option<String>,
    pub cancel_shortcut: Option<String>,
    pub long_form_submit_shortcut: Option<String>,
    pub recording_mode: String,
    pub use_hypr_bindings: bool,
    pub selected_device_name: Option<String>,
    pub audio_device_id: Option<usize>,
    pub audio_device_name: Option<String>,
    pub model: String,
    pub model_path: Option<String>,
    pub threads: usize,
    pub language: Option<String>,
    pub word_overrides: BTreeMap<String, String>,
    pub filter_filler_words: bool,
    pub filler_words: Vec<String>,
    pub symbol_replacements: bool,
    pub whisper_prompt: String,
    pub clipboard_behavior: bool,
    pub clipboard_clear_delay: f32,
    pub paste_mode: String,
    pub transcription_backend: String,
    pub rest_endpoint_url: Option<String>,
    pub rest_api_provider: Option<String>,
    pub rest_api_key: Option<String>,
    pub rest_headers: BTreeMap<String, String>,
    pub rest_body: BTreeMap<String, Value>,
    pub rest_timeout: u64,
    pub websocket_provider: Option<String>,
    pub websocket_model: Option<String>,
    pub websocket_url: Option<String>,
    pub realtime_timeout: u64,
    pub realtime_buffer_max_seconds: u64,
    pub realtime_mode: String,
    pub onnx_asr_model: String,
    pub onnx_asr_quantization: Option<String>,
    pub onnx_asr_use_vad: bool,
    pub faster_whisper_model: String,
    pub faster_whisper_device: String,
    pub faster_whisper_compute_type: String,
    pub faster_whisper_vad_filter: bool,
    pub long_form_temp_limit_mb: u64,
    pub long_form_auto_save_interval: u64,
    pub auto_submit: bool,
    pub show_overlay: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            primary_shortcut: "SUPER+ALT+D".to_string(),
            secondary_shortcut: None,
            secondary_language: None,
            cancel_shortcut: None,
            long_form_submit_shortcut: None,
            recording_mode: "toggle".to_string(),
            use_hypr_bindings: false,
            selected_device_name: None,
            audio_device_id: None,
            audio_device_name: None,
            model: "base".to_string(),
            model_path: None,
            threads: 4,
            language: Some("en".to_string()),
            word_overrides: BTreeMap::new(),
            filter_filler_words: false,
            filler_words: vec![
                "uh".into(),
                "um".into(),
                "er".into(),
                "ah".into(),
                "eh".into(),
                "hmm".into(),
                "hm".into(),
            ],
            symbol_replacements: true,
            whisper_prompt: "Transcribe with proper capitalization and punctuation.".to_string(),
            clipboard_behavior: false,
            clipboard_clear_delay: 5.0,
            paste_mode: "ctrl_shift".to_string(),
            transcription_backend: "whisper-rs".to_string(),
            rest_endpoint_url: None,
            rest_api_provider: None,
            rest_api_key: None,
            rest_headers: BTreeMap::new(),
            rest_body: BTreeMap::new(),
            rest_timeout: 30,
            websocket_provider: None,
            websocket_model: None,
            websocket_url: None,
            realtime_timeout: 30,
            realtime_buffer_max_seconds: 5,
            realtime_mode: "transcribe".to_string(),
            onnx_asr_model: "nemo-parakeet-tdt-0.6b-v3".to_string(),
            onnx_asr_quantization: Some("int8".to_string()),
            onnx_asr_use_vad: true,
            faster_whisper_model: "base".to_string(),
            faster_whisper_device: "auto".to_string(),
            faster_whisper_compute_type: "auto".to_string(),
            faster_whisper_vad_filter: true,
            long_form_temp_limit_mb: 500,
            long_form_auto_save_interval: 300,
            auto_submit: false,
            show_overlay: true,
        }
    }
}

impl AppConfig {
    pub fn load(paths: &AppPaths) -> Result<Self> {
        paths.ensure()?;
        if !paths.config_file.exists() {
            let config = Self::default();
            config.save(paths)?;
            return Ok(config);
        }

        let raw = fs::read_to_string(&paths.config_file)
            .with_context(|| format!("failed to read {}", paths.config_file.display()))?;
        let mut value: Value = serde_json::from_str(&raw).context("failed to parse config JSON")?;

        if let Some(object) = value.as_object_mut() {
            migrate_legacy_keys(object);
        }

        let mut config: AppConfig =
            serde_json::from_value(value).context("failed to deserialize config")?;
        normalize_backend_name(&mut config.transcription_backend);
        Ok(config)
    }

    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        paths.ensure()?;
        let value = serde_json::to_value(self).context("failed to serialize config")?;
        let mut sparse = Map::new();
        sparse.insert(
            "$schema".to_string(),
            Value::String(CONFIG_SCHEMA_URL.to_string()),
        );
        let defaults =
            serde_json::to_value(Self::default()).context("failed to serialize defaults")?;

        if let (Some(current), Some(defaults)) = (value.as_object(), defaults.as_object()) {
            for (key, current_value) in current {
                if defaults.get(key) != Some(current_value) {
                    sparse.insert(key.clone(), current_value.clone());
                }
            }
        }

        let json = serde_json::to_string_pretty(&Value::Object(sparse))
            .context("failed to format config")?;
        fs::write(&paths.config_file, format!("{json}\n"))
            .with_context(|| format!("failed to write {}", paths.config_file.display()))?;
        Ok(())
    }

    pub fn edit(&self, paths: &AppPaths) -> Result<()> {
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| default_editor().to_string());
        Command::new(editor)
            .arg(&paths.config_file)
            .status()
            .context("failed to launch editor")?;
        Ok(())
    }
}

fn migrate_legacy_keys(object: &mut Map<String, Value>) {
    object.remove("$schema");

    if let Some(value) = object.remove("push_to_talk") {
        let recording_mode = match value {
            Value::Bool(true) => "push_to_talk",
            _ => "toggle",
        };
        object.insert(
            "recording_mode".to_string(),
            Value::String(recording_mode.to_string()),
        );
    }

    if let Some(value) = object.remove("audio_device") {
        object.insert("audio_device_id".to_string(), value);
    }
}

fn normalize_backend_name(name: &mut String) {
    let normalized = match name.as_str() {
        "local" | "pywhispercpp" | "cpu" | "nvidia" | "vulkan" | "amd" => "whisper-rs",
        "remote" => "rest-api",
        "faster_whisper" | "fasterwhisper" => "faster-whisper",
        "realtime" | "realtime-websocket" | "openai-realtime" => "realtime-ws",
        other => other,
    };
    *name = normalized.to_string();
}

fn default_editor() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "notepad"
    }
    #[cfg(target_os = "macos")]
    {
        "open"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "nano"
    }
}
