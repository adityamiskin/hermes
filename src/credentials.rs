use crate::paths::AppPaths;
use anyhow::{Context, Result, anyhow};
use keyring::use_named_store;
use keyring_core::{Entry, Error};

const SERVICE_NAME: &str = "hermes";

#[cfg(target_os = "macos")]
const STORE_NAME: &str = "keychain";
#[cfg(target_os = "macos")]
const STORE_LABEL: &str = "macOS Keychain";

#[cfg(target_os = "windows")]
const STORE_NAME: &str = "windows";
#[cfg(target_os = "windows")]
const STORE_LABEL: &str = "Windows Credential Manager";

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
const STORE_NAME: &str = "secret-service";
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
const STORE_LABEL: &str = "Secret Service keyring";

#[cfg(target_os = "android")]
const STORE_NAME: &str = "android";
#[cfg(target_os = "android")]
const STORE_LABEL: &str = "Android credential store";

fn configure_store() -> Result<()> {
    #[cfg(any(
        target_os = "android",
        target_os = "freebsd",
        target_os = "linux",
        target_os = "macos",
        target_os = "openbsd",
        target_os = "windows",
    ))]
    {
        use_named_store(STORE_NAME).with_context(|| format!("failed to open {STORE_LABEL}"))?;
        Ok(())
    }

    #[cfg(not(any(
        target_os = "android",
        target_os = "freebsd",
        target_os = "linux",
        target_os = "macos",
        target_os = "openbsd",
        target_os = "windows",
    )))]
    {
        Err(anyhow!(
            "secure credential storage is not supported on {}",
            std::env::consts::OS
        ))
    }
}

fn entry_for(provider: &str) -> Result<Entry> {
    configure_store()?;
    Entry::new(SERVICE_NAME, provider)
        .with_context(|| format!("failed to create credential entry for provider '{provider}'"))
}

pub fn get_credential(_paths: &AppPaths, provider: &str) -> Result<Option<String>> {
    let entry = entry_for(provider)?;
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(err) => Err(anyhow!(
            "failed to read credential for provider '{provider}': {err}"
        )),
    }
}

pub fn save_credential(_paths: &AppPaths, provider: &str, key: &str) -> Result<()> {
    let entry = entry_for(provider)?;
    entry
        .set_password(key)
        .with_context(|| format!("failed to save credential for provider '{provider}'"))?;
    Ok(())
}
