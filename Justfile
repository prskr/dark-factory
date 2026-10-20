# Dark Factory developer commands. Run `just` to list them.
# `just` is optional: every recipe is a plain command you can copy into a shell.
# Details: docs/development.md

set shell := ["bash", "-euo", "pipefail", "-c"]

# Remote cache and execution on BuildBuddy; put the API key into .bazelrc.user:
#   build:remote --remote_header=x-buildbuddy-api-key=<key>
remote := "--config=remote"

default:
    @just --list

# --- Setup ---------------------------------------------------------------------------------------

# Install the pinned linters on PATH (needs direnv) and the git hooks
tools:
    bazel run //:bazel_env
    LEFTHOOK_BIN=$PWD/.bazel_env/bin/lefthook .bazel_env/bin/lefthook install

# Start a local PostgreSQL 17 container for `sqlx` tooling and manual runs (tests start their own)
db:
    docker start dark-factory-pg 2>/dev/null || docker run -d --name dark-factory-pg \
        -e POSTGRES_USER=postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=dark_factory \
        -p 5432:5432 postgres:17-alpine

# Stop and remove the local PostgreSQL container
db-rm:
    docker rm -f dark-factory-pg

# --- Cargo (local, tests need a Docker socket) ---------------------------------------------------

build:
    cargo build --workspace

# Run all tests (or the ones matching FILTER); a shared `df-test-postgres` container is started on demand
test *FILTER:
    cargo test --workspace {{FILTER}}

# Line coverage across the workspace
coverage:
    cargo llvm-cov --workspace

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

# Build everything on the BuildBuddy workers
bazel-build:
    bazel build {{remote}} //...

# Run all tests on the BuildBuddy workers
bazel-test:
    bazel test {{remote}} //...

# Run all tests on this machine; DB tests use the local Docker
bazel-test-local:
    bazel test //...

# Clippy and rustfmt through Bazel (pinned, hermetic)
bazel-lint:
    bazel build {{remote}} --config=lint //crates/... //plugins/...
    bazel build {{remote}} --config=fmt //crates/... //plugins/...

# Proto lint (the buf toolchain only runs on the machine that runs Bazel, so no remote)
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

# --- Release (static musl binaries, no remote execution) ------------------------------------------

# Build the OCI image layouts with the musl toolchain
release-build:
    bazel build --config=release //deploy:control_plane_oci_layout //deploy:runtime_oci_layout //deploy:test_project_oci_layout

# Check that the images are multi-arch and the binaries are static (needs a Linux host, CI runs it)
release-test:
    bazel test --config=release //deploy:images_structure_test

# Push the release images to ghcr.io/prskr/dark-factory (log in with `docker login ghcr.io` first)
release-push TAG="dev":
    bazel run --config=release --//deploy:image_tag={{TAG}} //deploy:push_images
