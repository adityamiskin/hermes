use crate::config::AppConfig;
use anyhow::{Context, Result};
use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
#[cfg(target_os = "linux")]
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, uinput::VirtualDevice};
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const CLIPBOARD_SYNC_DELAY_MS: u64 = 15;
#[cfg(target_os = "linux")]
const UINPUT_INTER_EVENT_DELAY_MS: u64 = 1;

pub struct TextInjector {
    clipboard: ClipboardBackend,
    injector: InputInjector,
}

enum InputInjector {
    #[cfg(target_os = "linux")]
    Uinput(LinuxUinputInjector),
    Enigo(Enigo),
}

#[cfg(target_os = "linux")]
struct LinuxUinputInjector {
    device: VirtualDevice,
}

impl TextInjector {
    pub fn new() -> Result<Self> {
        let clipboard = ClipboardBackend::new()?;

        #[cfg(target_os = "linux")]
        let injector = if let Ok(injector) = LinuxUinputInjector::new() {
            InputInjector::Uinput(injector)
        } else {
            eprintln!("[injector] /dev/uinput unavailable; falling back to enigo");
            InputInjector::Enigo(
                Enigo::new(&Settings::default())
                    .context("failed to initialize fallback input injector")?,
            )
        };

        #[cfg(not(target_os = "linux"))]
        let injector = InputInjector::Enigo(
            Enigo::new(&Settings::default()).context("failed to initialize input injector")?,
        );

        Ok(Self {
            clipboard,
            injector,
        })
    }

    pub fn inject_text(&mut self, config: &AppConfig, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }

        self.clipboard
            .set_text(text)
            .context("failed to copy transcript into clipboard")?;
        thread::sleep(Duration::from_millis(CLIPBOARD_SYNC_DELAY_MS));
        send_paste(&mut self.injector, &config.paste_mode)?;
        if config.auto_submit {
            #[cfg(target_os = "linux")]
            thread::sleep(Duration::from_millis(UINPUT_INTER_EVENT_DELAY_MS));
            send_enter(&mut self.injector)?;
        }

        if config.clipboard_behavior {
            let delay = config.clipboard_clear_delay.max(0.0);
            thread::sleep(Duration::from_secs_f32(delay));
            let _ = self.clipboard.set_text("");
        }

        Ok(())
    }
}

enum ClipboardBackend {
    Arboard(Clipboard),
    #[cfg(target_os = "linux")]
    WaylandCopy,
}

impl ClipboardBackend {
    fn new() -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() && command_exists("wl-copy") {
                return Ok(Self::WaylandCopy);
            }
        }

        Ok(Self::Arboard(
            Clipboard::new().context("failed to initialize clipboard")?,
        ))
    }

    fn set_text(&mut self, text: &str) -> Result<()> {
        match self {
            Self::Arboard(clipboard) => clipboard
                .set_text(text.to_string())
                .context("failed to store clipboard text via arboard"),
            #[cfg(target_os = "linux")]
            Self::WaylandCopy => {
                if let Err(error) = set_wayland_clipboard(text) {
                    eprintln!("[clipboard] wl-copy failed, falling back to arboard: {error}");
                    let mut fallback =
                        Clipboard::new().context("failed to initialize fallback clipboard")?;
                    fallback
                        .set_text(text.to_string())
                        .context("failed to store clipboard text via fallback clipboard")
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
use std::process::Child;
#[cfg(target_os = "linux")]
use std::sync::{Mutex, OnceLock};
#[cfg(target_os = "linux")]
static WL_COPY_CHILD: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

#[cfg(target_os = "linux")]
fn set_wayland_clipboard(text: &str) -> Result<()> {
    let child_slot = WL_COPY_CHILD.get_or_init(|| Mutex::new(None));
    if let Ok(mut slot) = child_slot.lock() {
        if let Some(mut old_child) = slot.take() {
            let _ = old_child.kill();
            let _ = old_child.wait();
        }
    }

    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn wl-copy")?;

    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .context("failed to write clipboard contents to wl-copy")?;
    }
    drop(child.stdin.take());

    thread::sleep(Duration::from_millis(8));
    if let Some(status) = child.try_wait().context("failed to poll wl-copy")? {
        if !status.success() {
            anyhow::bail!("wl-copy exited early with status {status}");
        }
    }

    let mut slot = child_slot
        .lock()
        .map_err(|_| anyhow::anyhow!("clipboard child lock poisoned"))?;
    *slot = Some(child);
    Ok(())
}

#[cfg(target_os = "linux")]
fn clear_wayland_clipboard_process() {
    if let Some(slot) = WL_COPY_CHILD.get() {
        if let Ok(mut slot) = slot.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for ClipboardBackend {
    fn drop(&mut self) {
        if matches!(self, ClipboardBackend::WaylandCopy) {
            clear_wayland_clipboard_process();
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl Drop for ClipboardBackend {
    fn drop(&mut self) {}
}

#[cfg(target_os = "linux")]
fn _debug_wlcopy_available() -> Result<()> {
    if !command_exists("wl-copy") {
        anyhow::bail!("wl-copy not found");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn _debug_wlpaste_available() -> Result<()> {
    if !command_exists("wl-paste") {
        anyhow::bail!("wl-paste not found");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn _debug_wayland_env_available() -> Result<()> {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        anyhow::bail!("WAYLAND_DISPLAY not set");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn _wayland_clipboard_preconditions() -> Result<()> {
    _debug_wayland_env_available()?;
    _debug_wlcopy_available()?;
    _debug_wlpaste_available()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn _read_wayland_clipboard() -> Result<String> {
    let output = Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .context("failed to run wl-paste")?;
    if !output.status.success() {
        anyhow::bail!("wl-paste exited with status {}", output.status);
    }
    String::from_utf8(output.stdout).context("wl-paste output was not valid UTF-8")
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {command} >/dev/null 2>&1"))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
impl LinuxUinputInjector {
    fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in [
            KeyCode::KEY_LEFTMETA,
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_V,
            KeyCode::KEY_ENTER,
        ] {
            keys.insert(key);
        }

        let device = VirtualDevice::builder()
            .context("failed to open /dev/uinput")?
            .name("Hermes Virtual Keyboard")
            .with_keys(&keys)
            .context("failed to configure uinput keyboard keys")?
            .build()
            .context("failed to create uinput virtual keyboard")?;

        // Give the compositor a brief window to register the virtual keyboard once.
        thread::sleep(Duration::from_millis(150));

        Ok(Self { device })
    }

    fn paste(&mut self, mode: &str) -> Result<()> {
        match mode {
            "super" => self.key_combo(&[KeyCode::KEY_LEFTMETA], KeyCode::KEY_V),
            "ctrl" => self.key_combo(&[KeyCode::KEY_LEFTCTRL], KeyCode::KEY_V),
            "alt" => self.key_combo(&[KeyCode::KEY_LEFTALT], KeyCode::KEY_V),
            _ => self.key_combo(
                &[KeyCode::KEY_LEFTCTRL, KeyCode::KEY_LEFTSHIFT],
                KeyCode::KEY_V,
            ),
        }
    }

    fn enter(&mut self) -> Result<()> {
        self.key_click(KeyCode::KEY_ENTER)
    }

    fn key_combo(&mut self, modifiers: &[KeyCode], key: KeyCode) -> Result<()> {
        for modifier in modifiers {
            self.device
                .emit(&[key_event(*modifier, 1)])
                .context("failed to emit uinput modifier press")?;
            thread::sleep(Duration::from_millis(UINPUT_INTER_EVENT_DELAY_MS));
        }
        self.device
            .emit(&[key_event(key, 1)])
            .context("failed to emit uinput key press")?;
        thread::sleep(Duration::from_millis(UINPUT_INTER_EVENT_DELAY_MS));
        self.device
            .emit(&[key_event(key, 0)])
            .context("failed to emit uinput key release")?;
        thread::sleep(Duration::from_millis(UINPUT_INTER_EVENT_DELAY_MS));
        for modifier in modifiers.iter().rev() {
            self.device
                .emit(&[key_event(*modifier, 0)])
                .context("failed to emit uinput modifier release")?;
            thread::sleep(Duration::from_millis(UINPUT_INTER_EVENT_DELAY_MS));
        }
        Ok(())
    }

    fn key_click(&mut self, key: KeyCode) -> Result<()> {
        self.device
            .emit(&[key_event(key, 1), key_event(key, 0)])
            .context("failed to emit uinput key click")
    }
}

fn send_paste(injector: &mut InputInjector, mode: &str) -> Result<()> {
    match injector {
        #[cfg(target_os = "linux")]
        InputInjector::Uinput(injector) => injector.paste(mode),
        InputInjector::Enigo(enigo) => send_paste_enigo(enigo, mode),
    }
}

fn send_enter(injector: &mut InputInjector) -> Result<()> {
    match injector {
        #[cfg(target_os = "linux")]
        InputInjector::Uinput(injector) => injector.enter(),
        InputInjector::Enigo(enigo) => send_enter_enigo(enigo),
    }
}

fn send_paste_enigo(enigo: &mut Enigo, mode: &str) -> Result<()> {
    match mode {
        "super" => key_combo(enigo, &[Key::Meta], Key::Unicode('v')),
        "ctrl" => key_combo(enigo, &[Key::Control], Key::Unicode('v')),
        "alt" => key_combo(enigo, &[Key::Alt], Key::Unicode('v')),
        _ => key_combo(enigo, &[Key::Control, Key::Shift], Key::Unicode('v')),
    }
}

fn send_enter_enigo(enigo: &mut Enigo) -> Result<()> {
    enigo
        .key(Key::Return, Direction::Click)
        .context("failed to send Enter")
}

#[cfg(target_os = "linux")]
fn key_event(code: KeyCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code.code(), value)
}

fn key_combo(enigo: &mut Enigo, modifiers: &[Key], key: Key) -> Result<()> {
    for modifier in modifiers {
        enigo
            .key(*modifier, Direction::Press)
            .context("failed to press modifier")?;
    }
    enigo
        .key(key, Direction::Click)
        .context("failed to click key")?;
    for modifier in modifiers.iter().rev() {
        enigo
            .key(*modifier, Direction::Release)
            .context("failed to release modifier")?;
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn wayland_clipboard_round_trip_if_available() -> Result<()> {
        if _wayland_clipboard_preconditions().is_err() {
            return Ok(());
        }

        let expected = "hermes clipboard smoke test";
        set_wayland_clipboard(expected)?;
        let actual = _read_wayland_clipboard()?;
        assert_eq!(actual, expected);
        Ok(())
    }
}
