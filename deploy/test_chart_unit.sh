#!/usr/bin/env bash
# Runs the chart's helm-unittest suites. Usage: test_chart_unit.sh <helm-unittest> <Chart.yaml>
set -euo pipefail

untt="$1"
chart_dir="$(dirname "$2")"

exec "$untt" "$chart_dir"
