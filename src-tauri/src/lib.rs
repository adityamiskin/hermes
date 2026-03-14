use hermes::app::DictationApp;
use hermes::audio::{InputDeviceInfo, list_input_devices};
use hermes::config::AppConfig;
use hermes::credentials;
use hermes::ipc;
use hermes::paths::AppPaths;
use serde::Serialize;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use tauri::Manager;
use tauri::State;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
struct AppState {
    paths: AppPaths,
    daemon: DaemonManager,
}

struct DaemonManager {
    inner: Mutex<DaemonInner>,
}

struct DaemonInner {
    stop: Option<Arc<AtomicBool>>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialStatus {
    groq: bool,
    openai: bool,
    elevenlabs: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopSnapshot {
    config: AppConfig,
    devices: Vec<InputDeviceInfo>,
    provider_keys: CredentialStatus,
    recording: bool,
    daemon_running: bool,
    config_path: String,
    autostart_enabled: bool,
}

impl DaemonManager {
    fn new() -> Self {
        Self {
            inner: Mutex::new(DaemonInner {
                stop: None,
                thread: None,
            }),
        }
    }

    fn start(&self, paths: &AppPaths) -> Result<(), String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "daemon lock poisoned".to_string())?;
        if inner.thread.is_some() {
            return Ok(());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let paths_for_thread = paths.clone();
        let thread = std::thread::spawn(move || {
            let result = AppConfig::load(&paths_for_thread)
                .and_then(|config| DictationApp::new(paths_for_thread.clone(), config))
                .and_then(|app| app.run_daemon_until(stop_for_thread));
            if let Err(error) = result {
                eprintln!("[desktop] daemon exited with error: {error}");
            }
        });

        inner.stop = Some(stop);
        inner.thread = Some(thread);
        Ok(())
    }

    fn restart(&self, paths: &AppPaths) -> Result<(), String> {
        self.stop()?;
        self.start(paths)
    }

    fn stop(&self) -> Result<(), String> {
        let (stop, thread) = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "daemon lock poisoned".to_string())?;
            (inner.stop.take(), inner.thread.take())
        };

        if let Some(stop) = stop {
            stop.store(true, Ordering::SeqCst);
        }

        if let Some(thread) = thread {
            let _ = thread.join();
        }

        Ok(())
    }
}

impl Drop for DaemonManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn desktop_snapshot(paths: &AppPaths, app: &tauri::AppHandle) -> Result<DesktopSnapshot, String> {
    let config = AppConfig::load(paths).map_err(|error| error.to_string())?;
    let devices = list_input_devices().map_err(|error| error.to_string())?;
    let provider_keys = CredentialStatus {
        groq: credentials::get_credential(paths, "groq")
            .map_err(|error| error.to_string())?
            .is_some(),
        openai: credentials::get_credential(paths, "openai")
            .map_err(|error| error.to_string())?
            .is_some(),
        elevenlabs: credentials::get_credential(paths, "elevenlabs")
            .map_err(|error| error.to_string())?
            .is_some(),
    };

    Ok(DesktopSnapshot {
        config_path: paths.config_file.display().to_string(),
        config,
        devices,
        provider_keys,
        recording: ipc::is_recording(paths),
        daemon_running: ipc::heartbeat_is_fresh(paths, Duration::from_secs(3)),
        autostart_enabled: app.autolaunch().is_enabled().unwrap_or(false),
    })
}

fn send_record_control(paths: &AppPaths, command: &str) -> Result<(), String> {
    ipc::send_control(paths, command).map_err(|error| error.to_string())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn get_overview(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopSnapshot, String> {
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn save_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<DesktopSnapshot, String> {
    config.save(&state.paths).map_err(|error| error.to_string())?;
    state.daemon.restart(&state.paths)?;
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn save_provider_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
    key: String,
) -> Result<DesktopSnapshot, String> {
    credentials::save_credential(&state.paths, &provider, &key)
        .map_err(|error| error.to_string())?;
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn set_autostart_enabled(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<DesktopSnapshot, String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|error| error.to_string())?;
    } else {
        manager.disable().map_err(|error| error.to_string())?;
    }
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn start_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopSnapshot, String> {
    state.daemon.start(&state.paths)?;
    send_record_control(&state.paths, "start")?;
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn stop_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopSnapshot, String> {
    send_record_control(&state.paths, "stop")?;
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn cancel_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopSnapshot, String> {
    send_record_control(&state.paths, "cancel")?;
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn toggle_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopSnapshot, String> {
    state.daemon.start(&state.paths)?;
    let command = if ipc::is_recording(&state.paths) {
        "stop"
    } else {
        "start"
    };
    send_record_control(&state.paths, command)?;
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn restart_daemon(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopSnapshot, String> {
    state.daemon.restart(&state.paths)?;
    desktop_snapshot(&state.paths, &app)
}

#[tauri::command]
fn ensure_daemon(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<DesktopSnapshot, String> {
    state.daemon.start(&state.paths)?;
    desktop_snapshot(&state.paths, &app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::discover().expect("failed to resolve Hermes paths");
    let state = AppState {
        paths: paths.clone(),
        daemon: DaemonManager::new(),
    };

    tauri::Builder::default()
        .manage(state)
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let toggle = MenuItemBuilder::with_id("toggle", "Start / Stop").build(app)?;
            let settings = MenuItemBuilder::with_id("settings", "Open Hermes").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&toggle, &settings, &quit])
                .build()?;

            TrayIconBuilder::new()
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app: &tauri::AppHandle, event: tauri::menu::MenuEvent| match event.id().as_ref() {
                    "toggle" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            let _ = state.daemon.start(&state.paths);
                            let command = if ipc::is_recording(&state.paths) {
                                "stop"
                            } else {
                                "start"
                            };
                            let _ = send_record_control(&state.paths, command);
                        }
                    }
                    "settings" => show_main_window(app),
                    "quit" => {
                        if let Some(state) = app.try_state::<AppState>() {
                            let _ = state.daemon.stop();
                        }
                        app.exit(0)
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray: &tauri::tray::TrayIcon, event: TrayIconEvent| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            if let Some(state) = app.try_state::<AppState>() {
                let _ = state.daemon.start(&state.paths);
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_overview,
            save_config,
            save_provider_key,
            set_autostart_enabled,
            start_recording,
            stop_recording,
            cancel_recording,
            toggle_recording,
            restart_daemon,
            ensure_daemon
        ])
        .run(tauri::generate_context!())
        .expect("error while running Hermes desktop");
}
