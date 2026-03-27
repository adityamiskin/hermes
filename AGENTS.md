# AGENTS Notes

This file stores durable repo-specific facts that are easy to miss but important when changing release, packaging, or credential-storage behavior.

## Release Invariants

- The release version must stay in sync across:
  - `package.json`
  - `Cargo.toml`
  - `src-tauri/Cargo.toml`
  - `packaging/arch/hermes-cli/PKGBUILD`
  - `packaging/arch/hermes-desktop/PKGBUILD`
  - `packaging/arch/hermes-cli/.SRCINFO`
  - `packaging/arch/hermes-desktop/.SRCINFO`
- Git tags are expected to be `v<version>`.
- Arch packaging is tag-based, not commit-based. `PKGBUILD` should source `#tag=v${pkgver}` and `.SRCINFO` should resolve to the concrete tag, e.g. `v0.1.2`.
- `scripts/check-release-metadata.sh` enforces the sync rules above and is wired into both CI and release workflows.

## CI/CD

- `.github/workflows/ci.yml` runs:
  - release metadata validation
  - frontend build
  - Rust format check
  - Rust check for the core crate
  - Rust check for the Tauri crate
- `.github/workflows/release.yml` runs on `v*` tags and publishes bundle artifacts to GitHub Releases.
- The release workflow must check out full history (`fetch-depth: 0`) so tag-based validation works correctly.

## Linux Credential Storage

- The app explicitly uses `secret-service` on Linux in `src/credentials.rs`.
- A local vendored copy of `keyring` lives under `vendor/keyring`.
- That vendored patch intentionally disables `keyring`'s SQLite/db-keystore backend because it drags in `turso -> aegis`, which broke Linux desktop builds through Clang/AVX512 compilation.
- Do not remove the `[patch.crates-io] keyring = { path = ... }` entries from:
  - `Cargo.toml`
  - `src-tauri/Cargo.toml`
  unless the upstream dependency chain is verified to build cleanly again on Linux.

## Arch Packaging

- AUR packages:
  - `hermes-cli`
  - `hermes-desktop`
- Desktop packaging depends on `bun install` during `prepare()`, so clean Arch builds need network access there unless the packaging strategy changes.
- Local Arch build artifacts and extracted package trees are intentionally ignored in `.gitignore`.

## Practical Release Flow

1. Update versions in manifests and Arch metadata.
2. Run `bash scripts/check-release-metadata.sh`.
3. Run local verification used by CI:
   - `bun run build`
   - `cargo check --all-targets`
   - `cargo check --manifest-path src-tauri/Cargo.toml --all-targets`
4. Push `main`.
5. Create and push the matching tag `v<version>`.
6. If AUR metadata changed, push updated package repos after the GitHub tag exists.
