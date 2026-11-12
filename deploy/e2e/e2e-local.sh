#!/usr/bin/env bash
# `just e2e-local`: runs the end-to-end suite (crates/df-e2e) against a throwaway kind cluster that
# this script creates and deletes. Details and prerequisites: docs/demo.md.
#
# Environment:
#   E2E_KEEP=1        keep the cluster after the run (delete it with `kind delete cluster --name df-e2e`)
#   E2E_TOOLS_DIR     where the Playwright driver is kept (default: ~/.cache/df-e2e)
#   E2E_KIND_CONFIG   kind configuration (default: deploy/e2e/kind.yaml, plus a mount of /.msb/tls/ca.pem into the node when that file exists)
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
  # `${a[@]+…}`: an empty array is unbound under `set -u` in Bash < 4.4 (macOS /bin/bash).
  for pid in ${pids[@]+"${pids[@]}"}; do
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$scratch"
  # Diagnostics are bounded: a hung API server must not keep the cluster from being deleted.
  diag() { kc --request-timeout=5s "$@"; }
  if [[ $status -ne 0 ]] && diag get nodes >/dev/null 2>&1; then
    log "Cluster state after the failure"
    diag get pods -A -o wide || true
    diag -n playwright describe pods || true
    diag -n playwright logs deployment/playwright --tail=200 --previous 2>/dev/null ||
      diag -n playwright logs deployment/playwright --tail=200 || true
    diag get events -A --sort-by=.lastTimestamp 2>/dev/null | tail -40 || true
  fi
  if [[ "${E2E_KEEP:-0}" == 1 ]]; then
    log "E2E_KEEP=1: cluster kept; delete it with: kind delete cluster --name $cluster"
  else
    log "Deleting the cluster"
    kind delete cluster --name "$cluster" || true
  fi
  exit "$status"
}
trap cleanup EXIT
# A cancelled CI job sends INT/TERM: exit through the EXIT trap, so the state is printed and the
# cluster deleted.
trap 'exit 130' INT
trap 'exit 143' TERM

require() {
  for tool in "$@"; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool (see docs/development.md, 'Pinned linters')" >&2; exit 1; }
  done
}

prepare_tools() {
  require kind kubectl helm curl cargo git gh jq
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
  kind create cluster --name "$cluster" --config "$(kind_config)" --wait 180s
  # Two CoreDNS endpoints need the iptables `statistic` match in kube-proxy, which some kernels
  # (agent sandboxes) lack; one failing Service fails the whole sync and no ClusterIP works.
  kc -n kube-system scale deployment/coredns --replicas=1
  kc -n kube-system rollout status deployment/coredns --timeout=120s
}

# Landlock ABI 6 needs Linux 6.12 or newer with `landlock` in the LSM list. The kind node runs the
# kernel of the host (or of the container VM), so the node is asked. A container does not mount
# securityfs, which holds the LSM list (`/sys/kernel/security/lsm`), so the privileged exec mounts it
# first; a kernel without securityfs fails that mount and counts as having no Landlock. Sets
# `landlock` for the release (`sandbox.landlock`) and for the suite (`E2E_LANDLOCK`): without ABI 6
# the sandboxes would refuse every run, so the suite runs unconfined and its Landlock check skips
# itself. Works with docker and podman (`exec --privileged` exists in both).
detect_landlock() {
  local cli=docker node="$cluster-control-plane" kernel
  [[ "${KIND_EXPERIMENTAL_PROVIDER:-}" == podman ]] && cli=podman
  kernel="$("$cli" exec "$node" uname -r 2>/dev/null)" || kernel=
  if [[ "$kernel" =~ ^([0-9]+)\.([0-9]+) ]] &&
    ((BASH_REMATCH[1] > 6 || (BASH_REMATCH[1] == 6 && BASH_REMATCH[2] >= 12))) &&
    "$cli" exec --privileged "$node" sh -c \
      'grep -qw landlock /sys/kernel/security/lsm 2>/dev/null ||
       { mount -t securityfs securityfs /sys/kernel/security 2>/dev/null &&
         grep -qw landlock /sys/kernel/security/lsm; }'; then
    landlock=enforce
  else
    landlock=disabled
    log "kernel '${kernel:-unknown}' of the cluster offers no Landlock ABI 6 (Linux 6.12+ with landlock in /sys/kernel/security/lsm): sandbox.landlock=disabled, the Landlock check is skipped"
  fi
}

# Prints the kind configuration: E2E_KIND_CONFIG, else deploy/e2e/kind.yaml. Behind a TLS-intercepting
# proxy (CA at $ca_cert) the default gets the CA mounted into /etc/ssl/certs of the node, which the
# node's containerd (Go) reads in addition to the CA bundle, so image pulls trust the proxy.
ca_cert=/.msb/tls/ca.pem
kind_config() {
  if [[ -n "${E2E_KIND_CONFIG:-}" || ! -f "$ca_cert" ]]; then
    printf '%s' "${E2E_KIND_CONFIG:-deploy/e2e/kind.yaml}"
    return
  fi
  # Appends to the last (only) node of kind.yaml.
  cat deploy/e2e/kind.yaml - >"$scratch/kind.yaml" <<EOF
    extraMounts:
      - hostPath: $ca_cert
        containerPath: /etc/ssl/certs/e2e-proxy-ca.pem
        readOnly: true
EOF
  printf '%s' "$scratch/kind.yaml"
}

# Builds the single-platform images with Bazel and imports their tarballs into the node: no registry,
# no container daemon. (`kind load docker-image` imports with --all-platforms, which fails on the
# multi-arch index when only one platform was loaded into the daemon.)
load_images() {
  local name archive
  for name in control_plane runtime test_project plugin_forgejo plugin_github playwright; do
    archive="$scratch/$name.tar"
    "${bazel[@]}" run --platforms="//build/platforms:linux_${arch}_musl" \
      --@rules_img//img/settings:load_daemon=tar "//deploy:load_e2e_$name" >"$archive"
    kind load image-archive "$archive" --name "$cluster"
    rm -f "$archive" # the Playwright image alone is about 1 GB
  done
}

install_infrastructure() {
  log "Installing cert-manager, PostgreSQL, Dex, the browser, the event receiver and the fake model upstream"
  # Through the API: plain `releases/download` URLs time out (504) from some networks.
  gh release download "$cert_manager_version" --repo cert-manager/cert-manager \
    --pattern cert-manager.yaml --dir "$scratch" --clobber
  kc apply -f "$scratch/cert-manager.yaml"
  kc apply -f deploy/e2e/postgres.yaml -f deploy/e2e/dex.yaml \
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
  kc -n receiver wait --for=condition=Available deployment/receiver --timeout=300s
}

# The Playwright server runs from an image that `load_images` imports; applied afterwards, so the
# pod does not back off on a pull of the missing image.
install_playwright() {
  kc apply -f deploy/e2e/playwright.yaml
  kc -n playwright wait --for=condition=Available deployment/playwright --timeout=300s
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
  # The file the hidden-path demo project (`HP`) hides from its agent.
  forgejo_api POST "repos/$forgejo_user/demo/contents/secret/value.txt" \
    '{"content":"c2VjcmV0Cg==","message":"Add a secret file"}'
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
    --values deploy/values-e2e.yaml --set "sandbox.landlock=$landlock" --wait --timeout 5m
}

# Source directory of the locked `playwright-rs` crate.
playwright_rs_src() {
  dirname "$(cargo metadata --quiet --format-version 1 --locked |
    jq -r '.packages[] | select(.name == "playwright-rs") | .manifest_path')"
}

# Playwright version of the driver in the locked `playwright-rs` (`PLAYWRIGHT_VERSION` in its build.rs).
playwright_rs_version() {
  sed -n 's/^const PLAYWRIGHT_VERSION: &str = "\(.*\)";$/\1/p' "$(playwright_rs_src)/build.rs"
}

# The browser server and the test driver must run the same Playwright version: playwright-core in
# deploy/images/playwright-server/package.json, the base image tag in MODULE.bazel and the driver
# of playwright-rs. Renovate groups all three (renovate.json); the check catches a playwright-rs release that lags.
check_playwright_versions() {
  local server image driver
  server="$(jq -r '.dependencies["playwright-core"]' deploy/images/playwright-server/package.json)"
  image="$(sed -n '/name = "playwright_base"/,/^)/s/^    tag = "v\(.*\)-noble",$/\1/p' MODULE.bazel)"
  driver="$(playwright_rs_version)"
  if [[ -z "$server" || "$server" != "$image" || "$server" != "$driver" ]]; then
    echo "Playwright versions differ: playwright-core in deploy/images/playwright-server/package.json: '$server'," \
      "base image tag in MODULE.bazel (playwright_base): '$image', playwright-rs driver: '$driver'" >&2
    exit 1
  fi
}

install_driver() {
  log "Installing the Playwright driver for the test host"
  # The workspace skips the driver download (.cargo/config.toml). The driver is what playwright-rs
  # assembles itself: the playwright-core npm package next to a pinned Node binary. Its installer
  # trusts only built-in roots, curl the system CA store (needed behind a TLS-intercepting proxy).
  # The versions come from the source of the locked playwright-rs. Reruns find the driver installed.
  local src pw node os node_arch driver tools="${E2E_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/df-e2e}"
  src="$(playwright_rs_src)"
  pw="$(playwright_rs_version)"
  node="$(sed -n 's/^const NODE_VERSION: &str = "\(.*\)";$/\1/p' "$src/src/build_support/driver_urls.rs")"
  [[ -n "$pw" && -n "$node" ]] || { echo "no Playwright or Node version in $src" >&2; exit 1; }
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  node_arch="${arch/amd64/x64}"
  driver="$tools/playwright-$pw-node$node-$os-$node_arch"
  if [[ ! -f "$driver/package/cli.js" ]]; then
    rm -rf "$driver.tmp"
    mkdir -p "$driver.tmp"
    curl -sSfL "https://registry.npmjs.org/playwright-core/-/playwright-core-$pw.tgz" | tar -xz -C "$driver.tmp"
    curl -sSfL "https://nodejs.org/dist/v$node/node-v$node-$os-$node_arch.tar.gz" |
      tar -xz -C "$driver.tmp" --strip-components=2 "node-v$node-$os-$node_arch/bin/node"
    mv "$driver.tmp" "$driver"
  fi
  export PLAYWRIGHT_DRIVER_PATH="$driver"
}

# Tests that need a verified HTTPS connection from a pod to a public host: the gateway never connects
# to private addresses, so no in-cluster host can stand in.
internet_tests=(denied_host_is_requested_and_the_approval_lets_the_retry_through)

# A pod with the CA store of the sandboxes (the test project image) fetches a public page. Fails
# without egress and behind a TLS-intercepting proxy whose CA the image does not trust (agent sandboxes).
pods_reach_the_internet() {
  kc -n playwright run egress-probe --rm -i --quiet --restart=Never \
    --image=ghcr.io/prskr/dark-factory/test-project:e2e --image-pull-policy=IfNotPresent \
    --command -- curl -sSf -m 10 -o /dev/null https://pypi.org/simple/pip/ >/dev/null 2>&1
}

suite_tests=(--test delegation --test demo --test flow --test interrupt --test landlock --test lifecycle_events --test ui --test webhooks)

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
  export E2E_LANDLOCK="$landlock"
  export E2E_USER=admin@example.com
  export E2E_PASSWORD=password
  read -ra test_args <<<"${E2E_TEST_ARGS:---ignored --test-threads=${E2E_TEST_THREADS:-1}}"
  if ! pods_reach_the_internet; then
    # CI has egress without interception: a failed probe there is a real failure, not a reason to skip.
    if [[ -n "${CI:-}" ]]; then
      echo "pods cannot fetch https://pypi.org in CI; the tests ${internet_tests[*]} need it" >&2
      exit 1
    fi
    log "pods cannot fetch https://pypi.org (no egress, or a TLS-intercepting proxy their CA store does not trust): skipping ${internet_tests[*]}"
    for name in "${internet_tests[@]}"; do test_args+=(--skip "$name"); done
  fi
  cargo test -p df-e2e "${suite_tests[@]}" -- "${test_args[@]}"
}

prepare_tools
check_playwright_versions
build_suite &
build_pid=$!
pids+=("$build_pid")
create_cluster
detect_landlock
load_images &
images_pid=$!
pids+=("$images_pid")
install_infrastructure
install_release
setup_forgejo
wait "$images_pid"
install_playwright
wait_for_fake_upstream
helm_install_release
install_driver
wait "$build_pid"
run_suite
