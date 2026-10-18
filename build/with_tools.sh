#!/usr/bin/env bash
# Runs a command with the Bazel-pinned tools (//:bazel_env) first on PATH.
# Usage: build/with_tools.sh <tool> [args...]
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="$root/.bazel_env/bin"

if [[ ! -d "$bin" ]]; then
  echo "error: $bin missing; run 'bazel run //:bazel_env'" >&2
  exit 1
fi

PATH="$bin:$PATH" exec "$@"
