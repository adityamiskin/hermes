import { invoke } from "@tauri-apps/api/core";

export interface AppConfig {
  primary_shortcut: string;
  secondary_shortcut: string | null;
  secondary_language: string | null;
  cancel_shortcut: string | null;
  long_form_submit_shortcut: string | null;
  recording_mode: string;
  use_hypr_bindings: boolean;
  selected_device_name: string | null;
  audio_device_id: number | null;
  audio_device_name: string | null;
  model: string;
  model_path: string | null;
  threads: number;
  language: string | null;
  word_overrides: Record<string, string>;
  filter_filler_words: boolean;
  filler_words: string[];
  symbol_replacements: boolean;
  whisper_prompt: string;
  clipboard_behavior: boolean;
  clipboard_clear_delay: number;
  paste_mode: string;
  transcription_backend: string;
  rest_endpoint_url: string | null;
  rest_api_provider: string | null;
  rest_api_key: string | null;
  rest_headers: Record<string, string>;
  rest_body: Record<string, unknown>;
  rest_timeout: number;
  websocket_provider: string | null;
  websocket_model: string | null;
  websocket_url: string | null;
  realtime_timeout: number;
  realtime_buffer_max_seconds: number;
  realtime_mode: string;
  onnx_asr_model: string;
  onnx_asr_quantization: string | null;
  onnx_asr_use_vad: boolean;
  faster_whisper_model: string;
  faster_whisper_device: string;
  faster_whisper_compute_type: string;
  faster_whisper_vad_filter: boolean;
  long_form_temp_limit_mb: number;
  long_form_auto_save_interval: number;
  auto_submit: boolean;
  show_overlay: boolean;
}

export interface InputDeviceInfo {
  id: number;
  name: string;
  is_default: boolean;
}

export interface DesktopOverview {
  config: AppConfig;
  configPath: string;
  recording: boolean;
  daemonRunning: boolean;
  autostartEnabled: boolean;
  devices: InputDeviceInfo[];
  providerKeys: Record<string, boolean>;
}

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

const mockOverview: DesktopOverview = {
  configPath: "~/.config/hermes/config.json",
  recording: false,
  daemonRunning: true,
  autostartEnabled: false,
  devices: [
    { id: 0, name: "Built-in Microphone", is_default: true },
    { id: 1, name: "USB Audio Interface", is_default: false },
  ],
  providerKeys: {
    groq: true,
    openai: false,
    elevenlabs: false,
  },
  config: {
    primary_shortcut: "SUPER+ALT+S",
    secondary_shortcut: null,
    secondary_language: null,
    cancel_shortcut: "ESC",
    long_form_submit_shortcut: "SUPER+ALT+E",
    recording_mode: "toggle",
    use_hypr_bindings: false,
    selected_device_name: null,
    audio_device_id: 0,
    audio_device_name: "Built-in Microphone",
    model: "base",
    model_path: null,
    threads: 4,
    language: "en",
    word_overrides: {},
    filter_filler_words: false,
    filler_words: ["uh", "um", "er", "ah", "eh", "hmm", "hm"],
    symbol_replacements: true,
    whisper_prompt: "Transcribe with proper capitalization and punctuation.",
    clipboard_behavior: false,
    clipboard_clear_delay: 5,
    paste_mode: "ctrl_shift",
    transcription_backend: "rest-api",
    rest_endpoint_url: null,
    rest_api_provider: "groq",
    rest_api_key: null,
    rest_headers: {},
    rest_body: {
      model: "whisper-large-v3-turbo",
    },
    rest_timeout: 30,
    websocket_provider: "openai",
    websocket_model: "gpt-4o-mini-transcribe",
    websocket_url: null,
    realtime_timeout: 30,
    realtime_buffer_max_seconds: 5,
    realtime_mode: "transcribe",
    onnx_asr_model: "nemo-parakeet-tdt-0.6b-v3",
    onnx_asr_quantization: "int8",
    onnx_asr_use_vad: true,
    faster_whisper_model: "base",
    faster_whisper_device: "auto",
    faster_whisper_compute_type: "auto",
    faster_whisper_vad_filter: true,
    long_form_temp_limit_mb: 500,
    long_form_auto_save_interval: 300,
    auto_submit: false,
    show_overlay: true,
  },
};

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getOverview(): Promise<DesktopOverview> {
  if (!isTauriRuntime()) {
    return structuredClone(mockOverview);
  }

  return invoke<DesktopOverview>("get_overview");
}

export async function saveConfig(config: AppConfig): Promise<DesktopOverview> {
  if (!isTauriRuntime()) {
    mockOverview.config = structuredClone(config);
    return structuredClone(mockOverview);
  }

  return invoke<DesktopOverview>("save_config", { config });
}

export async function saveProviderKey(
  provider: string,
  key: string,
): Promise<DesktopOverview> {
  if (!isTauriRuntime()) {
    mockOverview.providerKeys[provider] = key.trim().length > 0;
    return structuredClone(mockOverview);
  }

  return invoke<DesktopOverview>("save_provider_key", { provider, key });
}

export async function toggleRecording(): Promise<DesktopOverview> {
  if (!isTauriRuntime()) {
    mockOverview.recording = !mockOverview.recording;
    return structuredClone(mockOverview);
  }

  return invoke<DesktopOverview>("toggle_recording");
}

export async function restartDaemon(): Promise<DesktopOverview> {
  if (!isTauriRuntime()) {
    mockOverview.daemonRunning = true;
    return structuredClone(mockOverview);
  }

  return invoke<DesktopOverview>("restart_daemon");
}

export async function setAutostartEnabled(
  enabled: boolean,
): Promise<DesktopOverview> {
  if (!isTauriRuntime()) {
    mockOverview.autostartEnabled = enabled;
    return structuredClone(mockOverview);
  }

  return invoke<DesktopOverview>("set_autostart_enabled", { enabled });
}
