#!/usr/bin/env sh
# Bazel credential helper (https://github.com/EngFlow/credential-helper-spec) for remote.buildbuddy.io.
# Sends BUILDBUDDY_API_KEY as `x-buildbuddy-api-key`; without the variable no header is added.
set -eu

cat >/dev/null # request JSON ({"uri": ...}) is not needed: the helper is scoped to one host

if [ -n "${BUILDBUDDY_API_KEY:-}" ]; then
	printf '{"headers":{"x-buildbuddy-api-key":["%s"]}}\n' "$BUILDBUDDY_API_KEY"
else
	printf '{}\n'
fi
