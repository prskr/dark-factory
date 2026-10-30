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
PF_DIGEST_FILE="$5"
CHART_FILE="$6"
VALUES_DEV_FILE="$7"
shift 7

CHART_DIR="$(dirname "$CHART_FILE")"

REGISTRY="${DARK_FACTORY_REGISTRY:-ghcr.io/prskr/dark-factory}"
RELEASE="${DARK_FACTORY_RELEASE:-dark-factory}"
NAMESPACE="${DARK_FACTORY_NAMESPACE:-dark-factory-dev}"

CP_REPO="${REGISTRY}/control-plane"
RT_REPO="${REGISTRY}/runtime"
TP_REPO="${REGISTRY}/test-project"
PF_REPO="${REGISTRY}/plugin-forgejo"

CP_DIGEST="$(tr -d '[:space:]' < "$CP_DIGEST_FILE")"
RT_DIGEST="$(tr -d '[:space:]' < "$RT_DIGEST_FILE")"
TP_DIGEST="$(tr -d '[:space:]' < "$TP_DIGEST_FILE")"
PF_DIGEST="$(tr -d '[:space:]' < "$PF_DIGEST_FILE")"

echo "=== Pushing multi-arch OCI images to ${REGISTRY} ==="
# shellcheck disable=SC2086
"$PUSH_IMAGES" ${DARK_FACTORY_PUSH_ARGS:-}

# Builds deploy/images/<name> for amd64 and arm64, pushes ${REGISTRY}/<name>:dev and prints the digest.
push_dockerfile_image() {
  local name="$1" ref="${REGISTRY}/$1:dev" digestfile
  digestfile="$(mktemp)"
  case "$BUILDER" in
  buildah)
    buildah manifest rm "$ref" >/dev/null 2>&1 || true # --manifest appends to an existing list
    # --layers: Buildah caches no build steps by default, so every deploy reran npm/apt (emulated for the foreign arch)
    buildah build --layers --platform "$PLATFORMS" --manifest "$ref" "$IMAGES_DIR/$name" >&2
    # shellcheck disable=SC2086
    buildah manifest push --all ${DARK_FACTORY_BUILDAH_PUSH_ARGS:-} --digestfile "$digestfile" "$ref" "docker://$ref" >&2
    ;;
  docker)
    local meta
    meta="$(mktemp)"
    docker buildx build --platform "$PLATFORMS" --push --tag "$ref" --metadata-file "$meta" "$IMAGES_DIR/$name" >&2
    sed -n 's/.*"containerimage.digest": *"\([^"]*\)".*/\1/p' "$meta" > "$digestfile"
    rm -f "$meta"
    ;;
  *)
    echo "DARK_FACTORY_IMAGE_BUILDER must be buildah or docker, got '${BUILDER}'" >&2
    exit 1
    ;;
  esac
  tr -d '[:space:]' < "$digestfile"
  rm -f "$digestfile"
}

HARNESS_REPO="${REGISTRY}/claude-harness"
HARNESS_DIGEST=""
if [[ "${DARK_FACTORY_SKIP_HARNESS_IMAGES:-0}" != "1" ]]; then
  echo "=== Building and pushing the Dockerfile images (claude-harness, claude-project) ==="
  IMAGES_DIR="${BUILD_WORKSPACE_DIRECTORY:-$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)}/deploy/images"
  PLATFORMS="linux/amd64,linux/arm64"
  BUILDER="${DARK_FACTORY_IMAGE_BUILDER:-$(command -v buildah >/dev/null && echo buildah || echo docker)}"
  HARNESS_DIGEST="$(push_dockerfile_image claude-harness)"
  PROJECT_DIGEST="$(push_dockerfile_image claude-project)"
  [[ -n "$HARNESS_DIGEST" && -n "$PROJECT_DIGEST" ]] || { echo "no digest for a pushed image" >&2; exit 1; }
  echo "Harness Image:       ${HARNESS_REPO}@${HARNESS_DIGEST}"
  echo "Project Image:       ${REGISTRY}/claude-project@${PROJECT_DIGEST}"
fi

echo "Control Plane Image: ${CP_REPO}@${CP_DIGEST}"
echo "Runtime Image:       ${RT_REPO}@${RT_DIGEST}"
echo "Test Project Image:  ${TP_REPO}@${TP_DIGEST}"
echo "Forgejo Plugin Image: ${PF_REPO}@${PF_DIGEST}"

if [[ "${DARK_FACTORY_PUSH_ONLY:-0}" == "1" ]]; then
  echo "DARK_FACTORY_PUSH_ONLY=1 set; skipping helm upgrade."
  exit 0
fi

harness_args=()
if [[ -n "$HARNESS_DIGEST" ]]; then
  harness_args=(--set "image.harness.claudeCode.repository=${HARNESS_REPO}"
    --set "image.harness.claudeCode.digest=${HARNESS_DIGEST}")
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
  --set "plugins.forgejo.image.repository=${PF_REPO}" \
  --set "plugins.forgejo.image.digest=${PF_DIGEST}" \
  ${harness_args[@]+"${harness_args[@]}"} \
  --wait \
  --timeout 3m "$@"
