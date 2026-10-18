# Local Development Guide

This guide covers setting up a local development environment for Dark Factory, running database-backed tests, updating `sqlx` offline query metadata, and running `dark-factory migrate`.

## 1. Prerequisites

- **Rust toolchain**: pinned via [`rust-toolchain.toml`](../rust-toolchain.toml) (`stable`, edition 2024, with targets `x86_64-unknown-linux-musl` and `wasm32-wasip2`, and `llvm-tools-preview`).
- **Protobuf compiler**: `protoc` (`>= 29.x`) on `PATH` (or set via `PROTOC`).
- **Cargo subcommands**:
  - `cargo-deny` (`cargo deny check`)
  - `sqlx-cli` (`cargo sqlx migrate`, `cargo sqlx prepare`)
  - `cargo-llvm-cov` (`cargo llvm-cov --workspace`)
- **Docker / OrbStack**: for running a local PostgreSQL 17 instance with LZ4 support.

## 2. Local PostgreSQL Container

Start a local PostgreSQL 17 container on port `5432`:

```bash
docker run -d \
  --name dark-factory-pg \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=dark_factory \
  -p 5432:5432 \
  postgres:17-alpine
```

Export `DATABASE_URL` for `sqlx` and `#[sqlx::test]`:

```bash
export DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/dark_factory"
```

Run migrations directly with `sqlx-cli`:

```bash
cargo sqlx migrate run --source crates/df-db/migrations
```

## 3. Running Tests and Coverage

With `DATABASE_URL` exported:

```bash
cargo test --workspace
```

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

## Pinned linters (`bazel_env` + `rules_multitool`)

`buildifier`, `cargo-deny`, `helm`, `helm-unittest`, `kingfisher` and `shellcheck` come from Bazel, so the
lefthook hooks use the same versions on every machine.

| Task | Command |
|------|---------|
| Put the tools on `PATH` (once, needs [direnv](https://direnv.net/)) | `bazel run //:bazel_env && direnv allow` |
| Bump tools to the latest upstream releases | `cargo xtask update-tools` |
| Fail if `multitool.lock.json` is stale (CI / scheduled job) | `cargo xtask update-tools --check` |
| Add a tool | add an entry to `TOOLS` in `tools/xtask/src/main.rs` and to `//:bazel_env` |

`multitool.lock.json` is generated; do not edit it by hand. Set `GITHUB_TOKEN` to avoid API rate limits.
