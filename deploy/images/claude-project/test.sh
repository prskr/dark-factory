#!/usr/bin/env bash
# Structure check of the reference Claude project image (openspec change harness-images, design D9).
# Usage: test.sh <image>      CONTAINER_CLI selects the container CLI (default: docker).
set -euo pipefail

IMAGE="${1:?usage: $0 <image>}"
CLI="${CONTAINER_CLI:-docker}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

run() { $CLI run --rm --entrypoint "" "$IMAGE" "$@"; }

for tool in git bash curl ssh; do
  run sh -c "command -v $tool" >/dev/null || fail "$tool is missing"
done
run test -f /etc/ssl/certs/ca-certificates.crt || fail "CA certificates are missing"
[[ "$(run id -u)" == "1000" ]] || fail "the image does not run as UID 1000"
[[ "$(run pwd)" == "/workspace" ]] || fail "the working directory is not /workspace"
# `|| true`: a missing binary is the expected outcome
run sh -c 'command -v node || command -v claude-agent-acp' && fail "node or claude-agent-acp is present" || true

echo "OK: $IMAGE"
