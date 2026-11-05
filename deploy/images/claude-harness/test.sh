#!/usr/bin/env bash
# Checks the Claude harness image (openspec change harness-images, design D9) from its OCI layout.
# Usage: test.sh <lib.sh> <jq> <oci layout> <package.json> [--acp]
#
# Without --acp: structure checks of both architectures (no container runtime needed; the
# `structure_test` target). With --acp: additionally, for the host architecture in a glibc and a
# musl project image (the `test` target), `bin/claude --version` and an ACP initialize through
# `bin/node` and the adapter, both from the image mounted at /opt/harness/claude-code. The container
# CLI is chosen in lib.sh.
set -euo pipefail

LIB="$1"
# shellcheck disable=SC2034 # JQ_BIN is read by jq() in lib.sh
JQ_BIN="$(realpath "$2")"
LAYOUT="$3"
PACKAGE_JSON="$4"
ACP="${5:-}"
# shellcheck source=deploy/images/lib.sh
source "$LIB"
WORK="$(mktemp -d)"
HS=""
cleanup() {
  [[ -z "$HS" ]] || $CLI rm -f "$HS" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

check_structure() { # <arch> <root>
  local arch="$1" root="$2" bin
  check_musl_runtime "$arch" "$root"
  for bin in node claude; do
    check_patched_binary "$arch" "$root" "bin/$bin" /opt/harness/claude-code
  done
  [[ -f "$root/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js" ]] ||
    fail "$arch: the adapter is missing"
  # bin/claude is the patched CLI; the unpatched build the SDK package brings along must not be here
  local unpatched
  unpatched="$(find "$root/node_modules" -name 'claude-agent-sdk-linux-*' | head -n 3)"
  [[ -z "$unpatched" ]] || fail "$arch: unpatched build of the Claude Code CLI in the image: $unpatched"

  # the installed adapter is the version pinned in package.json
  local installed expected
  installed="$(jq -r .version "$root/node_modules/@agentclientprotocol/claude-agent-acp/package.json")"
  expected="$(jq -r '.dependencies["@agentclientprotocol/claude-agent-acp"]' "$PACKAGE_JSON")"
  [[ "$installed" == "$expected" ]] || fail "$arch: adapter version is '$installed', expected '$expected'"
  echo "structure ok: $arch"
}

acp() { # <root>
  local root="$1" project_image reply version
  local init='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{}}}'
  HS="df-acp-test-$$" # named, so cleanup can remove it
  for project_image in debian:trixie-slim alpine; do
    $CLI pull -q "$project_image" >/dev/null # keep the pull out of the read timeout
    # shellcheck disable=SC2086 # CLI may hold arguments (CONTAINER_CLI)
    version="$(timeout 120 $CLI run --rm -v "$root:/opt/harness/claude-code:ro" \
      "$project_image" /opt/harness/claude-code/bin/claude --version 2>"$WORK/cli.err")" ||
      fail "bin/claude --version failed in $project_image: '$version' stderr: $(head -c 2000 "$WORK/cli.err")"
    echo "claude --version in $project_image: $version"
    # The adapter answers on its first line. Read it with a bounded wait, then remove the container
    # (the client would otherwise wait for more input). The process substitution keeps stdin open
    # and, not being waited for by the shell, does not delay the pipeline; its `sleep` (stderr closed,
    # so it cannot hold the log) ends on its own.
    # shellcheck disable=SC2086 # CLI may hold arguments (CONTAINER_CLI)
    reply="$(timeout 120 $CLI run --rm -i --name "$HS" -v "$root:/opt/harness/claude-code:ro" \
      "$project_image" /opt/harness/claude-code/bin/node \
      /opt/harness/claude-code/node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js 2>"$WORK/acp.err" \
      < <(echo "$init"; exec sleep 30 2>/dev/null) |
      { read -t 30 -r line || true; echo "${line:-}"; $CLI rm -f "$HS" >/dev/null 2>&1 || true; })" || true
    grep -q '"agentInfo"' <<<"$reply" ||
      fail "no ACP initialize reply in $project_image: '$reply' stderr: $(head -c 2000 "$WORK/acp.err")"
    echo "initialize ok in $project_image"
  done
}

archs="$(jq -r '.manifests[].platform.architecture' "$LAYOUT/index.json" | sort | tr '\n' ' ')"
[[ "$archs" == "amd64 arm64 " ]] || fail "expected an index with amd64 and arm64, got: $archs"

for arch in amd64 arm64; do
  extract "$arch" "$WORK/$arch"
  check_structure "$arch" "$WORK/$arch"
done

if [[ "$ACP" == "--acp" ]]; then
  case "$(uname -m)" in
  x86_64) acp "$WORK/amd64" ;;
  aarch64 | arm64) acp "$WORK/arm64" ;;
  *) fail "unsupported host architecture $(uname -m)" ;;
  esac
fi

echo "OK"
