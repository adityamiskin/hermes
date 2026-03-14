use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use hermes::app::DictationApp;
use hermes::audio::AudioRecorder;
use hermes::backend::BackendManager;
use hermes::config::AppConfig;
use hermes::credentials;
use hermes::ipc;
use hermes::overlay;
use hermes::paths::AppPaths;
#[derive(Debug, Parser)]
#[command(
    name = "hermes",
    version,
    about = "Native speech-to-text CLI and daemon",
    long_about = "Hermes is a native speech-to-text engine with daemon controls, configuration management, transcription commands, and credential storage."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Run the long-lived Hermes dictation daemon")]
    Daemon,
    #[command(hide = true)]
    Overlay,
    #[command(about = "Initialize, inspect, and edit Hermes configuration")]
    Config(ConfigCommand),
    #[command(about = "Send recording controls to the running daemon")]
    Record(RecordArgs),
    #[command(about = "Show runtime and backend status")]
    Status,
    #[command(about = "Validate microphone and backend initialization")]
    Validate,
    #[command(about = "Transcribe a mono 16 kHz WAV file once")]
    Transcribe(TranscribeArgs),
    #[command(about = "Store provider API keys in OS keychain")]
    Credentials(CredentialsArgs),
}

#[derive(Debug, Args)]
struct RecordArgs {
    #[command(subcommand)]
    action: RecordCommand,
}

#[derive(Debug, Subcommand)]
enum RecordCommand {
    #[command(about = "Start recording")]
    Start {
        #[arg(long = "lang", help = "Temporary language override (example: en)")]
        language: Option<String>,
    },
    #[command(about = "Stop recording and transcribe current buffer")]
    Stop,
    #[command(about = "Submit long-form session buffer")]
    Submit,
    #[command(about = "Cancel active recording without transcription")]
    Cancel,
    #[command(about = "Toggle recording state")]
    Toggle {
        #[arg(long = "lang", help = "Temporary language override (example: en)")]
        language: Option<String>,
    },
    #[command(about = "Print recording state (recording/idle)")]
    Status,
}

#[derive(Debug, Args)]
struct ConfigCommand {
    #[command(subcommand)]
    action: ConfigAction,
}

#[derive(Debug, Subcommand)]
enum ConfigAction {
    #[command(about = "Create default config file")]
    Init,
    #[command(about = "Print active config as JSON")]
    Show,
    #[command(about = "Open config in your default editor")]
    Edit,
    #[command(about = "Configure Groq REST backend and store Groq API key")]
    UseGroq {
        #[arg(long, help = "Groq API key")]
        api_key: String,
        #[arg(long, default_value = "whisper-large-v3-turbo", help = "Groq model id")]
        model: String,
        #[arg(long, help = "Default language code (example: en)")]
        language: Option<String>,
    },
}

#[derive(Debug, Args)]
struct TranscribeArgs {
    #[arg(help = "Path to mono 16 kHz WAV file")]
    file: std::path::PathBuf,
    #[arg(long, help = "Optional language override (example: en)")]
    language: Option<String>,
}

#[derive(Debug, Args)]
struct CredentialsArgs {
    #[arg(help = "Provider name (groq/openai/elevenlabs)")]
    provider: String,
    #[arg(long, help = "Provider API key")]
    key: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = AppPaths::discover()?;

    match cli.command {
        Command::Daemon => {
            let config = AppConfig::load(&paths)?;
            DictationApp::new(paths, config)?.run_daemon()?;
        }
        Command::Overlay => {
            overlay::run(paths)?;
        }
        Command::Config(command) => match command.action {
            ConfigAction::Init => {
                AppConfig::default().save(&paths)?;
                println!("initialized {}", paths.config_file.display());
            }
            ConfigAction::Show => {
                let config = AppConfig::load(&paths)?;
                println!("{}", serde_json::to_string_pretty(&config)?);
            }
            ConfigAction::Edit => {
                let config = AppConfig::load(&paths)?;
                config.edit(&paths)?;
            }
            ConfigAction::UseGroq {
                api_key,
                model,
                language,
            } => {
                let mut config = AppConfig::load(&paths)?;
                config.transcription_backend = "rest-api".to_string();
                config.rest_api_provider = Some("groq".to_string());
                config.rest_endpoint_url = None;
                config.rest_api_key = None;
                config.language = language;
                config.rest_body.insert(
                    "model".to_string(),
                    serde_json::Value::String(model.clone()),
                );
                config.save(&paths)?;
                credentials::save_credential(&paths, "groq", &api_key)?;
                println!("configured Groq transcription with model {model}");
            }
        },
        Command::Record(args) => {
            paths.ensure()?;
            match args.action {
                RecordCommand::Start { language } => {
                    let command = match language {
                        Some(language) => format!("start:{language}"),
                        None => "start".to_string(),
                    };
                    ipc::send_control(&paths, &command)?;
                    println!("sent {command}");
                }
                RecordCommand::Stop => {
                    ipc::send_control(&paths, "stop")?;
                    println!("sent stop");
                }
                RecordCommand::Submit => {
                    ipc::send_control(&paths, "submit")?;
                    println!("sent submit");
                }
                RecordCommand::Cancel => {
                    ipc::send_control(&paths, "cancel")?;
                    println!("sent cancel");
                }
                RecordCommand::Toggle { language } => {
                    let command = if ipc::is_recording(&paths) {
                        "stop".to_string()
                    } else {
                        match language {
                            Some(language) => format!("start:{language}"),
                            None => "start".to_string(),
                        }
                    };
                    ipc::send_control(&paths, &command)?;
                    println!("sent {command}");
                }
                RecordCommand::Status => {
                    println!(
                        "{}",
                        if ipc::is_recording(&paths) {
                            "recording"
                        } else {
                            "idle"
                        }
                    );
                }
            }
        }
        Command::Status => {
            let config = AppConfig::load(&paths)?;
            println!("config: {}", paths.config_file.display());
            println!("backend: {}", config.transcription_backend);
            if let Some(provider) = config.rest_api_provider {
                println!("provider: {provider}");
            }
            println!(
                "recording: {}",
                if ipc::is_recording(&paths) {
                    "yes"
                } else {
                    "no"
                }
            );
            println!("control: {}", paths.control_file.display());
            println!("model: {}", config.model);
            if let Some(model_path) = config.model_path {
                println!("model_path: {model_path}");
            }
        }
        Command::Validate => {
            let config = AppConfig::load(&paths)?;
            let _ = AudioRecorder::start(&config).context("microphone validation failed")?;
            let _ = BackendManager::new(paths.clone(), &config)?;
            println!("validation passed");
        }
        Command::Transcribe(args) => {
            let config = AppConfig::load(&paths)?;
            let mut backend = BackendManager::new(paths.clone(), &config)?;
            let samples = load_wav_mono_16k(&args.file)?;
            let text = backend.transcribe(&config, &samples, args.language.as_deref())?;
            println!("{text}");
        }
        Command::Credentials(args) => {
            credentials::save_credential(&paths, &args.provider, &args.key)?;
            println!("saved credential for {}", args.provider);
        }
    }

    Ok(())
}

fn load_wav_mono_16k(path: &std::path::Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 {
        anyhow::bail!("expected mono 16kHz WAV input");
    }

    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .context("failed to read float WAV samples")?,
        hound::SampleFormat::Int => {
            let ints = reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .context("failed to read PCM WAV samples")?;
            ints.into_iter()
                .map(|sample| sample as f32 / i16::MAX as f32)
                .collect()
        }
    };
    Ok(samples)
}
