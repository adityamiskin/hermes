use crate::audio::AudioRecorder;
use crate::backend::BackendManager;
use crate::config::AppConfig;
use crate::feedback;
use crate::hotkeys::{HotkeyCommand, HotkeyService};
use crate::ipc;
use crate::longform::SegmentStore;
use crate::overlay;
use crate::paths::AppPaths;
use crate::text_injector::TextInjector;
use anyhow::{Context, Result, bail};
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub struct DictationApp {
    paths: AppPaths,
    config: AppConfig,
    backend: BackendManager,
    injector: TextInjector,
    overlay_process: Option<Child>,
    active: Option<ActiveRecording>,
    long_form: Option<LongFormSession>,
}

struct ActiveRecording {
    recorder: AudioRecorder,
    realtime_session: Option<crate::realtime::RealtimeSession>,
    language_override: Option<String>,
    started_at: Instant,
}

struct LongFormSession {
    store: SegmentStore,
    language_override: Option<String>,
}

impl DictationApp {
    pub fn new(paths: AppPaths, config: AppConfig) -> Result<Self> {
        let backend = BackendManager::new(paths.clone(), &config)?;
        let injector = TextInjector::new()?;
        Ok(Self {
            paths,
            config,
            backend,
            injector,
            overlay_process: None,
            active: None,
            long_form: None,
        })
    }

    pub fn run_daemon(self) -> Result<()> {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        ctrlc::set_handler(move || {
            signal.store(true, Ordering::SeqCst);
        })
        .context("failed to install Ctrl-C handler")?;
        self.run_daemon_until(stop)
    }

    pub fn run_daemon_until(mut self, stop: Arc<AtomicBool>) -> Result<()> {
        self.paths.ensure()?;
        ipc::reset_runtime_state(&self.paths);
        ipc::write_daemon_heartbeat(&self.paths)?;

        let hotkeys = if self.config.use_hypr_bindings {
            None
        } else {
            match HotkeyService::new(&self.config) {
                Ok(service) => Some(service),
                Err(error) => {
                    eprintln!("[hotkeys] initialization failed: {error}");
                    None
                }
            }
        };
        while !stop.load(Ordering::SeqCst) {
            ipc::write_daemon_heartbeat(&self.paths)?;

            if let Some(command) = ipc::take_control(&self.paths)? {
                self.handle_command(&command)?;
            }

            if let Some(hotkeys) = hotkeys.as_ref() {
                if let Some(command) = hotkeys.try_next_command() {
                    self.handle_hotkey(command)?;
                }
            }

            self.maybe_rollover_long_form_segment()?;
            self.update_audio_level();
            thread::sleep(Duration::from_millis(100));
        }

        self.cancel_recording().ok();
        self.stop_overlay();
        ipc::reset_runtime_state(&self.paths);
        Ok(())
    }

    pub fn handle_command(&mut self, command: &str) -> Result<()> {
        let normalized = command.trim();
        match normalized.split_once(':') {
            Some(("start", language)) => self.start_recording(Some(language.trim().to_string())),
            _ => match normalized.to_lowercase().as_str() {
                "start" => self.start_recording(None),
                "stop" => self.stop_recording(),
                "submit" => self.submit_recording(),
                "cancel" => self.cancel_recording(),
                "toggle" => {
                    if self.active.is_some() {
                        self.stop_recording()
                    } else {
                        self.start_recording(None)
                    }
                }
                "" => Ok(()),
                other => Err(anyhow::anyhow!("unknown control command: {other}")),
            },
        }
    }

    fn handle_hotkey(&mut self, command: HotkeyCommand) -> Result<()> {
        match command {
            HotkeyCommand::Toggle => {
                if self.active.is_some() {
                    self.stop_recording()
                } else {
                    self.start_recording(None)
                }
            }
            HotkeyCommand::Start(language) => self.start_recording(language),
            HotkeyCommand::Stop => self.stop_recording(),
            HotkeyCommand::Cancel => self.cancel_recording(),
            HotkeyCommand::Submit => self.submit_recording(),
        }
    }

    fn start_recording(&mut self, language_override: Option<String>) -> Result<()> {
        if self.is_long_form_mode() && self.backend.is_realtime_backend(&self.config) {
            bail!("realtime websocket transcription is not supported in long_form mode yet");
        }
        if self.is_long_form_mode() {
            self.start_or_resume_long_form(language_override)
        } else {
            self.start_standard_recording(language_override)
        }
    }

    fn stop_recording(&mut self) -> Result<()> {
        if self.is_long_form_mode() {
            self.pause_long_form()
        } else {
            self.stop_standard_recording()
        }
    }

    fn submit_recording(&mut self) -> Result<()> {
        if self.is_long_form_mode() {
            self.submit_long_form()
        } else {
            self.stop_standard_recording()
        }
    }

    fn start_standard_recording(&mut self, language_override: Option<String>) -> Result<()> {
        if self.active.is_some() {
            return Ok(());
        }
        ipc::clear_zero_volume(&self.paths);
        let realtime_session = self
            .backend
            .start_realtime_session(&self.config, language_override.as_deref())?;
        let recorder = if let Some(session) = realtime_session.as_ref() {
            AudioRecorder::start_with_sink(&self.config, Some(session.audio_sink()))?
        } else {
            AudioRecorder::start(&self.config)?
        };
        self.ensure_overlay_running();
        ipc::set_recording_status(&self.paths, true)?;
        self.active = Some(ActiveRecording {
            recorder,
            realtime_session,
            language_override,
            started_at: Instant::now(),
        });
        println!("recording started");
        Ok(())
    }

    fn stop_standard_recording(&mut self) -> Result<()> {
        let Some(active) = self.active.take() else {
            return Ok(());
        };

        self.stop_overlay();
        ipc::set_recording_status(&self.paths, false)?;
        ipc::clear_audio_level(&self.paths);
        let audio = active.recorder.stop()?;
        if is_zero_volume(&audio) {
            ipc::mark_zero_volume(&self.paths, "No microphone signal detected")?;
            if let Some(session) = active.realtime_session {
                let _ = session.abort();
            }
            feedback::play_completion_tone();
            println!("recording stopped: no usable audio");
            return Ok(());
        }

        let transcript = if let Some(session) = active.realtime_session {
            session.finish()?
        } else {
            self.backend
                .transcribe(&self.config, &audio, active.language_override.as_deref())?
        };
        let processed = preprocess_text(&self.config, &transcript);
        self.inject_text(&processed)?;
        feedback::play_completion_tone();
        println!("recording stopped");
        Ok(())
    }

    fn start_or_resume_long_form(&mut self, language_override: Option<String>) -> Result<()> {
        if self.active.is_some() {
            return Ok(());
        }

        let existing_session = self.long_form.is_some();
        if self.long_form.is_none() {
            let store = SegmentStore::new(&self.paths, self.config.long_form_temp_limit_mb)?;
            self.long_form = Some(LongFormSession {
                store,
                language_override: language_override.clone(),
            });
        } else if let Some(language) = language_override.clone() {
            if let Some(session) = self.long_form.as_mut() {
                session.language_override = Some(language);
            }
        }

        let session_language = self
            .long_form
            .as_ref()
            .and_then(|session| session.language_override.clone());

        ipc::clear_zero_volume(&self.paths);
        let recorder = AudioRecorder::start(&self.config)?;
        self.ensure_overlay_running();
        ipc::set_recording_status(&self.paths, true)?;
        self.active = Some(ActiveRecording {
            recorder,
            realtime_session: None,
            language_override: session_language,
            started_at: Instant::now(),
        });

        if existing_session {
            println!("long-form recording resumed");
        } else {
            println!("long-form recording started");
        }
        Ok(())
    }

    fn pause_long_form(&mut self) -> Result<()> {
        self.stop_active_long_form_segment(true, true)?;
        Ok(())
    }

    fn submit_long_form(&mut self) -> Result<()> {
        if self.long_form.is_none() && self.active.is_none() {
            return Ok(());
        }

        self.stop_active_long_form_segment(false, false)?;

        let Some(mut session) = self.long_form.take() else {
            return Ok(());
        };
        let audio = session.store.concatenate()?;
        if is_zero_volume(&audio) {
            ipc::mark_zero_volume(&self.paths, "No microphone signal detected")?;
            session.store.clear_session();
            feedback::play_completion_tone();
            println!("long-form submit: no usable audio");
            return Ok(());
        }

        let transcript =
            self.backend
                .transcribe(&self.config, &audio, session.language_override.as_deref())?;
        let processed = preprocess_text(&self.config, &transcript);
        self.inject_text(&processed)?;
        session.store.clear_session();
        feedback::play_completion_tone();
        println!("long-form recording submitted");
        Ok(())
    }

    fn stop_active_long_form_segment(
        &mut self,
        emit_pause_message: bool,
        play_tone: bool,
    ) -> Result<()> {
        let Some(active) = self.active.take() else {
            return Ok(());
        };

        self.stop_overlay();
        ipc::set_recording_status(&self.paths, false)?;
        ipc::clear_audio_level(&self.paths);
        let audio = active.recorder.stop()?;
        if !is_zero_volume(&audio) {
            if let Some(session) = self.long_form.as_mut() {
                session.store.save_segment(&audio)?;
            }
        }

        if play_tone {
            feedback::play_completion_tone();
        }
        if emit_pause_message {
            println!("long-form recording paused");
        }
        Ok(())
    }

    fn maybe_rollover_long_form_segment(&mut self) -> Result<()> {
        if !self.is_long_form_mode() || self.long_form.is_none() {
            return Ok(());
        }

        let interval = self.config.long_form_auto_save_interval;
        if interval == 0 {
            return Ok(());
        }

        let should_roll = self
            .active
            .as_ref()
            .is_some_and(|active| active.started_at.elapsed() >= Duration::from_secs(interval));
        if !should_roll {
            return Ok(());
        }

        let Some(active) = self.active.take() else {
            return Ok(());
        };

        let session_language = self
            .long_form
            .as_ref()
            .and_then(|session| session.language_override.clone());
        let audio = active.recorder.stop()?;
        if !is_zero_volume(&audio) {
            if let Some(session) = self.long_form.as_mut() {
                session.store.save_segment(&audio)?;
            }
        }

        match AudioRecorder::start(&self.config) {
            Ok(recorder) => {
                self.ensure_overlay_running();
                ipc::set_recording_status(&self.paths, true)?;
                self.active = Some(ActiveRecording {
                    recorder,
                    realtime_session: None,
                    language_override: session_language,
                    started_at: Instant::now(),
                });
                println!("long-form auto-saved segment");
            }
            Err(error) => {
                ipc::set_recording_status(&self.paths, false)?;
                ipc::clear_audio_level(&self.paths);
                eprintln!("[long-form] failed to resume after auto-save: {error}");
            }
        }

        Ok(())
    }

    fn cancel_recording(&mut self) -> Result<()> {
        if let Some(active) = self.active.take() {
            if let Some(session) = active.realtime_session {
                let _ = session.abort();
            }
            let _ = active.recorder.stop();
        }

        if let Some(mut session) = self.long_form.take() {
            session.store.clear_session();
        }

        self.stop_overlay();
        ipc::set_recording_status(&self.paths, false)?;
        ipc::clear_audio_level(&self.paths);
        println!("recording cancelled");
        Ok(())
    }

    fn update_audio_level(&self) {
        if let Some(active) = self.active.as_ref() {
            let _ = ipc::write_audio_level(&self.paths, active.recorder.current_level());
        }
    }

    fn inject_text(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        self.injector.inject_text(&self.config, text)
    }

    fn is_long_form_mode(&self) -> bool {
        self.config.recording_mode.eq_ignore_ascii_case("long_form")
    }

    fn ensure_overlay_running(&mut self) {
        if !self.config.show_overlay || self.overlay_process.is_some() {
            return;
        }

        self.overlay_process = match overlay::spawn() {
            Ok(child) => Some(child),
            Err(error) => {
                eprintln!("[overlay] failed to start: {error}");
                None
            }
        };
    }

    fn stop_overlay(&mut self) {
        if let Some(mut child) = self.overlay_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn is_zero_volume(audio: &[f32]) -> bool {
    if audio.is_empty() {
        return true;
    }
    audio.iter().all(|sample| sample.abs() < 1e-6)
}

fn preprocess_text(config: &AppConfig, text: &str) -> String {
    let mut output = text.trim().to_string();
    if output.is_empty() {
        return output;
    }

    if config.symbol_replacements {
        output = apply_symbol_replacements(&output);
    }

    for (from, to) in &config.word_overrides {
        output = output.replace(from, to);
    }

    if config.filter_filler_words {
        let fillers: Vec<String> = config
            .filler_words
            .iter()
            .map(|word| word.to_lowercase())
            .collect();
        output = output
            .split_whitespace()
            .filter(|word| {
                let normalized = word
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                !fillers.contains(&normalized)
            })
            .collect::<Vec<_>>()
            .join(" ");
    }

    if !output.ends_with(' ') {
        output.push(' ');
    }
    output
}

fn apply_symbol_replacements(text: &str) -> String {
    let replacements = [
        (" comma", ","),
        (" period", "."),
        (" question mark", "?"),
        (" exclamation mark", "!"),
        (" colon", ":"),
        (" semicolon", ";"),
        (" open parenthesis", " ("),
        (" close parenthesis", ")"),
        (" new line", "\n"),
    ];

    let mut output = format!(" {}", text);
    for (from, to) in replacements {
        output = output.replace(from, to);
    }
    output.trim().to_string()
}
