---
name: rust
description: Project rules for Rust work in this workspace that go beyond AGENTS.md. Use when cargo or rustc is missing (sandbox toolchain setup) or a target such as wasm32-wasip2 seems missing, when running Rust tests (choosing cargo or Bazel, filters, no container socket, disk full), when running or writing end-to-end tests (just e2e-local, crates/df-e2e), when adding, removing or updating a Rust dependency, changing Cargo.toml or Cargo.lock, when a Bazel build of a crate fails with unresolved crates, or when running container-backed tests (df_test_postgres) with cargo or Bazel, when adding a crate or a Bazel test target, or when changing lints or a file over the length limit.
---

# Rust in Dark Factory

Rules learned from real failures in this repository. `AGENTS.md` stays the base; this skill adds the details.

## Toolchain in a sandbox

Sandboxes have no Rust toolchain, but `rustup` is available. Before running anything else (`cargo`, `rustc`, `rust-analyzer`, Bazel targets that call `cargo`), install the toolchain pinned in `rust-toolchain.toml`:

```bash
# from the repository root: rustup reads rust-toolchain.toml (channel, components, targets)
rustup show active-toolchain || rustup toolchain install
rustc --version   # must print the channel from rust-toolchain.toml
cargo clippy --version && cargo fmt --version
```

- Run these from inside the repository, otherwise `rustup` picks its default toolchain.
- `rust-toolchain.toml` is the source of truth. The same version is repeated in `MODULE.bazel` (`versions`); bump both together.
- A missing component or target: `rustup component add <name>` / `rustup target add <triple>` with the names from the file; don't change the file.
- A plain `cargo build` also needs `protoc` (>= 29.x) on `PATH` (or `PROTOC`). Without it, build and test through Bazel.
- `.bazel_env/bin` (on `PATH` through direnv) contains Bazel's own `cargo` and `rustc`, which come before rustup's and know no extra targets: `wasm32-wasip2` looks missing. Don't take `.bazel_env/bin` off `PATH` (it also provides `swc`, `tailwindcss`, `kind`, `buf`); call rustup's toolchain explicitly for target builds: `rustup run "$(rustup show active-toolchain | cut -d' ' -f1)" cargo build --target wasm32-wasip2 -p forgejo-plugin`.
- `df-web` compiles its scripts (`swc`) and stylesheet (`tailwindcss`) in `build.rs`. Without the tools (`bazel run //:bazel_env` builds them into `.bazel_env/bin`, which must be on `PATH`; in a sandbox they are usually missing) the build succeeds with a warning, nothing is embedded, and `cargo test -p df-web` lists the tests that read compiled assets as `ignored` (cfgs `df_web_scripts` / `df_web_styles`). Say in the report that they did not run; `bazel test //crates/df-web/...` always runs them.

## Adding a dependency

The order of preference (std, existing dependency, hand-rolled) is in `AGENTS.md`. When you add one:

- Declare it in the root `[workspace.dependencies]` and reference it with `.workspace = true`. Standalone workspaces (`tools/xtask`, `plugins/*` with their own lock) declare it in their own manifest.
- Start on the latest release. `cargo xtask outdated` (pre-commit hook for changed dependencies) fails when a requirement excludes the newest stable release. Holding one back needs an entry with a reason under `[workspace.metadata.outdated.ignore]` in the root `Cargo.toml`.
- Enable only the features that are needed (`default-features = false` where sensible); check what a feature pulls in (`chrono`'s `clock` adds about 25 crates, `now` none).
- Remove dependencies that are no longer used; `cargo-machete` runs in the pre-commit hook. Don't silence it with `ignored = [...]`.
- `cargo deny check` must pass (licenses, advisories).
- Commit `Cargo.lock` and the Bazel lock file with the change (next section).

## Dependencies and `MODULE.bazel.lock`

`MODULE.bazel.lock` holds more than input hashes: it stores the whole crate_universe resolution, including the generated `defs.bzl` that `all_crate_deps(...)` in every `BUILD.bazel` reads. A dependency added to a crate's `Cargo.toml` reaches the Bazel build only through that resolution. CI and the release pipeline build only with Bazel, so a stale lock file breaks them even when `cargo build` passes.

- Never edit `MODULE.bazel.lock` by hand. If only the `Cargo.toml`/`Cargo.lock` hashes are updated, Bazel accepts the stale resolution, `--lockfile_mode=error` passes, and the crate compiles without the new dependency (`cannot find module or crate`, follow-on errors such as "trait is not dyn compatible" for `async_trait`).
- After any change to `Cargo.toml` or `Cargo.lock`, regenerate the lock file:

  ```bash
  git checkout main -- MODULE.bazel.lock   # only if it was edited by hand
  bazel --bazelrc=.bazelrc.agents build --lockfile_mode=update //crates/<crate>
  ```

- Then verify with a real build, not only a lock check:

  ```bash
  bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error //crates/... //plugins/...
  ```

  If the Bazel server dies (`Server terminated abruptly`), retry with `--jobs=4`.
- Commit `Cargo.lock` and `MODULE.bazel.lock` together with the change.

## Tests

### Feedback loop: cargo with a filter, Bazel once at the end

- **While red/green, run only the test you work on:** `cargo test -p <crate> <test_name>` or `cargo test -p <crate> --test <file> <test_name>`. Cargo compiles the edited crate incrementally; Bazel (rules_rust) recompiles the whole crate as static musl on every change and reruns the whole test target.
- **Before committing,** run all touched crates in one command: `cargo test -p <a> -p <b> …` (or `cargo test --workspace`). Never loop `cargo test -p $c` over crates: each invocation resolves features anew and relinks shared dependencies, so a loop spends ~10 minutes compiling for ~2 minutes of tests.
- Use the same feature flags for clippy and tests (`cargo clippy --all-targets --all-features` and `cargo test --all-features`, or neither): different features invalidate the build and the next command recompiles most of the workspace.
- **Bazel only when the Bazel inputs changed:** a `BUILD.bazel`, `MODULE.bazel`, a `Cargo.toml` or `Cargo.lock`, or `data`/`rustc_env` a test reads. Then run the affected targets once (`bazel --bazelrc=.bazelrc.agents test //crates/<crate>/...`). Otherwise the cargo run is enough; CI runs Bazel. Cargo-only crates (`[workspace.metadata.check-crates.exempt]`, such as `crates/df-e2e`) have no Bazel targets.
- Don't alternate cargo and Bazel runs during the loop: they share no build outputs, so each switch compiles everything twice.
- `.bazelrc.agents` uses no remote cache (it was unreachable from sandboxes). Don't add `--config=cache`.
- **Long commands:** start one in the background and wait for its completion notification. Don't stack wait loops, monitors or `sleep` polls; they wake you for nothing and blocked sleeps fail.
- `--test_filter` narrows a Bazel run, but each new filter value is a new cache key and reruns the target.

### Container-backed tests

Find the container socket first, as described in `AGENTS.md` ("Containers and integration tests in a sandbox"). `<socket>` below is the one that answered the probe, for example `$XDG_RUNTIME_DIR/podman/podman.sock`.

- **No socket:** run only tests you have identified as needing no containers, not broad targets such as `bazel --bazelrc=.bazelrc.agents test //crates/<crate>/...` or `cargo test -p <crate> --lib`, which may include container-backed tests (anything using `df_test_postgres` or testcontainers; find them with `grep -rl "df_test_postgres\|testcontainers" crates/<crate>`). Select explicitly: `cargo test -p <crate> <filter>` / `--test <name>`, `cargo test -p <crate> -- --skip <container test>`, or specific Bazel test targets. Report which container-backed tests you could not run.

- **Container-backed tests under Bazel** (`df_db_test`, `df_web_test`, ...): the linux-sandbox hides the socket. Pass it in:

  ```bash
  bazel --bazelrc=.bazelrc.agents test <targets> \
    --test_env=DOCKER_HOST=unix://<socket> \
    --sandbox_writable_path=$(dirname <socket>)
  ```

- **Tests hang after the container runtime restarted:** the shared PostgreSQL container (`df-test-postgres-*`, reused by all test binaries) stays exited. With the CLI of the runtime that owns `<socket>` (`podman` or `docker`), check with `<cli> ps -a --filter name=df-test-postgres` and remove it with `<cli> rm -f <name>`; testcontainers recreates it on the next run.
- **Disk full (`no space left on device`):** the shared container `df-test-postgres-17-alpine-r1` keeps every test database (TD-123) and grows to 15+ GB. Check with `<cli> system df` / `df -h /var/lib/docker`, then remove it with its volume: `<cli> rm -fv df-test-postgres-17-alpine-r1`. Its data is throwaway; the next test run recreates it. Do this before an e2e run, which needs several GB for images.
- **`dark-factory` tests with plain cargo** load the Forgejo wasm plugin from `target/wasm32-wasip2/debug/`. Build it first with rustup's toolchain (see "Toolchain in a sandbox"): `rustup run "$(rustup show active-toolchain | cut -d' ' -f1)" cargo build --target wasm32-wasip2 -p forgejo-plugin`. Bazel (CI, release) builds it as test `data` and sets `FORGEJO_PLUGIN_WASM`; no extra step there.

### End-to-end suite (`just e2e-local`)

`deploy/e2e/e2e-local.sh` creates a kind cluster (~2-4 minutes), runs `crates/df-e2e` against it and deletes it. A full run takes ~15 minutes; `flow.rs` alone ~10 minutes. Needs a container socket and free disk (previous section).

- **While working on one e2e test, run only it:** `E2E_TEST_ARGS="--ignored --test-threads=1 <test_name_filter>" just e2e-local`. The setup still runs, but the other tests are skipped (~12 minutes saved per run). `E2E_TEST_ARGS` replaces the default arguments, so keep `--test-threads=1`: a filter can match several tests, and the cluster's memory budget (`docs/demo.md`) is one test at a time.
- **Run the full suite once,** at the end, when the targeted test passes.
- **A new test file** under `crates/df-e2e/tests/` must be added to `suite_tests` in `deploy/e2e/e2e-local.sh`; otherwise it is built but never run, and the suite still passes.
- `E2E_KEEP=1` keeps the cluster after the run for inspection (`kubectl --context kind-df-e2e …`); the next run still recreates it.
- Forgejo refuses to merge a draft PR (HTTP 405): a test that merges an attached or draft PR marks it ready through the test's own Forgejo client first. It also answers 405 while it re-checks a PR after its base branch moved (another test merged); `Forgejo::merge_pull` retries that.

## Adding a crate

Run `cargo xtask check-crates` when done (pre-commit hook and CI run it); it checks every item:

1. Directory `crates/<name>` with a `Cargo.toml` that has `[lints]` with `workspace = true`. A plugin (`plugins/<name>`) instead copies `[lints.rust]` and `[lints.clippy]` of the workspace into its own `Cargo.toml`, without `workspace = true` (Cargo rejects both in one manifest); `unsafe_code` only has to be present, at any level.
2. `BUILD.bazel` with `extract_cargo_lints(name = "lints", manifest = "Cargo.toml", workspace = "//:Cargo.toml")` and `lint_config = ":lints"` on every `rust_library`, `rust_binary`, `rust_test` and `rust_shared_library`.
3. One `rust_test` per `tests/*.rs`, plus one for the unit tests. Never tag a test `manual`: the lint run skips manual targets.
4. `"//crates/<name>:Cargo.toml"` (or `"//plugins/<name>:Cargo.toml"`) in the `crate.from_cargo` manifests of `MODULE.bazel`, then regenerate `MODULE.bazel.lock` (above).

A crate that only `cargo` builds (`crates/df-e2e`) skips items 2 to 4 if it has an entry with a reason in `[workspace.metadata.check-crates.exempt]` of the root `Cargo.toml`. It must not have a `BUILD.bazel`.

Details that bite:

- Bazel runs only what has a target. A `tests/*.rs` or a unit-test module without `rust_test` is never built, run or linted under Bazel.
- A test that spawns a binary gets it through `data` and the path through `env` (`$(rootpath :bin)`), or through `rustc_env` when the test reads it with `env!` (`CARGO_BIN_EXE_<name>`).
- Do not tag a target `manual`: `//crates/...` and the clippy aspect skip it, so it would escape linting. Tag a test that cannot run in Bazel `external` instead.
- A new workspace member needs the manifest entry in `MODULE.bazel`; then regenerate `MODULE.bazel.lock` as described above.

## Lints

- One source: `[workspace.lints]` in the root `Cargo.toml` and `clippy.toml` (thresholds such as `too-many-lines-threshold`, `excessive-nesting-threshold`). Plugins repeat both tables in their own `[lints]` (`unsafe_code` may have any level there); `tools/xtask` mirrors both.
- Lint levels are declared only there and in `clippy.toml`. `extract_cargo_lints` turns the manifest tables into `lint_config`, which rustc and the clippy aspect (`--config=lint`) apply. `.bazelrc` keeps only `-Dwarnings`, because rules_rust drops its own once `clippy_flags` is set.
- To add a lint, edit the workspace table (and the mirrored tables of the plugins and `tools/xtask`; `cargo xtask check-crates` compares them). Nothing else. Enable a lint only once the tree is clean under it.
- Lint options live in `clippy.toml`: the test exemptions (`allow-unwrap-in-tests`, `allow-panic-in-tests`, `allow-indexing-slicing-in-tests`, `allow-dbg-in-tests`, `allow-print-in-tests`), `excessive-nesting-threshold` and `disallowed-methods` (`std::env::temp_dir`).
- `tools/xtask` is a standalone workspace outside Bazel; its clippy run is in the pre-push hook and CI. Bazel's `//:cargo` cannot compile xtask (no host std), so CI runs its clippy and checks with the rustup toolchain.
- Silence a lint with `#[expect(..., reason = "...")]`, never `#[allow]` (`clippy::allow_attributes` fails the build). Generated code (`df-proto`, `wit_bindgen::generate!`) expects the lints it triggers.
- `clippy::pedantic` is enabled. The workspace allows `must_use_candidate`, `missing_errors_doc`, `missing_panics_doc`, `assert_is_empty`, `needless_pass_by_value`, `similar_names` and `struct_excessive_bools`, each with a reason in the root `Cargo.toml`.
- File length: `cargo xtask check-file-length` caps source files (`rs`, `proto`, `wit`, `bzl`, `bazel`, `sh`, `js`, `ts`, `css`, `html`, `toml`, `yaml`, `yml`) at 1,500 lines. A file over the limit needs an entry with a reason (generated code, or a tracked split) in `[workspace.metadata.file-length.allow]` of the root `Cargo.toml`; an entry for a missing file, for a file within the limit, or without a reason fails too.
- A macro that expands to `#[allow]` (seen with `sqlx::query!` and a `&bool` as the last bind argument) trips `allow_attributes`; dereference or wrap the argument.
