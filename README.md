# Hermes

Hermes is the Rust desktop rewrite of `hyprwhspr`.

It has two layers:

- `hermes` core: audio capture, hotkeys, transcription backends, overlay, and text injection.
- `hermes-desktop`: a Tauri desktop shell with a React + shadcn settings UI and tray integration.

## About

Hermes is built for fast dictation with a clean desktop UX:

- Global hotkey dictation toggle
- Live mic overlay pill
- Local and remote transcription backends
- Secure API key storage in OS keychain
- Tray app behavior with launch-at-login support

## Features

- Cross-platform desktop app (Linux, macOS, Windows) via Tauri
- Multiple STT backends:
- `rest-api` (Groq/OpenAI/ElevenLabs-compatible)
- `realtime-ws` (realtime websocket transcription path)
- `whisper-rs` (local)
- `faster-whisper` (local runner)
- Configurable hotkeys and recording modes (`toggle`, `push_to_talk`, `long_form`)
- Audio device selection and clipboard/paste behavior controls
- Provider credential management from the app UI

## Project Structure

- [src](./src): Rust core engine (`hermes` crate)
- [src-tauri](./src-tauri): desktop host (`hermes-desktop` crate)
- [src-web](./src-web): frontend UI (React + shadcn)
- [support](./support): helper scripts (for local backend integrations)

## Quick Start

### Prerequisites

- Rust toolchain (stable)
- Bun
- Linux only: Tauri system dependencies (webkit2gtk, libsoup3, etc.)

### Run Dev App

```bash
bun install
bun run tauri dev
```

### Build Release App

```bash
bun run tauri build
```

Built bundles are generated under:

- `src-tauri/target/release/bundle/`

## Core CLI (Optional)

The Rust core CLI is still available for direct testing:

```bash
cargo run -- daemon
cargo run -- record toggle
```

## Configuration and Credentials

Hermes uses its own app directories:

- Linux config: `~/.config/hermes/config.json`
- Linux data: `~/.local/share/hermes/`

Credentials are stored in OS keychain services:

- Linux: Secret Service
- macOS: Keychain
- Windows: Credential Manager

## Auto Start

Launch-at-login is supported in-app through the desktop UI:

- macOS: LaunchAgent
- Linux: desktop autostart entry managed by plugin
- Windows: autostart entry managed by plugin

The app starts the Hermes daemon on desktop app startup.

## CI/CD

This repo includes GitHub Actions:

- `CI` on push/PR:
- frontend install + build
- Rust format check
- Rust checks for core and Tauri crates
- `Release` on tags (`v*`):
- builds Linux, macOS, and Windows bundles
- uploads artifacts to GitHub Release

Create a release by pushing a version tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Arch Linux Packages

Arch packaging files are included in [packaging/arch](./packaging/arch):

- `hermes-cli`
- `hermes-desktop`

Build locally with:

```bash
cd packaging/arch/hermes-cli && makepkg -si
cd packaging/arch/hermes-desktop && makepkg -si
```

## Notes

- Linux AppImage generation may require additional host tooling.
- macOS notarization and Windows code-signing are not configured in this repo yet.
