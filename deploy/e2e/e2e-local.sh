#!/usr/bin/env bash
# `just e2e-local`: runs the end-to-end suite (crates/df-e2e) against a throwaway kind cluster that
# this script creates and deletes. Details and prerequisites: docs/demo.md.
#
# Environment:
#   E2E_KEEP=1        keep the cluster after the run (delete it with `kind delete cluster --name df-e2e`)
#   E2E_TOOLS_DIR     where the Playwright driver installer is kept (default: ~/.cache/df-e2e)
#   E2E_TEST_THREADS  tests run in parallel per test binary (default: 1, the memory budget of docs/demo.md)
#   E2E_TEST_ARGS     arguments after `--` of `cargo test` (default: --ignored --test-threads=$E2E_TEST_THREADS)
#   BAZEL             Bazel command, e.g. "bazel --bazelrc=.bazelrc.agents" (default: bazel)
#   KIND_EXPERIMENTAL_PROVIDER, DOCKER_HOST   as for kind; Podman is used when Docker is not installed
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

cluster=df-e2e
ctx="kind-$cluster"
namespace=dark-factory
release=dark-factory

# Pinned versions of what the cluster runs besides the images built from this repository.
cert_manager_version=v1.21.3
csi_driver_version=v0.16.1
forgejo_chart_version=17.3.2

# Ports of the host side of the port-forwards.
pg_port=15432
forgejo_port=13000
playwright_port=13001
receiver_port=13002

# Credentials of the throwaway cluster (not secrets).
owner_password=e2e-owner-password
app_password=e2e-app-password
oidc_secret=e2e-oidc-secret
forgejo_user=factory
forgejo_password=e2e-factory-password
forgejo_reviewer=reviewer
forgejo_reviewer_password=e2e-reviewer-password
webhook_secret=e2e-webhook-secret

kc() { kubectl --context "$ctx" "$@"; }
hc() { helm --kube-context "$ctx" "$@"; }
log() { printf '\n=== %s\n' "$*" >&2; }

scratch="$(mktemp -d)"
pids=()

cleanup() {
  local status=$?
  trap - EXIT
  for pid in "${pids[@]}"; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$scratch"
  if [[ "${E2E_KEEP:-0}" == 1 ]]; then
    log "E2E_KEEP=1: cluster kept; delete it with: kind delete cluster --name $cluster"
  else
    log "Deleting the cluster"
    kind delete cluster --name "$cluster" || true
  fi
  exit "$status"
}
trap cleanup EXIT

require() {
  for tool in "$@"; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool (see docs/development.md, 'Pinned linters')" >&2; exit 1; }
  done
}

prepare_tools() {
  require kind kubectl helm curl cargo git
  read -ra bazel <<<"${BAZEL:-bazel}"
  require "${bazel[0]}"
  case "$(uname -m)" in
    x86_64) arch=amd64 ;;
    aarch64 | arm64) arch=arm64 ;;
    *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
  esac
  if ! command -v docker >/dev/null; then
    require podman
    export KIND_EXPERIMENTAL_PROVIDER=podman
  fi
}

create_cluster() {
  log "Creating the kind cluster"
  kind delete cluster --name "$cluster" >/dev/null 2>&1 || true
  kind create cluster --name "$cluster" --config deploy/e2e/kind.yaml --wait 180s
}

# Builds the single-platform images with Bazel and imports their tarballs into the node: no registry,
# no container daemon. (`kind load docker-image` imports with --all-platforms, which fails on the
# multi-arch index when only one platform was loaded into the daemon.)
load_images() {
  local name archive
  for name in control_plane runtime test_project plugin_forgejo plugin_github; do
    archive="$scratch/$name.tar"
    "${bazel[@]}" run --platforms="//build/platforms:linux_${arch}_musl" \
      --@rules_img//img/settings:load_daemon=tar "//deploy:load_e2e_$name" >"$archive"
    kind load image-archive "$archive" --name "$cluster"
  done
}

install_infrastructure() {
  log "Installing cert-manager, PostgreSQL, Dex, the browser, the event receiver and the fake model upstream"
  kc apply -f "https://github.com/cert-manager/cert-manager/releases/download/${cert_manager_version}/cert-manager.yaml"
  kc apply -f deploy/e2e/postgres.yaml -f deploy/e2e/dex.yaml -f deploy/e2e/playwright.yaml \
    -f deploy/e2e/receiver.yaml -f deploy/e2e/fake-upstream.yaml
  kc -n cert-manager wait --for=condition=Available deployment --all --timeout=300s
  hc upgrade --install cert-manager-csi-driver "oci://quay.io/jetstack/charts/cert-manager-csi-driver" \
    --version "$csi_driver_version" --namespace cert-manager --wait --timeout 5m
  log "Installing Forgejo"
  hc upgrade --install forgejo "oci://code.forgejo.org/forgejo-helm/forgejo" \
    --version "$forgejo_chart_version" --namespace forgejo --create-namespace \
    --values deploy/e2e/forgejo-values.yaml --wait --timeout 8m
  kc -n postgres wait --for=condition=Available deployment/postgres --timeout=300s
  kc -n dex wait --for=condition=Available deployment/dex --timeout=300s
  kc -n playwright wait --for=condition=Available deployment/playwright --timeout=300s
  kc -n receiver wait --for=condition=Available deployment/receiver --timeout=300s
}

# The model provider's credential reference points at this Secret (the fake upstream expects the
# same key); the sandboxes never see it.
create_model_secret() {
  kc -n "$namespace" create secret generic fake-llm-key --from-literal=key=e2e-model-api-key
}

# The fake upstream runs from the test project image, which is imported by `load_images`.
wait_for_fake_upstream() {
  kc -n fake-upstream wait --for=condition=Available deployment/fake-anthropic --timeout=300s
}

forward() { # namespace service host-port service-port
  kc -n "$1" port-forward "svc/$2" "$3:$4" >"$scratch/forward-$2.log" 2>&1 &
  pids+=("$!")
  for _ in $(seq 30); do
    (: >"/dev/tcp/127.0.0.1/$3") 2>/dev/null && return 0
    sleep 1
  done
  cat "$scratch/forward-$2.log" >&2
  echo "port-forward to $2 did not come up" >&2
  return 1
}

forgejo_api() { # method path [json-body]
  curl -sf -X "$1" -H "Authorization: token $forgejo_token" -H 'Content-Type: application/json' \
    "http://127.0.0.1:$forgejo_port/api/v1/$2" ${3:+-d "$3"} >/dev/null
}

# Creates Forgejo user `$1` with password `$2` and prints its access token (all scopes).
create_forgejo_user() {
  local created token
  created="$(kc -n forgejo exec deploy/forgejo -c forgejo -- forgejo admin user create \
    --username "$1" --password "$2" --email "$1@example.com" \
    --must-change-password=false --access-token --access-token-name e2e --access-token-scopes all)"
  token="$(sed -n 's/.*Access token was successfully created\.\.\. //p' <<<"$created")"
  [[ -n "$token" ]] || { echo "no access token in: $created" >&2; exit 1; }
  printf '%s' "$token"
}

# The bot user with its token, the repository factory/demo with the issue the demo tasks link to and
# the webhook to the control plane, and a second user with write access who reviews.
setup_forgejo() {
  log "Setting up Forgejo"
  forgejo_token="$(create_forgejo_user "$forgejo_user" "$forgejo_password")"
  forgejo_reviewer_token="$(create_forgejo_user "$forgejo_reviewer" "$forgejo_reviewer_password")"
  forward forgejo forgejo-http "$forgejo_port" 3000
  forgejo_api POST user/repos '{"name":"demo","auto_init":true,"default_branch":"main"}'
  forgejo_api POST "repos/$forgejo_user/demo/issues" '{"title":"Add a greeting","body":"Linked from the demo tasks."}'
  forgejo_api PUT "repos/$forgejo_user/demo/collaborators/$forgejo_reviewer" '{"permission":"write"}'
  # The events of docs/content/integrations/forges/webhooks.md; `pull_request_only` is the
  # "Modification" checkbox of the form.
  local hook_url="http://$release-control-plane.$namespace.svc:8080/webhooks/forgejo"
  local events='["issue_comment","pull_request_only","pull_request_comment","pull_request_review","action_run_failure"]'
  forgejo_api POST "repos/$forgejo_user/demo/hooks" \
    "{\"type\":\"forgejo\",\"active\":true,\"events\":$events,\"config\":{\"url\":\"$hook_url\",\"content_type\":\"json\",\"secret\":\"$webhook_secret\"}}"
  check_forgejo_push
  # The sandboxes read the token through a credential reference (Secret in the release namespace).
  kc -n "$namespace" create secret generic forgejo-token --from-literal=token="$forgejo_token"
}

# Fails early when the token cannot push: pushes a throwaway branch over the port-forward.
check_forgejo_push() {
  local url="http://127.0.0.1:$forgejo_port/$forgejo_user/demo.git" work="$scratch/push-check"
  local git=(git -c "http.extraHeader=Authorization: token $forgejo_token")
  "${git[@]}" clone --quiet "$url" "$work"
  git -C "$work" -c user.name=e2e -c user.email=e2e@example.com commit --quiet --allow-empty -m "push check"
  "${git[@]}" -C "$work" push --quiet origin HEAD:refs/heads/e2e-push-check
  forgejo_api DELETE "repos/$forgejo_user/demo/branches/e2e-push-check"
}

install_release() {
  log "Installing Dark Factory"
  kc create namespace "$namespace"
  kc -n "$namespace" create secret generic dark-factory-db-owner --from-literal=password="$owner_password"
  kc -n "$namespace" create secret generic dark-factory-db-app --from-literal=password="$app_password"
  kc -n "$namespace" create secret generic dark-factory-oidc --from-literal=client-secret="$oidc_secret"
  # `plugins.forgejo.webhookSecret` of values-e2e.yaml; the repository webhook signs with it.
  kc -n "$namespace" create secret generic forge-webhooks --from-literal=forgejo="$webhook_secret"
  create_model_secret
}

helm_install_release() {
  hc upgrade --install "$release" deploy/helm/dark-factory --namespace "$namespace" \
    --values deploy/values-e2e.yaml --wait --timeout 5m
}

install_driver() {
  log "Installing the Playwright driver for the test host"
  # The workspace skips the driver download (.cargo/config.toml). `cargo install` in a directory
  # outside the repository does not read that setting, so the build assembles the driver in the
  # user cache, where the playwright-rs library looks for it. Reruns find it installed.
  local version tools="${E2E_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/df-e2e}"
  version="$(sed -n '/^name = "playwright-rs"$/{n;s/^version = "\(.*\)"$/\1/p;}' Cargo.lock)"
  [[ -n "$version" ]] || { echo "playwright-rs is not in Cargo.lock" >&2; exit 1; }
  (
    cd "$scratch"
    cargo install --quiet --locked --root "$tools" --version "$version" --features cli playwright-rs
    "$tools/bin/playwright-rs" install --driver-only
  )
}

suite_tests=(--test demo --test flow --test lifecycle_events --test webhooks)

# Compiles the seed program and the suite while the cluster comes up; `run_suite` reuses the build.
build_suite() {
  cargo build -q -p df-e2e --bin seed
  cargo test -q -p df-e2e "${suite_tests[@]}" --no-run
}

run_suite() {
  log "Seeding and running the suite"
  forward postgres postgres "$pg_port" 5432
  forward playwright playwright "$playwright_port" 3000
  forward receiver receiver "$receiver_port" 8080
  export E2E_DATABASE_URL="postgres://dark_factory_app:$app_password@127.0.0.1:$pg_port/dark_factory"
  cargo run -q -p df-e2e --bin seed
  export E2E_BASE_URL="http://$release-control-plane.$namespace.svc:8080"
  export E2E_PLAYWRIGHT_WS="ws://127.0.0.1:$playwright_port/?browser=chromium"
  export E2E_FORGEJO_URL="http://127.0.0.1:$forgejo_port"
  export E2E_FORGEJO_TOKEN="$forgejo_token"
  export E2E_FORGEJO_REVIEWER_TOKEN="$forgejo_reviewer_token"
  export E2E_RECEIVER_URL="http://127.0.0.1:$receiver_port"
  export E2E_USER=admin@example.com
  export E2E_PASSWORD=password
  read -ra test_args <<<"${E2E_TEST_ARGS:---ignored --test-threads=${E2E_TEST_THREADS:-1}}"
  cargo test -p df-e2e "${suite_tests[@]}" -- "${test_args[@]}"
}

prepare_tools
build_suite &
build_pid=$!
pids+=("$build_pid")
create_cluster
load_images &
images_pid=$!
pids+=("$images_pid")
install_infrastructure
install_release
setup_forgejo
wait "$images_pid"
wait_for_fake_upstream
helm_install_release
install_driver
wait "$build_pid"
run_suite
