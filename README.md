# Hermes

Hermes is the Rust desktop rewrite of `hyprwhspr`.

It now has two layers:

- `hermes` core: audio capture, hotkeys, dictation backends, overlay, injection
- `hermes-desktop`: a Tauri desktop shell with a React settings UI, tray menu, and packaged app output

## Desktop App

The Tauri app lives in [src-tauri](./src-tauri) and the frontend lives in [src-web](./src-web).

### Development

```bash
bun install
bun run tauri dev
```

### Build

```bash
bun run tauri build --debug --no-bundle
```

### Linux Package

```bash
bun run tauri build --debug --bundles deb
```

Current verified Linux package output:

- `src-tauri/target/debug/bundle/deb/Hermes_0.1.0_amd64.deb`

## Core CLI

The Rust core CLI still exists and is useful for testing:

```bash
cargo run -- daemon
cargo run -- record toggle
```

## Config

Hermes uses its own app directories.

- config: `~/.config/hermes/config.json`
- data: `~/.local/share/hermes/`

Credentials are stored in the OS keychain:

- Linux: Secret Service
- macOS: Keychain
- Windows: Credential Manager

## Current Scope

- Tauri desktop shell with a real Hermes settings UI
- in-process daemon lifecycle managed by the desktop app
- tray menu for toggle/open/quit
- secure provider key save flow
- local Whisper, faster-whisper, REST STT, and realtime backend support from the Rust core
- Linux `.deb` packaging verified locally

## Known Gaps

- updater feed is not wired yet
- AppImage bundling currently needs `linuxdeploy` on the build host
- macOS and Windows artifacts are scaffolded through Tauri but were not built on this Linux machine
- the desktop UI is first-pass and does not expose every config field yet
