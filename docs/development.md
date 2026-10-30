# Local Development Guide

This guide covers setting up a local development environment for Dark Factory, running database-backed tests, updating `sqlx` offline query metadata, and running `dark-factory migrate`.

## 1. Prerequisites

- **Rust toolchain**: pinned via [`rust-toolchain.toml`](../rust-toolchain.toml) (1.99.1, edition 2024, with targets `x86_64-unknown-linux-musl` and `wasm32-wasip2`, and `llvm-tools-preview`).
- **Protobuf compiler**: `protoc` (`>= 29.x`) on `PATH` (or set via `PROTOC`).
- **Cargo subcommands**:
  - `cargo-deny` (`cargo deny check`)
  - `sqlx-cli` (`cargo sqlx migrate`, `cargo sqlx prepare`)
  - `cargo-llvm-cov` (`cargo llvm-cov --workspace`)
- **Docker / OrbStack or Podman** with compose (`docker compose`, or `podman compose` with `podman-compose`): for the local services in [`compose.yml`](../compose.yml).
- **OpenSSL**: `just serve-local` generates a throwaway agent certificate with it.

## 2. Local Services (PostgreSQL and Dex)

Tests do not need these: they start their own PostgreSQL (section 3). [`compose.yml`](../compose.yml) runs the services for `sqlx` tooling (`cargo sqlx prepare`, `migrate run`) and running `dark-factory` by hand:

| Service | Address | Purpose |
| --- | --- | --- |
| `postgres` | `127.0.0.1:5432` | PostgreSQL 17, user and password `postgres`, database `dark_factory` |
| `dex` | `http://127.0.0.1:5556/dex` | OIDC provider for the web UI, static users from [`dev/dex.yaml`](../dev/dex.yaml) |

Earlier versions of this guide started PostgreSQL as a standalone container named `dark-factory-pg`. It still holds port `5432` and blocks `just db`; `just db-rm` does not remove it. Remove it first (export data you want to keep with `docker exec dark-factory-pg pg_dump -U postgres dark_factory > dump.sql`):

```bash
docker rm -f dark-factory-pg
```

`just db` starts both and waits up to 60 s for PostgreSQL (with `docker`, or `podman` when Docker is not installed; `CONTAINER_CLI` overrides). `just db-rm` removes them and their data. Without `just`:

```bash
docker compose up -d
docker compose down -v
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

`just test [FILTER]` does the same and also builds the plugin components first (see the end of this section). It sets `DOCKER_HOST` to the rootless Podman socket (`$XDG_RUNTIME_DIR/podman/podman.sock`) when that socket exists and `DOCKER_HOST` is unset; `just bazel-test [TARGETS]` passes the same socket into the Bazel sandbox (`--test_env=DOCKER_HOST=…`, `--sandbox_writable_path=…`). `just test-db-rm` removes the shared `df-test-postgres` containers.

The `df-test-postgres` crate (a dev-dependency, never linked into a binary) starts one `postgres:17-alpine` container on first use and creates a database per test: `df_test_postgres::migrated_pool(&df_db::MIGRATOR)`. testcontainers has no reaper and does not drop statics at process exit, so the container is marked reusable: all test binaries on the machine share it and it keeps running after the tests. `migrated_pool` migrates a template database once per migration set and clones it per test; templates (`df_tpl_<hash>`) stay in the container. Test databases older than 15 minutes (the creation time is in the UUIDv7 of the name) are dropped when a test binary starts. The container name carries the image tag and a config revision (see `crates/df-test-postgres/src/lib.rs`); remove old ones with `docker ps -a --filter name=df-test-postgres` and `docker rm -f`.

To measure line coverage across `df-core` and `df-db`:

```bash
cargo llvm-cov --workspace
```

Tests of `df-plugins` and `dark-factory` use prebuilt WebAssembly plugin components and do not build them. Build them first (Bazel sets `FORGEJO_PLUGIN_WASM` / `GITHUB_PLUGIN_WASM` / `TEST_PROBE_WASM` itself):

```bash
cargo build --target wasm32-wasip2 -p forgejo-plugin -p test-probe-plugin -p github-plugin
```

With direnv, `cargo` and `rustc` on `PATH` are the Bazel-managed wrappers from `.bazel_env/bin`, which only have the host's standard library. Build the plugins with rustup's toolchain instead, as `just plugins` does: `RUSTC="$(rustup which rustc)" "$(rustup which cargo)" build --target wasm32-wasip2 -p forgejo-plugin -p test-probe-plugin -p github-plugin`.

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

### Existing dev database after the harness-images change

The initial migration `20261017000001_initial_schema.sql` was edited in place: `personas.adapter_command` is gone and `projects.adapter_command` is new. `sqlx` stores a checksum per applied migration, so `migrate` on an existing database fails naming `20261017000001`. Either reset the database (`docker compose down -v && docker compose up -d`) or change it once by hand (schema `dark_factory`; adjust it to `database.schema`):

```bash
sum="$(sha384sum crates/df-db/migrations/20261017000001_initial_schema.sql | cut -d' ' -f1)"
docker compose exec -T postgres psql -U postgres dark_factory <<EOF
ALTER TABLE dark_factory.personas DROP COLUMN adapter_command;
ALTER TABLE dark_factory.projects ADD COLUMN adapter_command TEXT[] NOT NULL DEFAULT '{}'::TEXT[];
UPDATE dark_factory._sqlx_migrations SET checksum = decode('$sum', 'hex') WHERE version = 20261017000001;
EOF
```

The checksum is the SHA-384 of the migration file; run the `UPDATE` again after every further edit of that file. Set the adapter command on projects whose image brings its own adapter afterwards (project form, "Adapter command").

## 6. Running the Web UI Locally

The end-to-end suite and the demo data (`just seed`) are described in [`demo.md`](demo.md).

`just serve-local` runs the control plane against the compose services without a Kubernetes cluster:

1. starts the compose services (`just db`) and builds the plugin components (`just plugins`),
2. writes the password files, the OIDC client secret and a self-signed agent certificate to `target/local-dev/`,
3. runs `migrate` and `serve --no-cluster --demo-flows` with [`dev/config.yaml`](../dev/config.yaml), built with the Cargo feature `dev-tools`.

Open <http://127.0.0.1:8080> and log in at Dex:

| User | Password | Result |
| --- | --- | --- |
| `admin@example.com` | `password` | Member of the group `board-member`: full UI |
| `viewer@example.com` | `password` | No role: `403` page |

The flags `--no-cluster` and `--demo-flows` exist only in a build with the Cargo feature `dev-tools` (`cargo run -p dark-factory --features dev-tools -- serve --no-cluster`). A default or release build and the Bazel images have neither; `cargo clippy --all-targets --all-features` keeps the gated code linted. Limits of `serve --no-cluster` (local development only):

- Every sandbox operation fails, so starting a run fails. Tasks, projects, personas, credentials and `host` allowlist entries can be created and edited.
- Egress allowlist: adding a `cluster-service` entry fails ("Services cannot be checked"), because there is no Kubernetes API to look the Service up.
- Network traffic (`/egress/log`) answers `503 Network traffic unavailable` without `--demo-flows`: without `gateway.headlessService` and gateway pods there is nothing to stream. `--demo-flows` replays scripted events (`--demo-flows=script`, the default: allowed, blocked and open connections, a gap, three runs, one new event every 3 s) or `--demo-flows=bulk` (10,000 events, to see how the page copes with a full buffer). It needs `--no-cluster`.
- Project host icons: the script connects to `intranet.example.com` and `ghcr.io`. Create a credential reference, then a project with repository `https://intranet.example.com/org/df.git`, issue tracker `https://ghcr.io/org/df/issues` and that credential, and reload `/egress/log` (the lookup is built per connection) and `/egress/allowlist`: `intranet.example.com` shows the Forgejo icon and `ghcr.io` the issue icon instead of the globe. Without the credential it shows the generic Git icon.
- The agent port (`9443`) and the internal port (`9444`, `PolicyWatch` for the gateway) use the throwaway certificate; no sandbox or gateway connects.

Dex cannot add custom claims, so the local configuration reads the role from the `groups` claim: `auth.rolesClaim: groups` and `auth.extraScopes: [groups, offline_access]` (`offline_access` returns a refresh token). Because `auth.publicUrl` is `http://` on loopback, the session cookie is sent without `Secure`. Plain `http://` is accepted on any host (TLS is the operator's choice, for example the throwaway e2e cluster); on a non-loopback host the control plane logs a startup warning. With an `https://` URL the cookie always carries `Secure`.

```mermaid
sequenceDiagram
    participant B as Browser
    participant C as dark-factory :8080
    participant D as Dex :5556
    participant P as PostgreSQL :5432
    B->>C: GET /tasks
    C-->>B: 303 /login?next=/tasks
    B->>C: GET /login?next=/tasks
    C->>D: GET /.well-known/openid-configuration
    C-->>B: 303 Dex /auth (scope openid profile groups offline_access)
    B->>D: log in (admin@example.com)
    D-->>B: 302 /auth/callback?code
    B->>C: GET /auth/callback
    C->>D: POST /token
    D-->>C: ID token (groups: [board-member])
    C->>P: store session
    C-->>B: 303 /tasks + df_session cookie
```

## Bazel: platforms, cache and images

Day-to-day commands are collected in the [`Justfile`](../Justfile) (`just` lists them; the file also works as a plain command reference).

Everything builds on the machine that runs Bazel; there is no remote execution (background and how to bring it back: [remote-execution.md](remote-execution.md)). The same commands work on macOS (arm64) and Linux (amd64, arm64).

| Host | Target platform of `bazel build/test //...` | Images (`//deploy:*_image`) |
|------|---------------------------------------------|------------------------------|
| Linux amd64/arm64 | `//build/platforms:host_musl`: static musl for the host's cpu (`build:linux` in `.bazelrc`) | amd64 + arm64 static musl |
| Linux with `--config=linux-<arch>-musl` (CI) | `//build/platforms:linux_<arch>_musl` | amd64 + arm64 static musl, the host arch reuses the test build |
| macOS arm64 | native macOS | amd64 + arm64 static musl, cross-compiled |

```mermaid
flowchart LR
  src[crates/*] -->|"--platforms (host_musl or linux_&lt;arch&gt;_musl)"| tests[tests, static musl]
  src -->|image_index transition| amd64[linux_amd64_musl]
  src -->|image_index transition| arm64[linux_arm64_musl]
  amd64 --> img[OCI index]
  arm64 --> img
```

The images transition to `linux_amd64_musl` and `linux_arm64_musl`. `host_musl` is a different platform label and therefore a separate configuration: on a Linux host the host arch is compiled twice (tests and image). Setting `--config=linux-amd64-musl` (or `linux-arm64-musl`) for the host's arch makes the test build the image's configuration; CI does this. On a Linux arm64 machine, add `build --config=linux-arm64-musl` to `.bazelrc.user` for the same effect.

- Toolchains: Rust musl toolchains and `toolchains_musl` (`musl-gcc`) for Linux targets, registered first in `MODULE.bazel`. Exec tools (build scripts, proc-macros) are built for the host with the host's C compiler (Xcode clang, or `gcc`/`clang` on Linux), which must be installed.
- Tests only run for the host's arch; the other arch is cross-compiled into the images but not executed.
- `--config=cache` (optional) enables the BuildBuddy remote cache and build event stream. It needs an API key: export `BUILDBUDDY_API_KEY=<key>` (read by the credential helper `build/buildbuddy_credentials.sh`) or put `build:cache --remote_header=x-buildbuddy-api-key=<key>` in `.bazelrc.user` (CI uses the `BUILDBUDDY_ORG_API_KEY` secret). The variable is read when the Bazel client starts; a cached key is kept for `--credential_helper_cache_duration` (default 30 min). `--config=cache-debug` additionally writes `bazel-remote-grpc.log` and `execution_log.binpb.zst` to the workspace root (git-ignored).
- Cache hits are shared between machines with the same host OS and arch (exec tools differ otherwise).
- The binaries and the plugins in the images are built with `--compilation_mode=opt` whatever the command line says (`optimized` in `build/optimized.bzl`): a `fastbuild` control plane is ~224 MB with debug info, an optimized one ~84 MB. Symbols stay in, so panic backtraces keep function names.
- `//deploy:images_structure_test` (multi-arch index, static ELF check, no debug info in the binaries) only runs on Linux hosts and is skipped on macOS (`//...`; when named explicitly, add `--skip_incompatible_explicit_targets`, as `just release-test` does).
- Database tests start their PostgreSQL through testcontainers (see section 3) and need a local Docker socket. For Podman, point the tests at its socket, for example `build --test_env=DOCKER_HOST=unix:///run/user/501/podman/podman.sock` and `test --sandbox_writable_path=/run/user/501/podman` in `.bazelrc.user` (use your uid instead of `501`); without the writable path the sandbox cannot reach the socket. Anonymous Docker Hub pulls of `postgres:17-alpine` can hit rate limits, then switch the image in `crates/df-test-postgres/src/lib.rs` to a mirror such as `mirror.gcr.io/library/postgres`.
- Base images (`pull` in `MODULE.bazel`) are pinned by digest and fetched with Bazel's downloader, so their manifests and configs land in the repository cache and are not requested again; CI keeps that cache between runs (`repository-cache` of `setup-bazel`). `platforms` limits the fetch to `linux/amd64` and `linux/arm64`, because a registry counts every child manifest as a pull. The Bazel downloader cannot do the anonymous token handshake of `public.ecr.aws` (401); Docker Hub images therefore come from `mirror.gcr.io`, which serves them under the same digests.
- Images go to `ghcr.io/prskr/dark-factory/{control-plane,plugin-forgejo,plugin-github,runtime,test-project}`. The tag is `--//deploy:image_tag` (default `dev`); the `release` workflow sets it to the git tag (`v*`) and falls back to `dev` for manual runs from a branch. Locally: `docker login ghcr.io`, then `just release-push v1.2.3`.
- The plugin images (`//deploy:plugin_forgejo_image`, `//deploy:plugin_github_image`: `scratch`, one file `/plugin.wasm`) are mounted by the chart as image volumes at `/plugins/<name>`; the control plane image contains no plugin. `bazel run //deploy:dev` pushes them as `plugin-forgejo:dev` and `plugin-github:dev` and passes `plugins.forgejo.image.digest` and `plugins.github.image.digest` to Helm, so a plugin change replaces the control plane pods.
- `.bazelrc.agents` makes `--config=cache` the default for sandboxed agents.

## Pinned linters (`bazel_env` + `rules_multitool`)

`buf`, `buildifier`, `cargo`, `cargo-clippy`, `cargo-deny`, `helm`, `helm-unittest`, `kind`, `kingfisher`, `kube-linter`, `kubeconform`, `lefthook`, `rustc`, `shellcheck`, `jq`, `tailwindcss` and `yq` come from Bazel, so the
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

### Renovate

Renovate (`renovate.json`) opens the update PRs. It runs self-hosted in `.github/workflows/renovate.yaml`
(daily at 03:17 UTC, or "Run workflow" for a manual run) through `npx` on the runner, not in the official
action's container: the post-upgrade task `bazel mod tidy` needs Bazel and keeps `MODULE.bazel.lock` in
step with Bazel module, Cargo and pnpm updates. The harness adapter is bumped in one PR together with
every copy of its image tag (`deploy/README.md`, "What the harness image is built from"). The run time
falls into the window of the weekly lock file maintenance (before 04:00 UTC on Monday); Rust toolchain
updates change `rust-toolchain.toml` and `RUST_VERSION` in `MODULE.bazel` in one PR.

| Setting | Required | Default | Effect |
|---|---|---|---|
| secret `RENOVATE_TOKEN` | yes | none | Fine-grained PAT with read and write access to contents, pull requests, issues and workflows; renew it before it expires. A GitHub App installation token does not work (it expires after one hour), and neither does `GITHUB_TOKEN` (pull requests it opens trigger no CI runs). |
| `RENOVATE_VERSION`, `PNPM_VERSION` (workflow `env`) | yes | pinned | Renovate and pnpm versions on the runner; Renovate bumps them itself (`customManagers:githubActionsVersions`). |
| `RENOVATE_ALLOWED_COMMANDS` (workflow `env`) | yes | `["^bazel mod tidy$"]` | Commands `postUpgradeTasks` may run; extend it together with `renovate.json`. |
