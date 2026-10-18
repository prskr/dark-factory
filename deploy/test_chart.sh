#!/usr/bin/env bash
# Usage: test_chart.sh <helm> <kubeconform> <yq> <kube-linter> <.kube-linter.yaml> <Chart.yaml> <values-dev.yaml>
set -euo pipefail

helm="$1"
kubeconform="$2"
yq="$3"
kube_linter="$4"
KUBE_LINTER_CONFIG="$5"
CHART_FILE="$6"
VALUES_DEV_FILE="$7"
CHART_DIR="$(dirname "$CHART_FILE")"

echo "=== 1. Verifying every key in values.yaml is documented in README.md ==="
# Leaf keys: scalars, arrays and empty maps; the ingress selectors are leaves even though they are maps.
leaves="$("$yq" '
  .. | select(tag != "!!map" or length == 0
      or (path | join(".")) == "networkPolicy.ingressController.namespaceSelector"
      or (path | join(".")) == "networkPolicy.ingressController.podSelector")
  | path | select(map(select(tag == "!!int")) | length == 0) | join(".")
  | select(test("^networkPolicy\.ingressController\.(namespaceSelector|podSelector)\.") | not)
' "$CHART_DIR/values.yaml")"

missing=()
count=0
while IFS= read -r key; do
  count=$((count + 1))
  grep -qF -- "\`$key\`" "$CHART_DIR/README.md" || missing+=("$key")
done <<<"$leaves"
if ((${#missing[@]})); then
  echo "Missing keys in README.md: ${missing[*]}" >&2
  exit 1
fi
echo "All $count leaf keys documented in README.md."

echo "=== 2. Running helm lint ==="
HOME="$(mktemp -d)" "$helm" lint "$CHART_DIR"
HOME="$(mktemp -d)" "$helm" lint "$CHART_DIR" -f "$VALUES_DEV_FILE"

echo "=== 3. Running kubeconform ==="
HOME="$(mktemp -d)" "$helm" template dark-factory "$CHART_DIR" | "$kubeconform" -strict -ignore-missing-schemas -summary
HOME="$(mktemp -d)" "$helm" template dark-factory "$CHART_DIR" -f "$VALUES_DEV_FILE" | "$kubeconform" -strict -ignore-missing-schemas -summary

echo "=== 4. Running kube-linter ==="
# --namespace avoids false positives for objects in the "default" namespace.
HOME="$(mktemp -d)" "$helm" template dark-factory "$CHART_DIR" --namespace dark-factory >"$TEST_TMPDIR/default.yaml"
HOME="$(mktemp -d)" "$helm" template dark-factory "$CHART_DIR" --namespace dark-factory -f "$VALUES_DEV_FILE" >"$TEST_TMPDIR/dev.yaml"
"$kube_linter" lint --config "$KUBE_LINTER_CONFIG" "$TEST_TMPDIR/default.yaml" "$TEST_TMPDIR/dev.yaml"
