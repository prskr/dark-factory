# Dark Factory developer commands. Run `just` to list them.
# `just` is optional: every recipe is a plain command you can copy into a shell.
# Details: docs/development.md

set shell := ["bash", "-euo", "pipefail", "-c"]

# Container CLI for `db`: docker if installed, else podman. Override with CONTAINER_CLI.
container := env("CONTAINER_CLI", `command -v docker >/dev/null 2>&1 && echo docker || echo podman`)

# Socket for testcontainers: DOCKER_HOST if set, else the rootless Podman socket if it exists, else
# empty (testcontainers then uses /var/run/docker.sock).
docker_host := env("DOCKER_HOST", `s="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/podman/podman.sock"; [ -S "$s" ] && echo "unix://$s" || true`)

# Environment prefix for cargo test runs: exports DOCKER_HOST only when a socket was found.
docker_env := if docker_host == "" { "" } else { "DOCKER_HOST=" + docker_host }

# Bazel flags that let sandboxed tests reach that socket.
bazel_test_socket := if docker_host == "" { "" } else { "--test_env=DOCKER_HOST=" + docker_host + if docker_host =~ "^unix://" { " --sandbox_writable_path=" + parent_directory(trim_start_match(docker_host, "unix://")) } else { "" } }

default:
    @just --list

# --- Setup ---------------------------------------------------------------------------------------

# Install the pinned linters on PATH (needs direnv) and the git hooks
tools:
    bazel run //:bazel_env
    LEFTHOOK_BIN=$PWD/.bazel_env/bin/lefthook .bazel_env/bin/lefthook install

# Local services from compose.yml: PostgreSQL 17 on :5432 and Dex (OIDC) on :5556. Tests do not use them.
db:
    {{container}} compose up -d
    for _ in $(seq 60); do {{container}} compose exec postgres pg_isready -q -U postgres 2>/dev/null && exit 0; sleep 1; done; \
        {{container}} compose logs postgres; echo "PostgreSQL not ready after 60 s" >&2; exit 1

# Stop and remove the local services and their data
db-rm:
    {{container}} compose down -v

# Run the control plane with the web UI on http://127.0.0.1:8080 without a Kubernetes cluster and with scripted network traffic (login: admin@example.com / password). Needs the `dev-tools` feature.
serve-local: db plugins
    mkdir -p target/local-dev
    printf 'postgres\n' > target/local-dev/owner-password
    printf 'app-local-password\n' > target/local-dev/app-password
    printf 'local-dev-secret\n' > target/local-dev/oidc-client-secret
    [ -f target/local-dev/agent.crt ] || openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
        -days 365 -subj /CN=localhost -keyout target/local-dev/agent.key -out target/local-dev/agent.crt 2>/dev/null
    cargo run -p dark-factory --features dev-tools -- --config dev/config.yaml migrate
    cargo run -p dark-factory --features dev-tools -- --config dev/config.yaml serve --no-cluster --demo-flows

# Remove the shared test PostgreSQL containers that testcontainers keeps running between test runs
test-db-rm:
    {{container}} ps -aq --filter name=df-test-postgres | xargs -r {{container}} rm -f

# --- Cargo (local; database tests need a Docker or Podman socket) ---------------------------------

build:
    cargo build --workspace

# Build the WebAssembly plugin components the df-plugins and dark-factory tests load. Uses rustup's
# cargo: the `cargo` from .bazel_env only has the host's standard library, not wasm32-wasip2.
plugins:
    RUSTC="$(rustup which rustc)" "$(rustup which cargo)" build --target wasm32-wasip2 -p forgejo-plugin -p test-probe-plugin -p github-plugin

# Run all tests (or the ones matching FILTER); database tests share one `df-test-postgres` container
test *FILTER: plugins
    {{docker_env}} cargo test --workspace {{FILTER}}

# Line coverage across the workspace
coverage: plugins
    {{docker_env}} cargo llvm-cov --workspace

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

fmt:
    cargo fmt --all

# Dependency checks: licenses/advisories, unused and outdated dependencies
deps:
    cargo deny check
    cargo machete
    cargo xtask outdated

# Run `dark-factory migrate` against the local database, see docs/development.md section 5
migrate CONFIG="/tmp/dark-factory-dev/config.yaml":
    cargo run -p dark-factory -- --config {{CONFIG}} migrate

# Regenerate the checked-in sqlx offline metadata after changing a query! macro (needs DATABASE_URL, see docs/development.md section 2)
sqlx-prepare:
    cargo sqlx prepare --workspace -- --all-targets
    SQLX_OFFLINE=true cargo build --workspace

# --- Bazel ---------------------------------------------------------------------------------------

# Build everything (Linux: static musl; add --config=cache to .bazelrc.user for the BuildBuddy cache)
bazel-build:
    bazel build //...

# Run all tests (or TARGETS); database tests reach the Docker or Podman socket from the sandbox
bazel-test *TARGETS="//...":
    bazel test {{bazel_test_socket}} {{TARGETS}}

# Clippy and rustfmt through Bazel (pinned, hermetic)
bazel-lint:
    bazel build --config=lint //crates/... //plugins/...
    bazel build --config=fmt //crates/... //plugins/...

# Proto lint
buf-lint:
    bazel test //contract:buf_lint_test

# Format the protos in place
buf-format:
    bazel run //contract:format

# Re-lock the pinned tools to the latest upstream releases (set GITHUB_TOKEN to avoid rate limits)
update-tools:
    cargo xtask update-tools

# --- Deployment ----------------------------------------------------------------------------------

# Push the images and deploy the Helm chart to the dev cluster (DARK_FACTORY_REGISTRY/_RELEASE/_NAMESPACE override the defaults)
dev:
    bazel run //deploy:dev

# Chart lint and unit tests
chart-test:
    bazel test //deploy:helm_chart_test //deploy:helm_chart_unittest

# --- Release (static musl binaries for amd64 and arm64) -------------------------------------------

# Build the OCI image layouts
release-build:
    bazel build //deploy:control_plane_oci_layout //deploy:runtime_oci_layout //deploy:test_project_oci_layout

# Check that the images are multi-arch and the binaries are static (Linux hosts only, skipped on macOS)
release-test:
    bazel test --skip_incompatible_explicit_targets //deploy:images_structure_test

# Push the release images to ghcr.io/prskr/dark-factory (log in with `docker login ghcr.io` first)
release-push TAG="dev":
    bazel run --//deploy:image_tag={{TAG}} //deploy:push_images
