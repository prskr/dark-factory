#!/usr/bin/env bash
# Sets the sha256 of the two OpenCode archives in MODULE.bazel from the SHA256SUMS of the release
# that OPENCODE_VERSION names. Renovate runs it after bumping OPENCODE_VERSION (postUpgradeTasks in
# renovate.json); run it by hand after changing the version yourself.
set -euo pipefail
cd "$(dirname "$0")/../.."

version="$(sed -n 's/^OPENCODE_VERSION = "\([^"]*\)".*/\1/p' MODULE.bazel)"
[[ -n "$version" ]] || { echo "no OPENCODE_VERSION in MODULE.bazel" >&2; exit 1; }
sums="$(curl -fsSL --retry 3 "https://github.com/anomalyco/opencode/releases/download/v${version}/SHA256SUMS")"

set_sha() { # <http_archive name> <release asset>
  local new old
  new="$(awk -v a="$2" '$2 == a { print $1 }' <<<"$sums")"
  [[ "$new" =~ ^[0-9a-f]{64}$ ]] || { echo "no checksum for $2 in SHA256SUMS of v$version" >&2; exit 1; }
  # the first sha256 after `name = "<name>",`
  old="$(awk -v n="\"$1\"," '$1 == "name" && $3 == n { f = 1 } f && $1 == "sha256" { gsub(/[",]/, "", $3); print $3; exit }' MODULE.bazel)"
  [[ "$old" =~ ^[0-9a-f]{64}$ ]] || { echo "no sha256 for $1 in MODULE.bazel" >&2; exit 1; }
  sed -i "s/$old/$new/" MODULE.bazel
}

set_sha opencode_linux_amd64_musl opencode-linux-x64-musl.tar.gz
set_sha opencode_linux_arm64_musl opencode-linux-arm64-musl.tar.gz
