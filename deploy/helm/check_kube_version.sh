#!/usr/bin/env bash
# The chart requires Kubernetes 1.36 (image volumes GA): rendering for 1.35 must fail, for 1.36 succeed.
set -euo pipefail
chart="$(cd "$(dirname "$0")" && pwd)/dark-factory"

if helm template t "$chart" --kube-version 1.35.0 >/dev/null 2>&1; then
  echo "FAIL: chart renders for Kubernetes 1.35" >&2
  exit 1
fi
helm template t "$chart" --kube-version 1.36.0 >/dev/null || {
  echo "FAIL: chart does not render for Kubernetes 1.36" >&2
  exit 1
}
