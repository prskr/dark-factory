#!/usr/bin/env bash
set -euo pipefail
export PATH="$PATH:/opt/homebrew/bin:/usr/local/bin"

CHART_FILE="$1"
VALUES_DEV_FILE="$2"
CHART_DIR="$(dirname "$CHART_FILE")"

echo "=== 1. Verifying every key in values.yaml is documented in README.md ==="
ruby -ryaml -e '
values = YAML.load_file(ARGV[0])
readme = File.read(ARGV[1])
def leaves(obj, prefix = "")
  if obj.is_a?(Hash) && !obj.empty? && prefix != "networkPolicy.ingressController.namespaceSelector" && prefix != "networkPolicy.ingressController.podSelector"
    obj.flat_map { |k, v| leaves(v, prefix.empty? ? k.to_s : "#{prefix}.#{k}") }
  else
    [prefix]
  end
end
missing = leaves(values).reject { |k| readme.include?("`#{k}`") }
if missing.any?
  abort("Missing keys in README.md: #{missing.join(", ")}")
end
puts "All #{leaves(values).size} leaf keys documented in README.md."
' "$CHART_DIR/values.yaml" "$CHART_DIR/README.md"

echo "=== 2. Running helm lint ==="
HOME="$(mktemp -d)" helm lint "$CHART_DIR"
HOME="$(mktemp -d)" helm lint "$CHART_DIR" -f "$VALUES_DEV_FILE"

if command -v kubeconform >/dev/null 2>&1; then
  echo "=== 3. Running kubeconform ==="
  HOME="$(mktemp -d)" helm template dark-factory "$CHART_DIR" | kubeconform -strict -ignore-missing-schemas -summary
  HOME="$(mktemp -d)" helm template dark-factory "$CHART_DIR" -f "$VALUES_DEV_FILE" | kubeconform -strict -ignore-missing-schemas -summary
fi
