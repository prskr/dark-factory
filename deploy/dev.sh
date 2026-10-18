#!/usr/bin/env bash
set -euo pipefail
export PATH="$PATH:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin"

if [[ -z "${RUNFILES_DIR:-}" && -d "${0}.runfiles" ]]; then
  export RUNFILES_DIR="${0}.runfiles"
fi

PUSH_IMAGES="$1"
CP_DIGEST_FILE="$2"
RT_DIGEST_FILE="$3"
TP_DIGEST_FILE="$4"
CHART_FILE="$5"
VALUES_DEV_FILE="$6"
shift 6

CHART_DIR="$(dirname "$CHART_FILE")"

REGISTRY="${DARK_FACTORY_REGISTRY:-harbor.home.icb4dc0.de/dark-factory}"
RELEASE="${DARK_FACTORY_RELEASE:-dark-factory}"
NAMESPACE="${DARK_FACTORY_NAMESPACE:-dark-factory-dev}"

CP_REPO="${REGISTRY}/control-plane"
RT_REPO="${REGISTRY}/runtime"
TP_REPO="${REGISTRY}/test-project"

CP_DIGEST="$(tr -d '[:space:]' < "$CP_DIGEST_FILE")"
RT_DIGEST="$(tr -d '[:space:]' < "$RT_DIGEST_FILE")"
TP_DIGEST="$(tr -d '[:space:]' < "$TP_DIGEST_FILE")"

echo "=== Pushing multi-arch OCI images to ${REGISTRY} ==="
# shellcheck disable=SC2086
"$PUSH_IMAGES" ${DARK_FACTORY_PUSH_ARGS:-}

echo "Control Plane Image: ${CP_REPO}@${CP_DIGEST}"
echo "Runtime Image:       ${RT_REPO}@${RT_DIGEST}"
echo "Test Project Image:  ${TP_REPO}@${TP_DIGEST}"

if [[ "${DARK_FACTORY_PUSH_ONLY:-0}" == "1" ]]; then
  echo "DARK_FACTORY_PUSH_ONLY=1 set; skipping helm upgrade."
  exit 0
fi

echo "=== Running helm upgrade --install ${RELEASE} in namespace ${NAMESPACE} ==="
helm upgrade --install "$RELEASE" "$CHART_DIR" \
  --namespace "$NAMESPACE" \
  --create-namespace=false \
  --values "$VALUES_DEV_FILE" \
  --set "image.controlPlane.repository=${CP_REPO}" \
  --set "image.controlPlane.digest=${CP_DIGEST}" \
  --set "image.runtime.repository=${RT_REPO}" \
  --set "image.runtime.digest=${RT_DIGEST}" \
  --set "image.testProject.repository=${TP_REPO}" \
  --set "image.testProject.digest=${TP_DIGEST}" \
  --wait \
  --timeout 3m "$@"
