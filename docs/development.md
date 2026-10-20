# Local Development Guide

This guide covers setting up a local development environment for Dark Factory, running database-backed tests, updating `sqlx` offline query metadata, and running `dark-factory migrate`.

## 1. Prerequisites

- **Rust toolchain**: pinned via [`rust-toolchain.toml`](../rust-toolchain.toml) (1.99.1, edition 2024, with targets `x86_64-unknown-linux-musl` and `wasm32-wasip2`, and `llvm-tools-preview`).
- **Protobuf compiler**: `protoc` (`>= 29.x`) on `PATH` (or set via `PROTOC`).
- **Cargo subcommands**:
  - `cargo-deny` (`cargo deny check`)
  - `sqlx-cli` (`cargo sqlx migrate`, `cargo sqlx prepare`)
  - `cargo-llvm-cov` (`cargo llvm-cov --workspace`)
- **Docker / OrbStack**: for running a local PostgreSQL 17 instance with LZ4 support.

## 2. Local PostgreSQL Container

Tests do not need this: they start their own PostgreSQL (section 3). The container below is for `sqlx` tooling (`cargo sqlx prepare`, `migrate run`) and running `dark-factory` by hand. Start a local PostgreSQL 17 container on port `5432`:

```bash
docker run -d \
  --name dark-factory-pg \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=dark_factory \
  -p 5432:5432 \
  postgres:17-alpine
```

Export `DATABASE_URL` for the `sqlx` tooling:

```bash
export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/dark_factory"
```

Run migrations directly with `sqlx-cli`:

```bash
cargo sqlx migrate run --source crates/df-db/migrations
```

## 3. Running Tests and Coverage

Database tests need a Docker socket but no `DATABASE_URL`:

```bash
cargo test --workspace
```

The `df-test-postgres` crate (a dev-dependency, never linked into a binary) starts one `postgres:17-alpine` container on first use and creates a database per test: `df_test_postgres::migrated_pool(&df_db::MIGRATOR)`. testcontainers has no reaper and does not drop statics at process exit, so the container is marked reusable: all test binaries on the machine share it and it keeps running after the tests. `migrated_pool` migrates a template database once per migration set and clones it per test; templates (`df_tpl_<hash>`) stay in the container. Test databases older than 15 minutes (the creation time is in the UUIDv7 of the name) are dropped when a test binary starts. The container name carries the image tag and a config revision (see `crates/df-test-postgres/src/lib.rs`); remove old ones with `docker ps -a --filter name=df-test-postgres` and `docker rm -f`.

To measure line coverage across `df-core` and `df-db`:

```bash
cargo llvm-cov --workspace
```

## 4. Updating `sqlx` Offline Metadata

All compile-time checked queries in `df-db` have their metadata checked into `.sqlx/` so the workspace builds without a live database (e.g. in Bazel and CI).

After adding or changing any `sqlx::query!` / `sqlx::query_as!` macro, regenerate `.sqlx/`:

```bash
export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/dark_factory"
cargo sqlx prepare --workspace -- --all-targets
```

Verify that offline compilation succeeds without `DATABASE_URL`:

```bash
SQLX_OFFLINE=true cargo build --workspace
```

## 5. Local Configuration File and Running `dark-factory migrate`

Secrets are referenced via file paths (`*File`), never inline in YAML. To test `dark-factory migrate` locally:

```bash
mkdir -p /tmp/dark-factory-dev
printf "postgres\n" > /tmp/dark-factory-dev/owner-password
printf "app-local-password\n" > /tmp/dark-factory-dev/app-password

cat <<'EOF' > /tmp/dark-factory-dev/config.yaml
instanceName: local-dev
database:
  host: 127.0.0.1
  port: 5432
  database: dark_factory
  user: postgres
  passwordFile: /tmp/dark-factory-dev/owner-password
  schema: dark_factory
  maxConnections: 10
  migrate:
    appRole: dark_factory_app
    manageAppRole: true
    appPasswordFile: /tmp/dark-factory-dev/app-password
EOF

cargo run -p dark-factory -- --config /tmp/dark-factory-dev/config.yaml migrate
```

## Bazel: remote execution and release builds

Day-to-day commands are collected in the [`Justfile`](../Justfile) (`just` lists them; the file also works as a plain command reference).

| Mode | Command | Toolchain | Where it runs |
|------|---------|-----------|---------------|
| Local | `bazel build //...` | host toolchains | this machine |
| Remote (CI, fast feedback) | `bazel test --config=remote //...` | hermetic glibc LLVM (`toolchains_llvm`) | BuildBuddy workers (linux/amd64) |
| Release | `bazel build --config=release //deploy:...` | `toolchains_musl` (static musl) | the machine running Bazel, no remote |

- `--config=cache` is the remote cache and build event stream without remote execution (`remote` includes it); CI combines it with `--config=release`. `--config=remote-debug` additionally writes `bazel-remote-grpc.log` and `execution_log.binpb.zst` to the workspace root (git-ignored).
- `--config=remote` needs a BuildBuddy API key: `build:remote --remote_header=x-buildbuddy-api-key=<key>` in `.bazelrc.user` (CI uses the `BUILDBUDDY_ORG_API_KEY` secret).
- The remote path links against a glibc sysroot, so its binaries are not the static ones that ship. Release artifacts therefore come from `--config=release` (the `release` workflow). Do not combine the two configs.
- `//deploy:images_structure_test` (static ELF check) and `//contract:buf_lint_test` are tagged `manual`: run them explicitly. The former needs a Linux host.
- Database tests start their PostgreSQL through testcontainers (see section 3). On the workers they run in a Firecracker VM with `dockerd` (`test.init-dockerd` in `build/platforms/BUILD.bazel`); locally they use your Docker. Anonymous Docker Hub pulls of `postgres:17-alpine` can hit rate limits, then switch the image in `crates/df-test-postgres/src/lib.rs` to a mirror such as `mirror.gcr.io/library/postgres`.
- Images go to `ghcr.io/prskr/dark-factory/{control-plane,runtime,test-project}`. The tag is `--//deploy:image_tag` (default `dev`); the `release` workflow sets it to the git tag (`v*`) and falls back to `dev` for manual runs from a branch. Locally: `docker login ghcr.io`, then `just release-push v1.2.3`.
- `.bazelrc.agents` makes `--config=remote` the default for sandboxed agents.

## Pinned linters (`bazel_env` + `rules_multitool`)

`buf`, `buildifier`, `cargo`, `cargo-clippy`, `cargo-deny`, `helm`, `helm-unittest`, `kingfisher`, `kube-linter`, `kubeconform`, `lefthook`, `rustc`, `shellcheck`, `jq`, `tailwindcss` and `yq` come from Bazel, so the
lefthook hooks use the same versions on every machine.

| Task | Command |
|------|---------|
| Put the tools on `PATH` (once, needs [direnv](https://direnv.net/)) | `bazel run //:bazel_env && direnv allow` |
| Bump tools to the latest upstream releases | `cargo xtask update-tools` |
| Fail if `multitool.lock.json` is stale (CI / scheduled job) | `cargo xtask update-tools --check` |
| Add a tool | add an entry to `TOOLS` in `tools/xtask/src/main.rs` and to `//:bazel_env` |
| Check Rust formatting (pinned rustfmt, hermetic) | `bazel build --config=fmt //crates/... //plugins/...` |
| Format protos / lint protos | `bazel run //contract:format` / `bazel test //contract:buf_lint_test` |
| Install git hooks without direnv | `LEFTHOOK_BIN=$PWD/.bazel_env/bin/lefthook .bazel_env/bin/lefthook install` (git hooks use `LEFTHOOK_BIN`) |

`multitool.lock.json` is generated; do not edit it by hand. Set `GITHUB_TOKEN` to avoid API rate limits.

## Dependency freshness

New dependencies start on their latest release. `cargo xtask outdated` fails when a requirement in the
root `Cargo.toml` (`[workspace.dependencies]`) or in a member crate excludes the newest stable, non-yanked
crates.io release (for example `tonic = "0.12"` while 0.14 is out). Compatible updates within a requirement
are `cargo update`'s job and are not reported.

| Task | Command |
|------|---------|
| Check all dependencies | `cargo xtask outdated` |
| Check only what is new or changed since a revision (the pre-commit hook) | `cargo xtask outdated --since HEAD` |
| Hold a dependency back on purpose | add `crate-name = "reason"` under `[workspace.metadata.outdated.ignore]` in `Cargo.toml` |

The pre-commit hook `outdated-deps` runs when a `Cargo.toml` is staged. If crates.io cannot be reached it
prints a warning and passes. When you bump the Rust version, change `rust-toolchain.toml` and `versions` in
`MODULE.bazel` together.
