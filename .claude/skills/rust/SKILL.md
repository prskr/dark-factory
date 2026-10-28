---
name: rust
description: Project rules for Rust work in this workspace that go beyond AGENTS.md. Use when adding, removing or updating a Rust dependency, changing Cargo.toml or Cargo.lock, when a Bazel build of a crate fails with unresolved crates, or when running container-backed tests (df_test_postgres) with cargo or Bazel.
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
