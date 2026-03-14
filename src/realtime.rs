use crate::audio::{AudioChunkSink, resample_linear};
use crate::config::AppConfig;
use crate::credentials;
use crate::paths::AppPaths;
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tokio::runtime::Builder;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderValue, header};
use tokio_tungstenite::tungstenite::protocol::Message;

const OPENAI_REALTIME_URL: &str = "wss://api.openai.com/v1/realtime?intent=transcription";
const TARGET_SAMPLE_RATE: u32 = 24_000;

pub struct RealtimeSession {
    sender: mpsc::Sender<WorkerCommand>,
    handle: Option<JoinHandle<Result<String>>>,
}

enum WorkerCommand {
    Audio { samples: Vec<f32>, sample_rate: u32 },
    Finish,
    Abort,
}

#[derive(Default)]
struct TranscriptState {
    committed_segments: Vec<String>,
    partial_segment: String,
    last_error: Option<String>,
}

struct FinishState {
    deadline: Instant,
    quiet_until: Instant,
}

impl RealtimeSession {
    pub fn connect(
        paths: &AppPaths,
        config: &AppConfig,
        language_override: Option<&str>,
    ) -> Result<Self> {
        let provider = config.websocket_provider.as_deref().unwrap_or("openai");
        if !config.realtime_mode.eq_ignore_ascii_case("transcribe") {
            bail!("only realtime transcription mode is implemented");
        }
        if provider != "openai" && provider != "custom" {
            bail!("unsupported realtime websocket provider: {provider}");
        }

        let api_key = credentials::get_credential(paths, provider)?
            .context("realtime websocket provider requires an API key in the OS keychain")?;
        let model = config
            .websocket_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini-transcribe".to_string());
        let url = resolve_websocket_url(config, provider)?;
        let language = language_override
            .or(config.language.as_deref())
            .filter(|language| !language.eq_ignore_ascii_case("auto"))
            .map(ToOwned::to_owned);
        let timeout = Duration::from_secs(config.realtime_timeout.max(3));

        let (sender, receiver) = mpsc::channel(64);
        let handle = thread::spawn(move || {
            let runtime = Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to create realtime runtime")?;
            runtime.block_on(run_worker(url, api_key, model, language, timeout, receiver))
        });

        Ok(Self {
            sender,
            handle: Some(handle),
        })
    }

    pub fn audio_sink(&self) -> AudioChunkSink {
        let sender = self.sender.clone();
        AudioChunkSink::new(move |samples, sample_rate| {
            let _ = sender.try_send(WorkerCommand::Audio {
                samples,
                sample_rate,
            });
        })
    }

    pub fn finish(mut self) -> Result<String> {
        let _ = self.sender.blocking_send(WorkerCommand::Finish);
        self.join()
    }

    pub fn abort(mut self) -> Result<()> {
        let _ = self.sender.blocking_send(WorkerCommand::Abort);
        let _ = self.join()?;
        Ok(())
    }

    fn join(&mut self) -> Result<String> {
        let handle = self
            .handle
            .take()
            .context("realtime session worker already joined")?;
        handle
            .join()
            .map_err(|_| anyhow!("realtime session thread panicked"))?
    }
}

async fn run_worker(
    url: String,
    api_key: String,
    model: String,
    language: Option<String>,
    timeout: Duration,
    mut receiver: mpsc::Receiver<WorkerCommand>,
) -> Result<String> {
    let mut request = url
        .as_str()
        .into_client_request()
        .with_context(|| format!("invalid realtime websocket URL: {url}"))?;
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}")).context("invalid API key header")?,
    );
    request
        .headers_mut()
        .insert("OpenAI-Beta", HeaderValue::from_static("realtime=v1"));

    let (socket, _) = connect_async(request)
        .await
        .with_context(|| format!("failed to connect to realtime websocket {url}"))?;
    let (mut writer, mut reader) = socket.split();

    send_json(
        &mut writer,
        json!({
            "type": "session.update",
            "session": build_session_payload(&model, language.as_deref()),
        }),
    )
    .await?;

    let mut state = TranscriptState::default();
    let mut finish_state: Option<FinishState> = None;

    loop {
        while let Ok(command) = receiver.try_recv() {
            match command {
                WorkerCommand::Audio {
                    samples,
                    sample_rate,
                } => {
                    if finish_state.is_none() {
                        send_audio_chunk(&mut writer, &samples, sample_rate).await?;
                    }
                }
                WorkerCommand::Finish => {
                    if finish_state.is_none() {
                        let _ = send_json(
                            &mut writer,
                            json!({
                                "type": "input_audio_buffer.commit"
                            }),
                        )
                        .await;
                        let now = Instant::now();
                        finish_state = Some(FinishState {
                            deadline: now + timeout,
                            quiet_until: now
                                + if state.has_text() {
                                    Duration::from_millis(600)
                                } else {
                                    Duration::from_millis(1500)
                                },
                        });
                    }
                }
                WorkerCommand::Abort => {
                    let _ = writer.close().await;
                    return Ok(String::new());
                }
            }
        }

        if let Some(finish) = finish_state.as_ref() {
            if Instant::now() >= finish.deadline || Instant::now() >= finish.quiet_until {
                let _ = writer.close().await;
                let text = state.final_text();
                if text.is_empty() {
                    if let Some(error) = state.last_error.take() {
                        bail!("realtime websocket transcription failed: {error}");
                    }
                }
                return Ok(text);
            }
        }

        let wait_time = finish_state
            .as_ref()
            .map(|finish| {
                let now = Instant::now();
                let until = finish.quiet_until.min(finish.deadline);
                until
                    .saturating_duration_since(now)
                    .min(Duration::from_millis(250))
            })
            .unwrap_or_else(|| Duration::from_millis(100));

        match tokio::time::timeout(wait_time, reader.next()).await {
            Ok(Some(Ok(message))) => {
                if let Some(text) = handle_message(message, &mut state) {
                    eprintln!("[realtime] {text}");
                }
                if let Some(finish) = finish_state.as_mut() {
                    finish.quiet_until = Instant::now() + Duration::from_millis(650);
                }
            }
            Ok(Some(Err(error))) => {
                if finish_state.is_some() && state.has_text() {
                    break;
                }
                return Err(error).context("realtime websocket read failed");
            }
            Ok(None) => break,
            Err(_) => {}
        }
    }

    let text = state.final_text();
    if text.is_empty() {
        if let Some(error) = state.last_error.take() {
            bail!("realtime websocket transcription failed: {error}");
        }
    }
    Ok(text)
}

fn resolve_websocket_url(config: &AppConfig, provider: &str) -> Result<String> {
    if let Some(url) = config.websocket_url.as_ref() {
        if provider == "openai" && !url.contains('?') {
            return Ok(format!("{url}?intent=transcription"));
        }
        return Ok(url.clone());
    }

    match provider {
        "openai" => Ok(OPENAI_REALTIME_URL.to_string()),
        "custom" => bail!("custom realtime websocket backends require websocket_url"),
        other => bail!("unsupported realtime websocket provider: {other}"),
    }
}

fn build_session_payload(model: &str, language: Option<&str>) -> Value {
    let mut transcription = json!({ "model": model });
    if let Some(language) = language {
        transcription["language"] = Value::String(language.to_string());
    }

    json!({
        "type": "transcription",
        "audio": {
            "input": {
                "format": {
                    "type": "audio/pcm",
                    "rate": TARGET_SAMPLE_RATE
                },
                "transcription": transcription,
                "turn_detection": {
                    "type": "server_vad",
                    "threshold": 0.5,
                    "prefix_padding_ms": 300,
                    "silence_duration_ms": 500
                }
            }
        }
    })
}

async fn send_audio_chunk<S>(writer: &mut S, samples: &[f32], sample_rate: u32) -> Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let resampled = resample_linear(samples, sample_rate, TARGET_SAMPLE_RATE);
    let payload = json!({
        "type": "input_audio_buffer.append",
        "audio": BASE64_STANDARD.encode(float32_to_pcm16(&resampled)),
    });
    send_json(writer, payload).await
}

async fn send_json<S>(writer: &mut S, payload: Value) -> Result<()>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    writer
        .send(Message::Text(payload.to_string().into()))
        .await
        .context("failed to send realtime websocket event")
}

fn handle_message(message: Message, state: &mut TranscriptState) -> Option<String> {
    let Message::Text(text) = message else {
        return None;
    };
    let value: Value = serde_json::from_str(&text).ok()?;
    let event_type = value.get("type").and_then(Value::as_str)?;

    match event_type {
        "conversation.item.input_audio_transcription.delta" => {
            if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                state.partial_segment.push_str(delta);
            }
            None
        }
        "conversation.item.input_audio_transcription.completed" => {
            let transcript = value
                .get("transcript")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if !transcript.is_empty() {
                state.committed_segments.push(transcript);
            }
            state.partial_segment.clear();
            None
        }
        "error" => {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("unknown realtime websocket error")
                .to_string();
            state.last_error = Some(message.clone());
            Some(format!("server error: {message}"))
        }
        "input_audio_buffer.speech_started" => Some("speech detected".to_string()),
        "input_audio_buffer.speech_stopped" => Some("speech ended".to_string()),
        _ => None,
    }
}

fn float32_to_pcm16(audio: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(audio.len() * 2);
    for sample in audio {
        let clipped = sample.clamp(-1.0, 1.0);
        let pcm = (clipped * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&pcm.to_le_bytes());
    }
    bytes
}

impl TranscriptState {
    fn has_text(&self) -> bool {
        !self.committed_segments.is_empty() || !self.partial_segment.trim().is_empty()
    }

    fn final_text(&self) -> String {
        let mut parts = self.committed_segments.clone();
        let partial = self.partial_segment.trim();
        if !partial.is_empty() && parts.last().is_none_or(|segment| segment.trim() != partial) {
            parts.push(partial.to_string());
        }
        parts.join(" ").trim().to_string()
    }
}
