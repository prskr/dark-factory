# Proposal

## Why

`deploy/e2e/e2e-local.sh` (681 lines, 44 functions, run by `just e2e-local` and `.github/workflows/e2e.yaml`) mixes two kinds of code. Calling `kind`, `helm`, `kubectl`, the container runtime and Bazel in order suits shell. The Forgejo setup, the kept-cluster token state and the Playwright version check do not:

- **Forgejo setup** builds JSON for `curl` by string interpolation and parses the users' tokens out of `forgejo admin user create` output with `sed` through `kubectl exec`. Repository names and the webhook secret repeat `crates/df-e2e/src/data.rs` (`FORGE_REPO`, `FORGE_EXISTING_BRANCH_REPO`, `WEBHOOK_SECRET`).
- **Kept-cluster state** stores the two tokens in the Secret `e2e-state` and restores them on reuse (`state_value`, `restore_forgejo_users`). The tokens exist only because the setup cannot create them again.
- **Version checks** read Playwright versions from `build.rs`, `MODULE.bazel` and `package.json` with `sed` regexes.
- Nothing tests any of it, and it has to run on bash 3.2 (macOS) and bash 5 (Linux); TD-94 was found by hand.

Moving this logic into the `df-e2e` crate puts it next to the constants it repeats and under `cargo test`.

## What Changes

- **New binary `df-e2e --bin setup`** (next to `--bin seed`), clap subcommands:
  - `forgejo`: through the Forgejo API, no `kubectl exec`: creates the bot (`factory`) and the reviewer if missing (with the chart's admin user, basic auth) and replaces each user's token named `e2e`; deletes (404 is fine) and creates `demo` and `existing-branch` with the issue, the hidden file, the reviewer as collaborator and the webhook, using the names and the secret from `data.rs`. Prints both tokens as JSON. The same call serves a fresh and a kept cluster.
  - `playwright-versions`: checks that `playwright-core` of the Playwright server image, the base image tag in `MODULE.bazel` and the driver of the locked `playwright-rs` agree, and prints the Playwright and Node versions the driver install needs.
- **`crates/df-e2e/src/forgejo.rs`** gains basic auth and the calls the setup needs: create user, create and delete token, create and delete repository, add collaborator, create webhook.
- **BREAKING (e2e runner only):** a reused cluster runs `setup forgejo` again instead of restoring tokens. The Secret `e2e-state` keeps only `manifest-hash`; `state_value` and `restore_forgejo_users` are removed. A cluster kept with the old format keeps working (the extra keys are ignored).
- **Single source for credentials:** the Forgejo admin credentials move from `deploy/e2e/forgejo-values.yaml` to the script, which passes them to Helm (`--set`) and to the binary (environment). The bot and reviewer passwords move from the script to `data.rs`.
- **`e2e-local.sh`** keeps the sequence (tools, cluster, images, Helm, `setup`, `seed`, `cargo test`, traps, log collection) and the push check (`git`). The binaries `setup` and `seed` are built first in the background, before the test binaries; the script waits for them before the Forgejo step. Expected size: about 500 lines (the removed parts are about 150).
- **Unit tests** (`cargo test -p df-e2e --lib`, no cluster): request paths and bodies against a `wiremock` server, idempotent user setup (existing user, existing token), reset with missing repositories, version parsing and mismatch messages.
- The environment variables of the runner (`E2E_KEEP`, `E2E_FRESH`, `E2E_LIST`, `E2E_REPEAT`, `E2E_TEST_THREADS`, `E2E_TEST_ARGS`, `E2E_ARTIFACTS_DIR`, …) do not change.

### Out of scope

- A `state` subcommand for `e2e-state` and moving `cluster_is_reusable` into Rust: once the tokens are created on every run, what remains is `kubectl` calls and one hash comparison; Rust would only wrap `Command`. Rejected.
- Rewriting the whole runner in Rust or `cargo xtask`: it would call the same CLIs through `Command`, for little gain. Rejected.
- Tilt or helmfile for the cluster: one more pinned tool for four charts and a few manifests. Rejected.
- Moving the Playwright driver download (`install_driver`: `curl`, `tar`) into Rust: plain shell, no parsing left once the versions come from the binary. Rejected.
- Tests of the remaining shell: `shellcheck` already runs in the pre-commit hook; the remaining script is a sequence of CLI calls. Rejected.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `e2e-runner`: the reuse of a kept cluster recreates the Forgejo tokens instead of restoring them; new requirements for the Forgejo setup through the API with the suite's names and for the Playwright version check before the cluster is used.

## Impact

- **Code:** new `crates/df-e2e/src/bin/setup.rs` and `crates/df-e2e/src/setup/` (Forgejo setup, version check); `crates/df-e2e/src/forgejo.rs` (basic auth, new calls); `crates/df-e2e/src/data.rs` (user passwords, issue title, hidden file content).
- **Runner:** `deploy/e2e/e2e-local.sh` (Forgejo and version functions replaced by `setup` calls, build order), `deploy/e2e/forgejo-values.yaml` (admin block removed). Overlaps `openspec/changes/faster-test-loop` task 4.2 (TD-110, `--set` for `ALLOWED_HOST_LIST` in the same Helm call): whichever lands second rebases.
- **CI:** `.github/workflows/e2e.yaml` unchanged; it runs the script.
- **Dependencies:** `wiremock` (workspace dependency already) as a dev-dependency of `df-e2e`. No new crate in the workspace.
- **Docs:** `docs/demo.md` (steps 4 and the reuse section), skill `e2e`. No published docs change: nothing a user or operator sees or configures changes.
