#!/usr/bin/env bash
# Fails when the copies of the harness adapter version differ. The source is the
# @agentclientprotocol/claude-agent-acp dependency in deploy/images/claude-harness/package.json; the
# image tag in the df-core default, the chart values and the reference documentation must equal it.
# Renovate bumps all of them in one PR (customManagers in renovate.json cover the same files).
set -euo pipefail
cd "$(dirname "$0")/../.."

want="$(sed -n 's/^ *"@agentclientprotocol\/claude-agent-acp": *"\([^"]*\)".*/\1/p' deploy/images/claude-harness/package.json)"
[[ -n "$want" ]] || { echo "no @agentclientprotocol/claude-agent-acp in the harness package.json" >&2; exit 1; }

status=0
check() { # <file> <found version>
  if [[ "$2" != "$want" ]]; then
    echo "$1: harness version '$2', package.json has '$want'" >&2
    status=1
  fi
}

for f in crates/df-core/src/config.rs crates/df-k8s/README.md crates/df-agent-channel/docs/configuration.md; do
  found="$(grep -o 'claude-harness:[0-9][0-9.]*' "$f" | head -n 1 | cut -d: -f2 || true)"
  check "$f" "$found"
done
check deploy/helm/dark-factory/values.yaml \
  "$(awk '/^  harness:/{h=1} h&&/tag:/{print $2; exit}' deploy/helm/dark-factory/values.yaml | tr -d "' ")"
check deploy/helm/dark-factory/README.md \
  "$(sed -n 's/^| `image\.harness\.claudeCode\.tag` | `string` | `"\([^"]*\)"`.*/\1/p' deploy/helm/dark-factory/README.md)"
exit "$status"
