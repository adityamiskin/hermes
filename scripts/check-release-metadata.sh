#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

trim() {
  sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//'
}

extract_toml_version() {
  local file="$1"
  awk -F'"' '/^version = "/ { print $2; exit }' "$file"
}

extract_pkgbuild_version() {
  local file="$1"
  awk -F= '/^pkgver=/ { print $2; exit }' "$file" | trim
}

extract_srcinfo_version() {
  local file="$1"
  awk -F= '/pkgver = / { print $2; exit }' "$file" | trim
}

extract_pkgbuild_tag() {
  local file="$1"
  sed -n 's/.*#tag=v${pkgver}.*/v${pkgver}/p' "$file" | head -n1
}

extract_srcinfo_tag() {
  local file="$1"
  sed -n 's/.*#tag=\(v[^[:space:]]*\).*/\1/p' "$file" | head -n1
}

extract_pkgbuild_release_tag_url() {
  local file="$1"
  sed -n 's#.*releases/download/\(v\${pkgver}\)/.*#\1#p' "$file" | head -n1
}

extract_srcinfo_release_tag_url() {
  local file="$1"
  sed -n 's#.*releases/download/\(v[^/[:space:]]*\)/.*#\1#p' "$file" | head -n1
}

package_json_version="$(jq -r '.version' package.json)"
core_cargo_version="$(extract_toml_version Cargo.toml)"
desktop_cargo_version="$(extract_toml_version src-tauri/Cargo.toml)"
cli_pkgbuild_version="$(extract_pkgbuild_version packaging/arch/hermes-cli/PKGBUILD)"
desktop_pkgbuild_version="$(extract_pkgbuild_version packaging/arch/hermes-desktop/PKGBUILD)"
cli_bin_pkgbuild_version="$(extract_pkgbuild_version packaging/arch/hermes-cli-bin/PKGBUILD)"
desktop_bin_pkgbuild_version="$(extract_pkgbuild_version packaging/arch/hermes-desktop-bin/PKGBUILD)"
cli_srcinfo_version="$(extract_srcinfo_version packaging/arch/hermes-cli/.SRCINFO)"
desktop_srcinfo_version="$(extract_srcinfo_version packaging/arch/hermes-desktop/.SRCINFO)"
cli_bin_srcinfo_version="$(extract_srcinfo_version packaging/arch/hermes-cli-bin/.SRCINFO)"
desktop_bin_srcinfo_version="$(extract_srcinfo_version packaging/arch/hermes-desktop-bin/.SRCINFO)"

versions=(
  "$package_json_version"
  "$core_cargo_version"
  "$desktop_cargo_version"
  "$cli_pkgbuild_version"
  "$desktop_pkgbuild_version"
  "$cli_bin_pkgbuild_version"
  "$desktop_bin_pkgbuild_version"
  "$cli_srcinfo_version"
  "$desktop_srcinfo_version"
  "$cli_bin_srcinfo_version"
  "$desktop_bin_srcinfo_version"
)

reference_version="${versions[0]}"
for version in "${versions[@]}"; do
  if [[ "$version" != "$reference_version" ]]; then
    echo "Version mismatch detected:" >&2
    printf '  package.json: %s\n' "$package_json_version" >&2
    printf '  Cargo.toml: %s\n' "$core_cargo_version" >&2
    printf '  src-tauri/Cargo.toml: %s\n' "$desktop_cargo_version" >&2
    printf '  hermes-cli PKGBUILD: %s\n' "$cli_pkgbuild_version" >&2
    printf '  hermes-desktop PKGBUILD: %s\n' "$desktop_pkgbuild_version" >&2
    printf '  hermes-cli-bin PKGBUILD: %s\n' "$cli_bin_pkgbuild_version" >&2
    printf '  hermes-desktop-bin PKGBUILD: %s\n' "$desktop_bin_pkgbuild_version" >&2
    printf '  hermes-cli .SRCINFO: %s\n' "$cli_srcinfo_version" >&2
    printf '  hermes-desktop .SRCINFO: %s\n' "$desktop_srcinfo_version" >&2
    printf '  hermes-cli-bin .SRCINFO: %s\n' "$cli_bin_srcinfo_version" >&2
    printf '  hermes-desktop-bin .SRCINFO: %s\n' "$desktop_bin_srcinfo_version" >&2
    exit 1
  fi
done

expected_tag="v${reference_version}"

if [[ "$(extract_pkgbuild_tag packaging/arch/hermes-cli/PKGBUILD)" != "v\${pkgver}" ]]; then
  echo "hermes-cli PKGBUILD must source git tag v\${pkgver}." >&2
  exit 1
fi

if [[ "$(extract_pkgbuild_tag packaging/arch/hermes-desktop/PKGBUILD)" != "v\${pkgver}" ]]; then
  echo "hermes-desktop PKGBUILD must source git tag v\${pkgver}." >&2
  exit 1
fi

if [[ "$(extract_srcinfo_tag packaging/arch/hermes-cli/.SRCINFO)" != "$expected_tag" ]]; then
  echo "hermes-cli .SRCINFO tag does not match ${expected_tag}." >&2
  exit 1
fi

if [[ "$(extract_srcinfo_tag packaging/arch/hermes-desktop/.SRCINFO)" != "$expected_tag" ]]; then
  echo "hermes-desktop .SRCINFO tag does not match ${expected_tag}." >&2
  exit 1
fi

if [[ "$(extract_pkgbuild_release_tag_url packaging/arch/hermes-cli-bin/PKGBUILD)" != "v\${pkgver}" ]]; then
  echo "hermes-cli-bin PKGBUILD must source release tag v\${pkgver}." >&2
  exit 1
fi

if [[ "$(extract_pkgbuild_release_tag_url packaging/arch/hermes-desktop-bin/PKGBUILD)" != "v\${pkgver}" ]]; then
  echo "hermes-desktop-bin PKGBUILD must source release tag v\${pkgver}." >&2
  exit 1
fi

if [[ "$(extract_srcinfo_release_tag_url packaging/arch/hermes-cli-bin/.SRCINFO)" != "$expected_tag" ]]; then
  echo "hermes-cli-bin .SRCINFO release tag does not match ${expected_tag}." >&2
  exit 1
fi

if [[ "$(extract_srcinfo_release_tag_url packaging/arch/hermes-desktop-bin/.SRCINFO)" != "$expected_tag" ]]; then
  echo "hermes-desktop-bin .SRCINFO release tag does not match ${expected_tag}." >&2
  exit 1
fi

if [[ -n "${GITHUB_REF_NAME:-}" && "${GITHUB_REF_TYPE:-}" == "tag" && "${GITHUB_REF_NAME}" != "${expected_tag}" ]]; then
  echo "GitHub tag ${GITHUB_REF_NAME} does not match expected tag ${expected_tag}." >&2
  exit 1
fi

echo "Release metadata is consistent for ${expected_tag}."
