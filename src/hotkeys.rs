use crate::config::AppConfig;
use anyhow::{Context, Result};
#[cfg(not(target_os = "linux"))]
use global_hotkey::hotkey::HotKey;
#[cfg(not(target_os = "linux"))]
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
#[cfg(target_os = "linux")]
use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::mpsc::{self, Receiver};
#[cfg(target_os = "linux")]
use std::thread::{self, JoinHandle};
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use evdev::{Device, EventSummary, KeyCode};

#[derive(Debug, Clone)]
pub enum HotkeyCommand {
    Toggle,
    Start(Option<String>),
    Stop,
    Cancel,
    Submit,
}

pub struct HotkeyService {
    mode: String,
    secondary_language: Option<String>,
    backend: HotkeyBackend,
}

enum HotkeyBackend {
    #[cfg(target_os = "linux")]
    Linux(LinuxHotkeyBackend),
    #[cfg(not(target_os = "linux"))]
    Global(GlobalHotkeyBackend),
}

impl HotkeyService {
    pub fn new(config: &AppConfig) -> Result<Self> {
        #[cfg(target_os = "linux")]
        let backend = HotkeyBackend::Linux(LinuxHotkeyBackend::new(config)?);

        #[cfg(not(target_os = "linux"))]
        let backend = HotkeyBackend::Global(GlobalHotkeyBackend::new(config)?);

        Ok(Self {
            mode: config.recording_mode.clone(),
            secondary_language: config.secondary_language.clone(),
            backend,
        })
    }

    pub fn try_next_command(&self) -> Option<HotkeyCommand> {
        match &self.backend {
            #[cfg(target_os = "linux")]
            HotkeyBackend::Linux(backend) => {
                let event = backend.try_recv()?;
                match event.slot {
                    HotkeySlot::Cancel if event.pressed => Some(HotkeyCommand::Cancel),
                    HotkeySlot::Submit if event.pressed => Some(HotkeyCommand::Submit),
                    HotkeySlot::Secondary => match_hotkey_event(
                        &self.mode,
                        event.pressed,
                        self.secondary_language.clone(),
                    ),
                    HotkeySlot::Primary => match_hotkey_event(&self.mode, event.pressed, None),
                    HotkeySlot::Cancel | HotkeySlot::Submit => None,
                }
            }
            #[cfg(not(target_os = "linux"))]
            HotkeyBackend::Global(backend) => {
                let event = backend.try_recv()?;
                if backend.cancel.map(|hotkey| hotkey.id()) == Some(event.id)
                    && event.state == HotKeyState::Pressed
                {
                    return Some(HotkeyCommand::Cancel);
                }

                if backend.submit.map(|hotkey| hotkey.id()) == Some(event.id)
                    && event.state == HotKeyState::Pressed
                {
                    return Some(HotkeyCommand::Submit);
                }

                if backend.secondary.map(|hotkey| hotkey.id()) == Some(event.id) {
                    return match_hotkey_event(
                        &self.mode,
                        event.state == HotKeyState::Pressed,
                        self.secondary_language.clone(),
                    );
                }

                if backend.primary.map(|hotkey| hotkey.id()) == Some(event.id) {
                    return match_hotkey_event(
                        &self.mode,
                        event.state == HotKeyState::Pressed,
                        None,
                    );
                }

                None
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
struct GlobalHotkeyBackend {
    _manager: GlobalHotKeyManager,
    primary: Option<HotKey>,
    secondary: Option<HotKey>,
    cancel: Option<HotKey>,
    submit: Option<HotKey>,
}

#[cfg(not(target_os = "linux"))]
impl GlobalHotkeyBackend {
    fn new(config: &AppConfig) -> Result<Self> {
        let manager = GlobalHotKeyManager::new().context("failed to initialize global hotkeys")?;
        let primary = parse_hotkey(&config.primary_shortcut)?;
        let secondary = config
            .secondary_shortcut
            .as_deref()
            .map(parse_hotkey)
            .transpose()?
            .flatten();
        let cancel = config
            .cancel_shortcut
            .as_deref()
            .map(parse_hotkey)
            .transpose()?
            .flatten();
        let submit = config
            .long_form_submit_shortcut
            .as_deref()
            .map(parse_hotkey)
            .transpose()?
            .flatten();

        if let Some(primary) = primary {
            manager.register(primary)?;
        }
        if let Some(secondary) = secondary {
            manager.register(secondary)?;
        }
        if let Some(cancel) = cancel {
            manager.register(cancel)?;
        }
        if let Some(submit) = submit {
            manager.register(submit)?;
        }

        Ok(Self {
            _manager: manager,
            primary,
            secondary,
            cancel,
            submit,
        })
    }

    fn try_recv(&self) -> Option<GlobalHotKeyEvent> {
        GlobalHotKeyEvent::receiver().try_recv().ok()
    }
}

#[cfg(not(target_os = "linux"))]
fn parse_hotkey(value: &str) -> Result<Option<HotKey>> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let parsed = value
        .replace("CTRL+", "Control+")
        .replace("SUPER+", "Super+")
        .replace("ALT+", "Alt+")
        .replace("SHIFT+", "Shift+");
    let hotkey = parsed
        .parse::<HotKey>()
        .with_context(|| format!("invalid hotkey: {value}"))?;
    Ok(Some(hotkey))
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HotkeySlot {
    Primary,
    Secondary,
    Cancel,
    Submit,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct LinuxHotkeyEvent {
    slot: HotkeySlot,
    pressed: bool,
}

#[cfg(target_os = "linux")]
struct LinuxHotkeyBackend {
    rx: Receiver<LinuxHotkeyEvent>,
    stop: Arc<AtomicBool>,
    _threads: Vec<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl LinuxHotkeyBackend {
    fn new(config: &AppConfig) -> Result<Self> {
        let primary = parse_linux_hotkey(&config.primary_shortcut)
            .with_context(|| format!("invalid hotkey: {}", config.primary_shortcut))?;
        let secondary = config
            .secondary_shortcut
            .as_deref()
            .map(parse_linux_hotkey)
            .transpose()?;
        let cancel = config
            .cancel_shortcut
            .as_deref()
            .map(parse_linux_hotkey)
            .transpose()?;
        let submit = config
            .long_form_submit_shortcut
            .as_deref()
            .map(parse_linux_hotkey)
            .transpose()?;

        let hotkeys = vec![
            primary.map(|hotkey| (HotkeySlot::Primary, hotkey)),
            secondary
                .flatten()
                .map(|hotkey| (HotkeySlot::Secondary, hotkey)),
            cancel.flatten().map(|hotkey| (HotkeySlot::Cancel, hotkey)),
            submit.flatten().map(|hotkey| (HotkeySlot::Submit, hotkey)),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

        if hotkeys.is_empty() {
            anyhow::bail!("no hotkeys configured");
        }

        let mut devices = discover_linux_devices(&hotkeys)?;
        if devices.is_empty() {
            anyhow::bail!(
                "no readable keyboard devices found under /dev/input; hotkeys need input-device access"
            );
        }

        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let mut threads = Vec::new();

        for (path, mut device) in devices.drain(..) {
            let stop_flag = Arc::clone(&stop);
            let tx = tx.clone();
            let hotkeys = hotkeys.clone();
            let handle = thread::spawn(move || {
                if let Err(error) = device.set_nonblocking(true) {
                    eprintln!("[hotkeys] failed to set nonblocking for {}: {error}", path);
                    return;
                }

                let mut detector = LinuxHotkeyDetector::new(hotkeys);
                while !stop_flag.load(Ordering::Relaxed) {
                    match device.fetch_events() {
                        Ok(events) => {
                            for event in events {
                                if let EventSummary::Key(_, code, value) = event.destructure() {
                                    if let Some(message) = detector.handle_key(code, value) {
                                        let _ = tx.send(message);
                                    }
                                }
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => {
                            eprintln!("[hotkeys] device {} stopped: {error}", path);
                            break;
                        }
                    }
                }
            });
            threads.push(handle);
        }

        println!(
            "[hotkeys] Linux evdev listener active on {} device(s)",
            threads.len()
        );

        Ok(Self {
            rx,
            stop,
            _threads: threads,
        })
    }

    fn try_recv(&self) -> Option<LinuxHotkeyEvent> {
        self.rx.try_recv().ok()
    }
}

#[cfg(target_os = "linux")]
impl Drop for LinuxHotkeyBackend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
struct LinuxHotkey {
    trigger: KeyCode,
    modifiers: Vec<KeyCode>,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxHotkeyState {
    slot: HotkeySlot,
    hotkey: LinuxHotkey,
    active: bool,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxHotkeyDetector {
    pressed: HashSet<KeyCode>,
    hotkeys: Vec<LinuxHotkeyState>,
}

#[cfg(target_os = "linux")]
impl LinuxHotkeyDetector {
    fn new(hotkeys: Vec<(HotkeySlot, LinuxHotkey)>) -> Self {
        Self {
            pressed: HashSet::new(),
            hotkeys: hotkeys
                .into_iter()
                .map(|(slot, hotkey)| LinuxHotkeyState {
                    slot,
                    hotkey,
                    active: false,
                })
                .collect(),
        }
    }

    fn handle_key(&mut self, code: KeyCode, value: i32) -> Option<LinuxHotkeyEvent> {
        match value {
            1 => {
                self.pressed.insert(code);
            }
            0 => {
                self.pressed.remove(&code);
            }
            2 => {}
            _ => return None,
        }

        for hotkey in &mut self.hotkeys {
            let is_active = self.pressed.contains(&hotkey.hotkey.trigger)
                && hotkey
                    .hotkey
                    .modifiers
                    .iter()
                    .all(|modifier| self.pressed.contains(modifier));

            if !hotkey.active && is_active {
                hotkey.active = true;
                return Some(LinuxHotkeyEvent {
                    slot: hotkey.slot,
                    pressed: true,
                });
            }

            if hotkey.active && !is_active {
                hotkey.active = false;
                return Some(LinuxHotkeyEvent {
                    slot: hotkey.slot,
                    pressed: false,
                });
            }
        }

        None
    }
}

#[cfg(target_os = "linux")]
fn discover_linux_devices(hotkeys: &[(HotkeySlot, LinuxHotkey)]) -> Result<Vec<(String, Device)>> {
    let required_keys = hotkeys
        .iter()
        .flat_map(|(_, hotkey)| {
            std::iter::once(hotkey.trigger).chain(hotkey.modifiers.iter().copied())
        })
        .collect::<HashSet<_>>();

    let devices = evdev::enumerate()
        .filter_map(|(path, device)| {
            let supported = device.supported_keys()?;
            let is_candidate = required_keys.iter().all(|key| supported.contains(*key));
            if is_candidate {
                Some((path.display().to_string(), device))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    Ok(devices)
}

#[cfg(target_os = "linux")]
fn parse_linux_hotkey(value: &str) -> Result<Option<LinuxHotkey>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    let mut parts = value
        .split('+')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return Ok(None);
    }

    let trigger = parse_linux_key(parts.pop().unwrap())
        .with_context(|| format!("unknown trigger key in hotkey: {value}"))?;
    let modifiers = parts
        .into_iter()
        .map(|part| {
            parse_linux_modifier(part)
                .with_context(|| format!("unknown modifier '{part}' in hotkey: {value}"))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(Some(LinuxHotkey { trigger, modifiers }))
}

#[cfg(target_os = "linux")]
fn parse_linux_modifier(value: &str) -> Result<KeyCode> {
    match value.to_ascii_uppercase().as_str() {
        "CTRL" | "CONTROL" => Ok(KeyCode::KEY_LEFTCTRL),
        "ALT" => Ok(KeyCode::KEY_LEFTALT),
        "SHIFT" => Ok(KeyCode::KEY_LEFTSHIFT),
        "SUPER" | "META" | "WIN" | "WINDOWS" | "CMD" => Ok(KeyCode::KEY_LEFTMETA),
        other => anyhow::bail!("unsupported modifier: {other}"),
    }
}

#[cfg(target_os = "linux")]
fn parse_linux_key(value: &str) -> Result<KeyCode> {
    match value.to_ascii_uppercase().as_str() {
        "A" => Ok(KeyCode::KEY_A),
        "B" => Ok(KeyCode::KEY_B),
        "C" => Ok(KeyCode::KEY_C),
        "D" => Ok(KeyCode::KEY_D),
        "E" => Ok(KeyCode::KEY_E),
        "F" => Ok(KeyCode::KEY_F),
        "G" => Ok(KeyCode::KEY_G),
        "H" => Ok(KeyCode::KEY_H),
        "I" => Ok(KeyCode::KEY_I),
        "J" => Ok(KeyCode::KEY_J),
        "K" => Ok(KeyCode::KEY_K),
        "L" => Ok(KeyCode::KEY_L),
        "M" => Ok(KeyCode::KEY_M),
        "N" => Ok(KeyCode::KEY_N),
        "O" => Ok(KeyCode::KEY_O),
        "P" => Ok(KeyCode::KEY_P),
        "Q" => Ok(KeyCode::KEY_Q),
        "R" => Ok(KeyCode::KEY_R),
        "S" => Ok(KeyCode::KEY_S),
        "T" => Ok(KeyCode::KEY_T),
        "U" => Ok(KeyCode::KEY_U),
        "V" => Ok(KeyCode::KEY_V),
        "W" => Ok(KeyCode::KEY_W),
        "X" => Ok(KeyCode::KEY_X),
        "Y" => Ok(KeyCode::KEY_Y),
        "Z" => Ok(KeyCode::KEY_Z),
        "0" => Ok(KeyCode::KEY_0),
        "1" => Ok(KeyCode::KEY_1),
        "2" => Ok(KeyCode::KEY_2),
        "3" => Ok(KeyCode::KEY_3),
        "4" => Ok(KeyCode::KEY_4),
        "5" => Ok(KeyCode::KEY_5),
        "6" => Ok(KeyCode::KEY_6),
        "7" => Ok(KeyCode::KEY_7),
        "8" => Ok(KeyCode::KEY_8),
        "9" => Ok(KeyCode::KEY_9),
        "ENTER" | "RETURN" => Ok(KeyCode::KEY_ENTER),
        "ESC" | "ESCAPE" => Ok(KeyCode::KEY_ESC),
        "SPACE" => Ok(KeyCode::KEY_SPACE),
        other => anyhow::bail!("unsupported trigger key: {other}"),
    }
}

fn match_hotkey_event(
    mode: &str,
    pressed: bool,
    language: Option<String>,
) -> Option<HotkeyCommand> {
    match mode {
        "push_to_talk" | "auto" => {
            if pressed {
                Some(HotkeyCommand::Start(language))
            } else {
                Some(HotkeyCommand::Stop)
            }
        }
        _ => {
            if pressed {
                Some(HotkeyCommand::Toggle)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_hotkey_detector_toggles_on_combo_press_and_release() {
        let hotkey = LinuxHotkey {
            trigger: KeyCode::KEY_S,
            modifiers: vec![KeyCode::KEY_LEFTALT, KeyCode::KEY_LEFTMETA],
        };
        let mut detector = LinuxHotkeyDetector::new(vec![(HotkeySlot::Primary, hotkey)]);

        assert!(detector.handle_key(KeyCode::KEY_LEFTMETA, 1).is_none());
        assert!(detector.handle_key(KeyCode::KEY_LEFTALT, 1).is_none());

        let pressed = detector.handle_key(KeyCode::KEY_S, 1).unwrap();
        assert_eq!(pressed.slot, HotkeySlot::Primary);
        assert!(pressed.pressed);

        let released = detector.handle_key(KeyCode::KEY_S, 0).unwrap();
        assert_eq!(released.slot, HotkeySlot::Primary);
        assert!(!released.pressed);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn submit_hotkey_emits_submit_on_press_only() {
        let hotkey = LinuxHotkey {
            trigger: KeyCode::KEY_E,
            modifiers: vec![KeyCode::KEY_LEFTALT, KeyCode::KEY_LEFTMETA],
        };
        let mut detector = LinuxHotkeyDetector::new(vec![(HotkeySlot::Submit, hotkey)]);

        assert!(detector.handle_key(KeyCode::KEY_LEFTMETA, 1).is_none());
        assert!(detector.handle_key(KeyCode::KEY_LEFTALT, 1).is_none());

        let pressed = detector.handle_key(KeyCode::KEY_E, 1).unwrap();
        assert_eq!(pressed.slot, HotkeySlot::Submit);
        assert!(pressed.pressed);

        let released = detector.handle_key(KeyCode::KEY_E, 0).unwrap();
        assert_eq!(released.slot, HotkeySlot::Submit);
        assert!(!released.pressed);
    }
}
