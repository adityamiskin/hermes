use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const UNIT_NAME: &str = "hermes.service";

#[cfg(target_os = "linux")]
pub fn install(activate: bool) -> Result<()> {
    let unit_path = unit_file_path()?;
    let unit_dir = unit_path
        .parent()
        .context("failed to resolve systemd user unit directory")?;
    fs::create_dir_all(unit_dir)
        .with_context(|| format!("failed to create {}", unit_dir.display()))?;

    let executable = std::env::current_exe().context("failed to resolve hermes executable path")?;
    let unit = format!(
        "[Unit]\nDescription=Hermes Speech-to-Text Daemon\nAfter=graphical-session.target\nWants=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} daemon\nRestart=on-failure\nRestartSec=1\nEnvironment=RUST_LOG=info\n\n[Install]\nWantedBy=default.target\n",
        executable.display()
    );

    fs::write(&unit_path, unit)
        .with_context(|| format!("failed to write {}", unit_path.display()))?;

    run_systemctl(&["--user", "daemon-reload"])?;
    if activate {
        run_systemctl(&["--user", "enable", "--now", UNIT_NAME])?;
    }

    println!("installed {}", unit_path.display());
    if activate {
        println!("service enabled and started");
    } else {
        println!("next: hermes service enable && hermes service start");
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn install(_activate: bool) -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<()> {
    let unit_path = unit_file_path()?;
    let _ = run_systemctl(&["--user", "disable", "--now", UNIT_NAME]);
    if unit_path.exists() {
        fs::remove_file(&unit_path)
            .with_context(|| format!("failed to remove {}", unit_path.display()))?;
    }
    run_systemctl(&["--user", "daemon-reload"])?;
    println!("uninstalled {}", unit_path.display());
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn uninstall() -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
pub fn start() -> Result<()> {
    run_systemctl(&["--user", "start", UNIT_NAME])?;
    println!("service started");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn start() -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
pub fn stop() -> Result<()> {
    run_systemctl(&["--user", "stop", UNIT_NAME])?;
    println!("service stopped");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn stop() -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
pub fn restart() -> Result<()> {
    run_systemctl(&["--user", "restart", UNIT_NAME])?;
    println!("service restarted");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn restart() -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
pub fn enable() -> Result<()> {
    run_systemctl(&["--user", "enable", UNIT_NAME])?;
    println!("service enabled");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn enable() -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
pub fn disable() -> Result<()> {
    run_systemctl(&["--user", "disable", UNIT_NAME])?;
    println!("service disabled");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn disable() -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
pub fn status() -> Result<()> {
    let enabled = run_systemctl_capture(&["--user", "is-enabled", UNIT_NAME])?;
    let active = run_systemctl_capture(&["--user", "is-active", UNIT_NAME])?;

    println!("unit: {}", UNIT_NAME);
    println!("enabled: {}", enabled.trim());
    println!("active: {}", active.trim());
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn status() -> Result<()> {
    bail!("hermes service commands are currently supported on Linux (systemd user units) only")
}

#[cfg(target_os = "linux")]
fn unit_file_path() -> Result<PathBuf> {
    let home = directories::BaseDirs::new()
        .context("failed to resolve home directory")?
        .home_dir()
        .to_path_buf();
    Ok(home.join(".config/systemd/user").join(UNIT_NAME))
}

#[cfg(target_os = "linux")]
fn run_systemctl(args: &[&str]) -> Result<()> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("failed to run systemctl {}", args.join(" ")))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    bail!("systemctl {} failed:\n{}{}", args.join(" "), stdout, stderr)
}

#[cfg(target_os = "linux")]
fn run_systemctl_capture(args: &[&str]) -> Result<String> {
    let output = Command::new("systemctl")
        .args(args)
        .output()
        .with_context(|| format!("failed to run systemctl {}", args.join(" ")))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    bail!("systemctl {} failed:\n{}{}", args.join(" "), stdout, stderr)
}
