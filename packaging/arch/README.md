# Arch Packaging

This directory contains Arch Linux packaging for Hermes.

Packages:

- `hermes-cli`: core `hermes` binary (daemon + CLI controls)
- `hermes-desktop`: Tauri desktop app (`hermes-desktop`)
- `hermes-cli-bin`: prebuilt Linux CLI binary from GitHub Releases
- `hermes-desktop-bin`: prebuilt Linux desktop binary from GitHub Releases

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

### Binary packages

```bash
cd packaging/arch/hermes-cli-bin
makepkg -si

cd packaging/arch/hermes-desktop-bin
makepkg -si
```

## Notes

- `hermes-desktop` builds from source and runs `bun install` during `prepare()`.
- On clean build hosts, internet access is needed in `prepare()` for JS dependencies.
- Prefer the `-bin` packages for end-user installs. The source packages remain available for users who want local builds.
- Update `pkgver` in all PKGBUILDs when tagging a new release.
