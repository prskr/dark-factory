# Tasks

No prerequisites; independent of every other iteration-3 change. If `iter3-hidden-paths` landed first, extend its `## Sandbox` prompt block (design D6) instead of creating it.

Kernel-backed tests need Landlock ABI 6 (Linux 6.12+ with `landlock` in `/sys/kernel/security/lsm`). Container-backed tests (`df_test_postgres`) need a container socket; see `AGENTS.md`.

## 1. Configuration

- [x] 1.1 Write failing tests, then add `LandlockMode` (`enforce` default, `disabled`) as `SandboxConfig.landlock` in `df-core` (design D5). Verify with `cargo test -p df-core config`: the default, an explicit `disabled`, and the rejection of `off`.
- [x] 1.2 Write a failing test, then render `DF_LANDLOCK` on the sandbox container in `df-k8s/src/render.rs`. Verify with the existing render tests, extended by one assertion per mode.
- [x] 1.3 Add `sandbox.landlock` to `values.yaml`, `values.schema.json` and the instance-config template, with Helm unit tests for the three `helm-deployment` scenarios. Verify with the chart tests (`just helm-test` or the lefthook Helm step).
- [x] 1.4 Write failing tests, then parse `DF_LANDLOCK` in `factory-agent/src/config.rs` and add it to `crates/factory-agent/docs/configuration.md`. The existing doc-sync test checks the doc entry. Verify with `cargo test -p factory-agent config`.

## 2. Wrapper (`factory-agent`)

- [x] 2.1 Add `landlock` (latest release) to `[workspace.dependencies]` and `factory-agent`, then update `Cargo.lock` and the Bazel lock files (skill `rust`). Verify with `cargo build -p factory-agent` and `cargo deny check`.
- [x] 2.2 Write failing tests, then implement the pure `ruleset(layout, root_entries)` (design D2). Verify with unit tests for:
  - excluded `/data` and `/run`;
  - full access on repo, home, `/tmp` and `/dev`, taken from the layout (a non-default `DF_DATA_DIR` moves the grants);
  - read on `<run dir>/ca/` and nothing from the TLS dir;
  - every top-level symlink skipped (`/x → /`, `/var/run → /run`, `/bin → usr/bin`);
  - a missing `/tmp`.
- [x] 2.2a Write failing tests, then copy `ca.crt` into `<run dir>/ca/` (temporary file plus rename) at startup and on every TLS reload, point `sslCAInfo` in `render_git_config` and `NODE_EXTRA_CA_CERTS` (`df-core/src/harness.rs`, the path asserted in `df-agent-channel/src/assign.rs`) at the copy, and, at runtime start, refuse a symlinked `<data dir>/runtime`, remove symlinks inside it and break hard links under it (design D2). Verify with tests: the copy follows a renamed `ca.crt`, a file with link count 2 is replaced while its content stays, a symlinked runtime directory is refused, and a symlink inside it is removed while its target stays.
- [x] 2.3 Write failing kernel-backed tests, then implement the `sandbox` subcommand: read the layout from the environment, take the hidden `--root` flag, apply the ABI 6 ruleset with a hard requirement and signal plus abstract-socket scoping, then `exec` (design D1, D3). Verify with tests against the built binary that:
  - a fake `tls.key` is unreadable;
  - the CA copy is readable, also after it was replaced by rename;
  - a symlinked `home` exits with 125;
  - `runtime/` is not writable;
  - `kill` of the parent fails with `EPERM`, and reads of the parent's `/proc/<pid>/environ` and `mem` fail;
  - `exec` of a binary under a top-level directory other than `/usr` works;
  - `env` of the command has no `DF_CONTROL_PLANE_ADDR`, `DF_TLS_DIR` or `DF_LANDLOCK`, but keeps `DF_RUNTIME_SOCK`, `DF_RUN_DIR`, `DF_LOG_FORMAT` and `PATH`;
  - `/proc/self/fd` of the command holds only 0, 1 and 2, also when the parent has other descriptors open;
  - a setup failure, including a status other than `FullyEnforced`, exits with code 125 without running the command.
- [x] 2.4 Write failing tests, then wrap the adapter command in `spawn_harness`, probe the ABI before the spawn and emit `sandbox.landlock_unavailable` with outcome `FAILED` (design D4); with `disabled`, start the adapter directly and log the warning. Verify with runner tests using an injected ABI probe (a single-method trait, faked in tests): ABI 5 gives the notice and `FAILED` without a spawn, ABI 6 spawns through the wrapper, and `disabled` spawns directly and logs a warning.

## 3. System prompt (`df-agent-channel`)

- [x] 3.1 Write failing tests, then append the writable-directories sentence to the `## Sandbox` block in `build_assign` when `sandbox.landlock` is `enforce` (design D6). Verify with tests for both modes, and, if `iter3-hidden-paths` is merged, together with hidden paths in one block.

## 4. Documentation

- [x] 4.1 Write `docs/content/security/sandbox.md`, "Sandbox filesystem isolation". Link it from `docs/content/security/_index.md`. Verify with `just docs-build`. Content:
  - the threat model;
  - the ruleset table from design D2;
  - signal and ptrace scoping;
  - kernel requirements per runtime (runc: node kernel; Kata: guest kernel, Landlock enabled in the stock guest kernel since Kata 3.32.0; Linux 6.12+, `landlock` in the LSM list);
  - check commands: `kubectl debug node` for runc, and for Kata a pod with the configured runtime class and a security context the `restricted` policy accepts;
  - `sandbox.landlock: disabled` and what it gives up.
- [x] 4.2 Add a `sandbox.landlock_unavailable` entry to `docs/content/operations/troubleshooting.md` and the kernel requirement plus opt-out to `docs/content/operations/upgrades.md`. Verify with `just docs-build`.

## 5. Integration

- [ ] 5.1 Extend the e2e suite (`crates/df-e2e`) with a fake ACP agent script that reads `/run/factory/tls/tls.key` and writes to `/opt/probe-writable/x`, and expects `EACCES` for both, plus a factory tool call that still succeeds. The test image creates `/opt/probe-writable` owned by UID 1000, so the write is a control: it succeeds with `disabled` and fails only through Landlock, unlike `/usr`, which file permissions already protect. When the cluster kernel lacks ABI 6, the suite sets `disabled`, skips this step and prints why. Verify with `just e2e-local`.
- [ ] 5.2 Run a manual check on the homelab cluster with `runtimeClassName: kata` and with runc. Record both parts in the review log.
  - **Denial:** in each runtime, start a run with the Claude Code harness, ask it to `cat /run/factory/tls/tls.key`, and confirm the denial in the transcript.
  - **Trial run:** Landlock has no permissive mode. Let real sessions build and test a Rust project and a Node project (`cargo`/`npm` caches, test suites with localhost ports). Collect every `EACCES` from failing tool calls and the harness stderr. Fix each one by adjusting the ruleset or the docs, never by silently widening `/data`.
- [ ] 5.3 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo xtask check-file-length`, `openspec validate iter3-sandbox-landlock --strict` and `just bazel-test`. Verify that all pass.

## Workflow follow-up

- Archive the change after review, then update `product/iterations/iteration-3.md` if the scope changed during implementation.
