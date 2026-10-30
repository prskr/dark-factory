#!/usr/bin/env bash
# Checks the Claude harness image (openspec change harness-images, design D9) from its OCI layout.
# Usage: test.sh <jq> <oci layout> <package.json> [--handshake]
#
# Without --handshake: structure checks of both architectures (no container runtime needed; the
# `structure_test` target). With --handshake: additionally an ACP initialize handshake of the host
# architecture in a glibc and a musl project image (`bazel run :handshake`); CONTAINER_CLI selects
# the container CLI (default: docker).
set -euo pipefail

JQ_BIN="$(realpath "$1")"
LAYOUT="$2"
PACKAGE_JSON="$3"
HANDSHAKE="${4:-}"
CLI="${CONTAINER_CLI:-docker}"
WORK="$(mktemp -d)"
HS=""
cleanup() {
  [[ -z "$HS" ]] || $CLI rm -f "$HS" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

jq() { "$JQ_BIN" "$@"; }

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# extract <arch> <dir>: the image content, as the control plane mounts it at /opt/harness/claude-code
extract() {
  local manifest
  manifest="$(jq -r --arg arch "$1" \
    '.manifests[] | select(.platform.architecture == $arch) | .digest | sub("^sha256:"; "")' "$LAYOUT/index.json")"
  [[ -n "$manifest" ]] || fail "no $1 manifest in the index"
  mkdir -p "$2"
  jq -r '.layers[].digest | sub("^sha256:"; "")' "$LAYOUT/blobs/sha256/$manifest" | while read -r layer; do
    tar -xf "$LAYOUT/blobs/sha256/$layer" -C "$2"
  done
}

check_structure() { # <arch> <root>
  local arch="$1" root="$2" file_arch npm_arch loader
  case "$arch" in
  amd64) file_arch="x86-64" npm_arch="x64" loader="ld-musl-x86_64.so.1" ;;
  arm64) file_arch="ARM aarch64" npm_arch="arm64" loader="ld-musl-aarch64.so.1" ;;
  esac

  # no base: the launcher, Node and the bundled runtime exist, a shell does not
  [[ -x "$root/bin/claude-agent-acp" ]] || fail "$arch: bin/claude-agent-acp is missing or not executable"
  [[ "$(readlink "$root/bin/claude")" == "claude-agent-acp" ]] || fail "$arch: bin/claude is no link to the launcher"
  [[ -x "$root/node" ]] || fail "$arch: node is missing"
  for lib in "$loader" libstdc++.so.6 libgcc_s.so.1; do
    [[ -f "$root/lib/$lib" ]] || fail "$arch: lib/$lib is missing"
  done
  for sh in bin/sh bin/bash usr/bin/sh; do
    [[ ! -e "$root/$sh" ]] || fail "$arch: $sh exists: the image must not contain a shell"
  done

  # the launcher is a static executable of the right architecture
  local info
  info="$(file -L "$root/bin/claude-agent-acp")"
  grep -q "ELF 64-bit LSB" <<<"$info" || fail "$arch: the launcher is not an ELF executable: $info"
  grep -q "$file_arch" <<<"$info" || fail "$arch: the launcher is not $file_arch: $info"
  grep -Eq "statically linked|static-pie" <<<"$info" || fail "$arch: the launcher is not static: $info"

  # the Claude Code CLI of this architecture, at the path the launcher uses, and no other build of it
  local cli="$root/node_modules/@anthropic-ai/claude-agent-sdk-linux-$npm_arch-musl/claude"
  [[ -x "$cli" ]] || fail "$arch: the Claude Code CLI is missing at ${cli#"$root/"}"
  local others
  others="$(find "$root/node_modules" -name 'claude-agent-sdk-*-*' ! -name "claude-agent-sdk-linux-$npm_arch-musl*" | head -n 3)"
  [[ -z "$others" ]] || fail "$arch: other builds of the Claude Code CLI in the image: $others"

  # Node and the CLI need nothing beyond the bundled runtime (the musl libc is the loader itself)
  local bin needed
  for bin in "$root/node" "$cli"; do
    grep -q "$file_arch" <<<"$(file -L "$bin")" || fail "$arch: ${bin#"$root/"} is not $file_arch"
    needed="$(readelf -d "$bin" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' |
      grep -Ev '^(libstdc\+\+\.so\.6|libgcc_s\.so\.1|libc\.musl-.*\.so\.1|libc\.so)$' || true)"
    [[ -z "$needed" ]] || fail "$arch: ${bin#"$root/"} needs libraries the image lacks: $needed"
  done

  # the installed adapter is the version pinned in package.json
  local installed expected
  installed="$(jq -r .version "$root/node_modules/@agentclientprotocol/claude-agent-acp/package.json")"
  expected="$(jq -r '.dependencies["@agentclientprotocol/claude-agent-acp"]' "$PACKAGE_JSON")"
  [[ "$installed" == "$expected" ]] || fail "$arch: adapter version is '$installed', expected '$expected'"
  echo "structure ok: $arch"
}

handshake() { # <root>
  local root="$1" project_image reply
  local init='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}'
  HS="df-handshake-$$" # named, so cleanup can remove it
  for project_image in debian:trixie-slim alpine; do
    $CLI pull -q "$project_image" >/dev/null # keep the pull out of the read timeout
    # The adapter answers on its first line. Read it with a bounded wait, then remove the container
    # (the client would otherwise wait for more input). The process substitution keeps stdin open
    # and, not being waited for by the shell, does not delay the pipeline; its `sleep` (stderr closed,
    # so it cannot hold the log) ends on its own.
    # shellcheck disable=SC2086 # CONTAINER_CLI may hold arguments
    reply="$(timeout 120 $CLI run --rm -i --name "$HS" -v "$root:/opt/harness/claude-code:ro" \
      "$project_image" /opt/harness/claude-code/bin/claude-agent-acp 2>"$WORK/acp.err" \
      < <(echo "$init"; exec sleep 30 2>/dev/null) |
      { read -t 30 -r line || true; echo "${line:-}"; $CLI rm -f "$HS" >/dev/null 2>&1 || true; })" || true
    grep -q '"agentInfo"' <<<"$reply" ||
      fail "no ACP initialize reply in $project_image: '$reply' stderr: $(head -c 2000 "$WORK/acp.err")"
    echo "handshake ok in $project_image"
  done
}

archs="$(jq -r '.manifests[].platform.architecture' "$LAYOUT/index.json" | sort | tr '\n' ' ')"
[[ "$archs" == "amd64 arm64 " ]] || fail "expected an index with amd64 and arm64, got: $archs"

for arch in amd64 arm64; do
  extract "$arch" "$WORK/$arch"
  check_structure "$arch" "$WORK/$arch"
done

if [[ "$HANDSHAKE" == "--handshake" ]]; then
  case "$(uname -m)" in
  x86_64) handshake "$WORK/amd64" ;;
  aarch64 | arm64) handshake "$WORK/arm64" ;;
  *) fail "unsupported host architecture $(uname -m)" ;;
  esac
fi

echo "OK"
