#!/usr/bin/env bash
# Structure check of the Claude harness image (openspec change harness-images, design D9).
# Usage: test.sh <image>
# CONTAINER_CLI selects the container CLI (default: docker), HARNESS_ARCH the expected
# architecture (default: the image's).
set -euo pipefail

IMAGE="${1:?usage: $0 <image>}"
CLI="${CONTAINER_CLI:-docker}"
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
CID=""
HS=""
cleanup() {
  for c in "$CID" "$HS"; do
    [[ -z "$c" ]] || $CLI rm -f "$c" >/dev/null 2>&1 || true
  done
  rm -rf "$WORK"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

ARCH="${HARNESS_ARCH:-$($CLI image inspect --format '{{.Architecture}}' "$IMAGE")}"
case "$ARCH" in
amd64) FILE_ARCH="x86-64" ;;
arm64) FILE_ARCH="ARM aarch64" ;;
*) fail "unsupported architecture $ARCH" ;;
esac

# The image content, as the control plane mounts it at /opt/harness/claude-code.
CID="$($CLI create "$IMAGE" /bin/claude-agent-acp)"
ROOT="$WORK/root"
mkdir "$ROOT"
$CLI export "$CID" | tar -x -C "$ROOT"

# no base: the launcher, Node and the bundled loader exist, a shell does not
[[ -x "$ROOT/bin/claude-agent-acp" ]] || fail "bin/claude-agent-acp is missing or not executable"
[[ -x "$ROOT/bin/claude" ]] || fail "bin/claude is missing or not executable"
[[ -x "$ROOT/node" ]] || fail "node is missing"
compgen -G "$ROOT/lib/ld-musl-*.so.1" >/dev/null || fail "bundled musl loader is missing"
for sh in bin/sh bin/bash usr/bin/sh; do
  [[ ! -e "$ROOT/$sh" ]] || fail "$sh exists: the image must not contain a shell"
done

# the launcher is a static executable of the right architecture
for launcher in claude-agent-acp claude; do
  info="$(file -L "$ROOT/bin/$launcher")"
  grep -q "ELF 64-bit LSB" <<<"$info" || fail "bin/$launcher is not an ELF executable: $info"
  grep -q "$FILE_ARCH" <<<"$info" || fail "bin/$launcher is not $FILE_ARCH: $info"
  grep -Eq "statically linked|static-pie" <<<"$info" || fail "bin/$launcher is not static: $info"
done

# the installed adapter is the pinned version (the image tag when it is a version)
installed="$(sed -n 's/^ *"version": *"\([^"]*\)".*/\1/p' \
  "$ROOT/node_modules/@agentclientprotocol/claude-agent-acp/package.json" | head -n 1)"
expected="$(sed -n 's/^ARG CLAUDE_AGENT_ACP_VERSION=//p' "$HERE/Dockerfile")"
tag="${IMAGE##*:}"
[[ "$tag" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]] && expected="$tag"
[[ "$installed" == "$expected" ]] || fail "adapter version is '$installed', expected '$expected'"

# the adapter completes an ACP initialize handshake in a glibc and a musl project image
INIT='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}'
HS="df-handshake-$$" # named, so cleanup can remove it
for project_image in debian:trixie-slim alpine; do
  $CLI pull -q "$project_image" >/dev/null # keep the pull out of the read timeout
  # The adapter answers on its first line. Read it with a bounded wait, then remove the container
  # (the client would otherwise wait for more input). The process substitution keeps stdin open
  # and, not being waited for by the shell, does not delay the pipeline; its `sleep` (stderr closed, so
  # it cannot hold the log) ends on its own.
  # shellcheck disable=SC2086 # CONTAINER_CLI may hold arguments
  reply="$(timeout 120 $CLI run --rm -i --name "$HS" -v "$ROOT:/opt/harness/claude-code:ro" \
    "$project_image" /opt/harness/claude-code/bin/claude-agent-acp 2>"$WORK/acp.err" \
    < <(echo "$INIT"; exec sleep 30 2>/dev/null) |
    { read -t 30 -r line || true; echo "${line:-}"; $CLI rm -f "$HS" >/dev/null 2>&1 || true; })" || true
  grep -q '"agentInfo"' <<<"$reply" ||
    fail "no ACP initialize reply in $project_image: '$reply' stderr: $(head -c 2000 "$WORK/acp.err")"
  echo "handshake ok in $project_image"
done

echo "OK: $IMAGE"
