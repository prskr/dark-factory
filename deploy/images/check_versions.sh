#!/usr/bin/env bash
# Fails when the copies of a harness version differ.
# - Claude Code: the source is the @agentclientprotocol/claude-agent-acp dependency in
#   deploy/images/claude-harness/package.json.
# - OpenCode: the source is OPENCODE_VERSION in MODULE.bazel (the release the opencode-harness image
#   ships).
# The image tag in the df-core default, the chart values and the reference documentation must equal
# it. Renovate bumps all copies of a harness in one PR (customManagers in renovate.json cover the same
# files).
set -euo pipefail
cd "$(dirname "$0")/../.."

want="$(sed -n 's/^ *"@agentclientprotocol\/claude-agent-acp": *"\([^"]*\)".*/\1/p' deploy/images/claude-harness/package.json)"
[[ -n "$want" ]] || { echo "no @agentclientprotocol/claude-agent-acp in the harness package.json" >&2; exit 1; }
want_opencode="$(sed -n 's/^OPENCODE_VERSION = "\([^"]*\)".*/\1/p' MODULE.bazel)"
[[ -n "$want_opencode" ]] || { echo "no OPENCODE_VERSION in MODULE.bazel" >&2; exit 1; }

status=0
check() { # <file> <found version> <wanted version> <harness>
  if [[ "$2" != "$3" ]]; then
    echo "$1: $4 version '$2', the source has '$3'" >&2
    status=1
  fi
}

for f in crates/df-core/src/config.rs crates/df-k8s/README.md crates/df-agent-channel/docs/configuration.md; do
  found="$(grep -o 'claude-harness:[0-9][0-9.]*' "$f" | head -n 1 | cut -d: -f2 || true)"
  check "$f" "$found" "$want" "harness"
  found="$(grep -o 'opencode-harness:[0-9][0-9.]*' "$f" | head -n 1 | cut -d: -f2 || true)"
  check "$f" "$found" "$want_opencode" "opencode harness"
done
chart_tag() { # <harness block name>
  awk -v block="$1" '$0 ~ "^    " block ":"{h=1} h&&/tag:/{print $2; exit}' deploy/helm/dark-factory/values.yaml | tr -d "' "
}
check deploy/helm/dark-factory/values.yaml "$(chart_tag claudeCode)" "$want" "harness"
check deploy/helm/dark-factory/values.yaml "$(chart_tag opencode)" "$want_opencode" "opencode harness"
readme_tag() { # <harness key>
  sed -n "s/^| \`image\\.harness\\.$1\\.tag\` | \`string\` | \`\"\\([^\"]*\\)\"\`.*/\\1/p" deploy/helm/dark-factory/README.md
}
check deploy/helm/dark-factory/README.md "$(readme_tag claudeCode)" "$want" "harness"
check deploy/helm/dark-factory/README.md "$(readme_tag opencode)" "$want_opencode" "opencode harness"
exit "$status"
