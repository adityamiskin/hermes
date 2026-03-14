# Arch Packaging

This directory contains Arch Linux packaging for Hermes.

Packages:

- `hermes-cli`: core `hermes` binary (daemon + CLI controls)
- `hermes-desktop`: Tauri desktop app (`hermes-desktop`)

## Build Locally

### CLI package

```bash
cd packaging/arch/hermes-cli
makepkg -si
```

### Desktop package

```bash
cd packaging/arch/hermes-desktop
makepkg -si
```

## Notes

- `hermes-desktop` builds from source and runs `bun install` during `prepare()`.
- On clean build hosts, internet access is needed in `prepare()` for JS dependencies.
- Update `pkgver` in both PKGBUILDs when tagging a new release.
