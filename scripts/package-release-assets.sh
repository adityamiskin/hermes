#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <output-dir>" >&2
  exit 1
fi

out_dir="$1"
arch="x86_64"
cli_root="hermes-cli-linux-${arch}"
desktop_root="hermes-desktop-linux-${arch}"

rm -rf "$out_dir"
mkdir -p "$out_dir/$cli_root/usr/bin" \
  "$out_dir/$cli_root/usr/share/doc/hermes-cli" \
  "$out_dir/$desktop_root/usr/bin" \
  "$out_dir/$desktop_root/usr/share/applications" \
  "$out_dir/$desktop_root/usr/share/icons/hicolor/128x128/apps" \
  "$out_dir/$desktop_root/usr/share/doc/hermes-desktop"

install -Dm755 target/release/hermes \
  "$out_dir/$cli_root/usr/bin/hermes"
install -Dm644 README.md \
  "$out_dir/$cli_root/usr/share/doc/hermes-cli/README.md"

install -Dm755 src-tauri/target/release/hermes-desktop \
  "$out_dir/$desktop_root/usr/bin/hermes-desktop"
install -Dm644 packaging/arch/hermes-desktop/hermes-desktop.desktop \
  "$out_dir/$desktop_root/usr/share/applications/hermes-desktop.desktop"
install -Dm644 src-tauri/icons/128x128.png \
  "$out_dir/$desktop_root/usr/share/icons/hicolor/128x128/apps/hermes.png"
install -Dm644 README.md \
  "$out_dir/$desktop_root/usr/share/doc/hermes-desktop/README.md"

tarball() {
  local root="$1"
  tar --sort=name \
    --mtime='UTC 1970-01-01' \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "$out_dir" \
    -cf - "$root" | gzip -n > "$out_dir/${root}.tar.gz"
}

tarball "$cli_root"
tarball "$desktop_root"
