use crate::config::AppConfig;
use crate::credentials;
use crate::paths::AppPaths;
use crate::realtime::RealtimeSession;
use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::{Client, multipart};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const FASTER_WHISPER_RUNNER: &str = include_str!("../support/faster_whisper_runner.py");

pub struct BackendManager {
    paths: AppPaths,
    local: Option<LocalBackend>,
}

enum LocalBackend {
    Whisper(LocalWhisperBackend),
    FasterWhisper(FasterWhisperBackend),
}

impl BackendManager {
    pub fn new(paths: AppPaths, config: &AppConfig) -> Result<Self> {
        let local = match normalized_backend(&config.transcription_backend) {
            "whisper-rs" => Some(LocalBackend::Whisper(LocalWhisperBackend::new(
                &paths, config,
            )?)),
            "faster-whisper" => Some(LocalBackend::FasterWhisper(FasterWhisperBackend::new(
                paths.clone(),
            )?)),
            _ => None,
        };

        Ok(Self { paths, local })
    }

    pub fn is_realtime_backend(&self, config: &AppConfig) -> bool {
        normalized_backend(&config.transcription_backend) == "realtime-ws"
    }

    pub fn start_realtime_session(
        &self,
        config: &AppConfig,
        language_override: Option<&str>,
    ) -> Result<Option<RealtimeSession>> {
        if !self.is_realtime_backend(config) {
            return Ok(None);
        }

        Ok(Some(RealtimeSession::connect(
            &self.paths,
            config,
            language_override,
        )?))
    }

    pub fn transcribe(
        &mut self,
        config: &AppConfig,
        audio: &[f32],
        language_override: Option<&str>,
    ) -> Result<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        match normalized_backend(&config.transcription_backend) {
            "whisper-rs" => match self.local.as_mut() {
                Some(LocalBackend::Whisper(backend)) => {
                    backend.transcribe(config, audio, language_override)
                }
                _ => bail!("local whisper backend not initialized"),
            },
            "faster-whisper" => match self.local.as_ref() {
                Some(LocalBackend::FasterWhisper(backend)) => {
                    backend.transcribe(config, audio, language_override)
                }
                _ => bail!("faster-whisper backend not initialized"),
            },
            "rest-api" => transcribe_rest(&self.paths, config, audio, language_override),
            "realtime-ws" => {
                bail!("realtime websocket backend is only available for live recording")
            }
            other => bail!("unsupported backend: {other}"),
        }
    }
}

fn normalized_backend(name: &str) -> &str {
    match name {
        "local" | "pywhispercpp" | "cpu" | "nvidia" | "vulkan" | "amd" => "whisper-rs",
        "remote" => "rest-api",
        "faster_whisper" | "fasterwhisper" => "faster-whisper",
        "realtime" | "realtime-websocket" | "openai-realtime" => "realtime-ws",
        other => other,
    }
}

struct LocalWhisperBackend {
    context: WhisperContext,
    model_path: String,
}

impl LocalWhisperBackend {
    fn new(paths: &AppPaths, config: &AppConfig) -> Result<Self> {
        let model_path = resolve_model_path(paths, config)?;
        let context =
            WhisperContext::new_with_params(&model_path, WhisperContextParameters::default())
                .with_context(|| format!("failed to load whisper model {}", model_path))?;
        Ok(Self {
            context,
            model_path,
        })
    }

    fn transcribe(
        &mut self,
        config: &AppConfig,
        audio: &[f32],
        language_override: Option<&str>,
    ) -> Result<String> {
        let mut state = self
            .context
            .create_state()
            .context("failed to create whisper state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 0 });
        params.set_n_threads(config.threads as i32);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let requested_language = language_override
            .or(config.language.as_deref())
            .or(Some("en"));
        match requested_language {
            Some(language) if !language.eq_ignore_ascii_case("auto") => {
                params.set_language(Some(language))
            }
            _ => {
                params.set_detect_language(true);
                params.set_language(None);
            }
        }

        if !config.whisper_prompt.trim().is_empty() {
            params.set_initial_prompt(&config.whisper_prompt);
        }

        state
            .full(params, audio)
            .with_context(|| format!("failed to transcribe via {}", self.model_path))?;

        let mut text = String::new();
        for segment in state.as_iter() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(segment.to_str_lossy()?.trim());
        }

        Ok(text.trim().to_string())
    }
}

struct FasterWhisperBackend {
    paths: AppPaths,
    runner_path: PathBuf,
}

impl FasterWhisperBackend {
    fn new(paths: AppPaths) -> Result<Self> {
        let runner_path = ensure_faster_whisper_runner(&paths)?;
        Ok(Self { paths, runner_path })
    }

    fn transcribe(
        &self,
        config: &AppConfig,
        audio: &[f32],
        language_override: Option<&str>,
    ) -> Result<String> {
        let wav = encode_wav(audio)?;
        let wav_path = write_temp_wav(&self.paths, &wav)?;

        let python = detect_python_binary();
        let language = language_override
            .or(config.language.as_deref())
            .filter(|language| !language.eq_ignore_ascii_case("auto"))
            .unwrap_or("en");

        let mut command = Command::new(&python);
        command
            .arg(&self.runner_path)
            .arg("--file")
            .arg(&wav_path)
            .arg("--model")
            .arg(&config.faster_whisper_model)
            .arg("--device")
            .arg(&config.faster_whisper_device)
            .arg("--compute-type")
            .arg(&config.faster_whisper_compute_type)
            .arg("--language")
            .arg(language);

        if config.faster_whisper_vad_filter {
            command.arg("--vad-filter");
        }
        if !config.whisper_prompt.trim().is_empty() {
            command
                .arg("--initial-prompt")
                .arg(config.whisper_prompt.trim());
        }

        let output = command.output().with_context(|| {
            format!(
                "failed to launch faster-whisper runner with {}",
                python.display()
            )
        })?;
        let _ = fs::remove_file(&wav_path);

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let body = if stdout.trim().is_empty() {
            stderr.trim()
        } else {
            stdout.trim()
        };

        if !output.status.success() {
            if let Ok(parsed) = serde_json::from_str::<Value>(body) {
                if let Some(error) = parsed.get("error").and_then(Value::as_str) {
                    bail!("faster-whisper failed: {error}");
                }
            }
            bail!("faster-whisper failed: {body}");
        }

        extract_transcript(body)
            .ok_or_else(|| anyhow!("could not extract transcript from faster-whisper output"))
    }
}

fn resolve_model_path(paths: &AppPaths, config: &AppConfig) -> Result<String> {
    if let Some(path) = config.model_path.as_ref() {
        return Ok(path.clone());
    }

    for candidate in paths.model_search_paths(&config.model) {
        if candidate.exists() {
            return Ok(candidate.display().to_string());
        }
    }

    bail!(
        "no whisper model found for '{}'; set config.model_path or place ggml-{}.bin under {}",
        config.model,
        config.model,
        paths.data_dir.join("models").display()
    )
}

fn transcribe_rest(
    paths: &AppPaths,
    config: &AppConfig,
    audio: &[f32],
    language_override: Option<&str>,
) -> Result<String> {
    let provider = config.rest_api_provider.as_deref().unwrap_or("custom");
    let endpoint = config
        .rest_endpoint_url
        .as_deref()
        .or_else(|| default_endpoint(provider))
        .context("REST backend requires rest_endpoint_url or rest_api_provider")?;
    let api_key = resolve_api_key(paths, config)?;

    let mut headers = config.rest_headers.clone();
    if let Some(key) = api_key.as_deref() {
        match provider {
            "elevenlabs" => {
                headers.insert("xi-api-key".to_string(), key.to_string());
            }
            _ => {
                headers.insert("Authorization".to_string(), format!("Bearer {key}"));
            }
        }
    }

    let wav = encode_wav(audio)?;
    let file_part = multipart::Part::bytes(wav)
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .context("failed to build multipart audio body")?;

    let mut form_fields: BTreeMap<String, String> = default_rest_body(provider)
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    for (key, value) in &config.rest_body {
        if !value.is_null() {
            form_fields.insert(key.clone(), json_value_to_field(value));
        }
    }

    let mut form = multipart::Form::new().part("file", file_part);
    for (key, value) in form_fields {
        form = form.text(key, value);
    }
    if let Some(language) = language_override
        .or(config.language.as_deref())
        .or(Some("en"))
    {
        if !language.eq_ignore_ascii_case("auto") {
            form = form.text("language".to_string(), language.to_string());
        }
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(config.rest_timeout))
        .build()
        .context("failed to create HTTP client")?;
    let mut request = client.post(endpoint).multipart(form);
    for (key, value) in headers {
        request = request.header(&key, value);
    }

    let response = request
        .send()
        .with_context(|| format!("failed to call {endpoint}"))?;
    let status = response.status();
    let body = response
        .text()
        .context("failed to read REST response body")?;
    if !status.is_success() {
        bail!("REST backend returned {status}: {body}");
    }

    extract_transcript(&body)
        .ok_or_else(|| anyhow!("could not extract transcript from REST response"))
}

fn resolve_api_key(paths: &AppPaths, config: &AppConfig) -> Result<Option<String>> {
    if let Some(key) = config.rest_api_key.as_ref() {
        return Ok(Some(key.clone()));
    }

    if let Some(provider) = config.rest_api_provider.as_deref() {
        return credentials::get_credential(paths, provider);
    }

    Ok(None)
}

fn default_endpoint(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("https://api.openai.com/v1/audio/transcriptions"),
        "groq" => Some("https://api.groq.com/openai/v1/audio/transcriptions"),
        "elevenlabs" => Some("https://api.elevenlabs.io/v1/speech-to-text"),
        _ => None,
    }
}

fn default_rest_body(provider: &str) -> BTreeMap<&'static str, &'static str> {
    match provider {
        "openai" => BTreeMap::from([("model", "gpt-4o-mini-transcribe")]),
        "groq" => BTreeMap::from([("model", "whisper-large-v3-turbo")]),
        "elevenlabs" => BTreeMap::from([("model_id", "scribe_v2")]),
        _ => BTreeMap::new(),
    }
}

fn json_value_to_field(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn extract_transcript(body: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(body).ok()?;
    if let Some(text) = parsed.get("text").and_then(Value::as_str) {
        return Some(text.trim().to_string());
    }
    if let Some(text) = parsed.get("transcript").and_then(Value::as_str) {
        return Some(text.trim().to_string());
    }
    parsed
        .pointer("/results/channels/0/alternatives/0/transcript")
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
}

fn encode_wav(audio: &[f32]) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    let mut writer =
        hound::WavWriter::new(&mut cursor, spec).context("failed to create WAV writer")?;
    for sample in audio {
        let sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(sample)
            .context("failed to write sample to WAV buffer")?;
    }
    writer.finalize().context("failed to finalize WAV buffer")?;
    Ok(cursor.into_inner())
}

fn ensure_faster_whisper_runner(paths: &AppPaths) -> Result<PathBuf> {
    let support_dir = paths.data_dir.join("support");
    fs::create_dir_all(&support_dir)
        .with_context(|| format!("failed to create {}", support_dir.display()))?;
    let runner_path = support_dir.join("faster_whisper_runner.py");
    let needs_write = fs::read_to_string(&runner_path)
        .map(|existing| existing != FASTER_WHISPER_RUNNER)
        .unwrap_or(true);
    if needs_write {
        fs::write(&runner_path, FASTER_WHISPER_RUNNER)
            .with_context(|| format!("failed to write {}", runner_path.display()))?;
    }
    Ok(runner_path)
}

fn write_temp_wav(paths: &AppPaths, wav: &[u8]) -> Result<PathBuf> {
    let temp_dir = paths.data_dir.join("tmp");
    fs::create_dir_all(&temp_dir)
        .with_context(|| format!("failed to create {}", temp_dir.display()))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let wav_path = temp_dir.join(format!("faster-whisper-{now}-{}.wav", std::process::id()));
    fs::write(&wav_path, wav).with_context(|| format!("failed to write {}", wav_path.display()))?;
    Ok(wav_path)
}

fn detect_python_binary() -> PathBuf {
    if let Ok(python) = env::var("HYPERWHISPER_PYTHON") {
        return PathBuf::from(python);
    }
    if let Ok(python) = env::var("PYTHON") {
        return PathBuf::from(python);
    }
    PathBuf::from("python3")
}
