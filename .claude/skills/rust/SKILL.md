---
name: rust
description: Project rules for Rust work in this workspace that go beyond AGENTS.md. Use when adding, removing or updating a Rust dependency, changing Cargo.toml or Cargo.lock, when a Bazel build of a crate fails with unresolved crates, or when running container-backed tests (df_test_postgres) with cargo or Bazel, when adding a crate or a Bazel test target, or when changing lints.
---

# Rust in Dark Factory

Rules learned from real failures in this repository. `AGENTS.md` stays the base; this skill adds the details.

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

Find the container socket first, as described in `AGENTS.md` ("Containers and integration tests in a sandbox"). `<socket>` below is the one that answered the probe, for example `$XDG_RUNTIME_DIR/podman/podman.sock`.

- **Container-backed tests under Bazel** (`df_db_test`, `df_web_test`, ...): the linux-sandbox hides the socket. Pass it in:

  ```bash
  bazel --bazelrc=.bazelrc.agents test <targets> \
    --test_env=DOCKER_HOST=unix://<socket> \
    --sandbox_writable_path=$(dirname <socket>)
  ```

- **Tests hang after the container runtime restarted:** the shared PostgreSQL container (`df-test-postgres-*`, reused by all test binaries) stays exited. With the CLI of the runtime that owns `<socket>` (`podman` or `docker`), check with `<cli> ps -a --filter name=df-test-postgres` and remove it with `<cli> rm -f <name>`; testcontainers recreates it on the next run.
- **`dark-factory` tests with plain cargo** load the Forgejo wasm plugin from `target/wasm32-wasip2/debug/`. Build it first: `cargo build --target wasm32-wasip2 -p forgejo-plugin`. Bazel (CI, release) builds it as test `data` and sets `FORGEJO_PLUGIN_WASM`; no extra step there.

## Adding a crate (Bazel details)

The checklist is in `AGENTS.md` ("Adding a crate"). Details that bite:

- Bazel runs only what has a target. A `tests/*.rs` or a unit-test module without `rust_test` is never built, run or linted under Bazel.
- A test that spawns a binary gets it through `data` and the path through `env` (`$(rootpath :bin)`), or through `rustc_env` when the test reads it with `env!` (`CARGO_BIN_EXE_<name>`).
- Do not tag a target `manual`: `//crates/...` and the clippy aspect skip it, so it would escape linting. Tag a test that cannot run in Bazel `external` instead.
- A new workspace member needs the manifest entry in `MODULE.bazel`; then regenerate `MODULE.bazel.lock` as described above.

## Lints

- One source: `[workspace.lints]` in the root `Cargo.toml` and `clippy.toml`. `extract_cargo_lints` turns the manifest tables into `lint_config`, which rustc and the clippy aspect (`--config=lint`) apply. `.bazelrc` keeps only `-Dwarnings`, because rules_rust drops its own once `clippy_flags` is set.
- To add a lint, edit the workspace table (and the mirrored tables of the plugins and `tools/xtask`; `cargo xtask check-crates` compares them). Nothing else. Enable a lint only once the tree is clean under it.
- Lint options live in `clippy.toml`: the test exemptions (`allow-unwrap-in-tests`, `allow-panic-in-tests`, `allow-indexing-slicing-in-tests`, `allow-dbg-in-tests`, `allow-print-in-tests`), `excessive-nesting-threshold` and `disallowed-methods` (`std::env::temp_dir`).
- `tools/xtask` is a standalone workspace outside Bazel; its clippy run is in the pre-push hook and CI. Bazel's `//:cargo` cannot compile xtask (no host std), so CI runs its clippy and checks with the rustup toolchain.
- Silence a lint with `#[expect(..., reason = "...")]`, never `#[allow]` (`clippy::allow_attributes` fails the build). Generated code (`df-proto`, `crd.rs` from kopium, `wit_bindgen::generate!`) expects the lints it triggers. `crd.rs` is kopium output kept verbatim; its lints are expected on `mod crd` in `df-k8s/src/lib.rs`, so regenerating needs no manual edit.
- A macro that expands to `#[allow]` (seen with `sqlx::query!` and a `&bool` as the last bind argument) trips `allow_attributes`; dereference or wrap the argument.
