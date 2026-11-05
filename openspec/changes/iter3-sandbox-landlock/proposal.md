# Proposal

## Why

The runtime and the harness run as the same UID in one container, and nothing separates them. A prompt-injected harness can:
- read the runtime's mTLS private key (`/run/factory/tls/tls.key`) and impersonate the sandbox towards the control plane;
- rewrite the transcript outbox under `/data/runtime`;
- signal or trace the runtime;
- modify system directories of the project image.

ADR-0054 decides to restrict the harness with Landlock. Landlock ABI 7 is available on both supported runtimes (runc on Talos 6.18, Kata guest 6.18).

## What Changes

- **Sandbox wrapper**:
  - The runtime starts every harness through a new subcommand of its own binary, `factory-agent sandbox -- <command>`. This covers the default adapter, project adapter commands and full-harness mode.
  - The subcommand applies a Landlock ruleset to itself and then `exec`s the command. The runtime itself stays unrestricted.
- **Ruleset**:
  - read and execute: every top-level directory except the ones holding `/data` and `/run`;
  - full access: `/data/repo`, `/data/home`, `/tmp` and `/dev`;
  - read only: a copy of the CA certificate in `/run/factory/ca/`, refreshed by the runtime when the CSI driver rotates `tls/` (a rule on the original file would break at the first renewal);
  - top-level symlinks are skipped, writable paths with a symlink component make the run fail, and hard links into the runtime directory left by unrestricted runs are broken before every run;
  - no access: everything else, including `tls.key`, `tls.crt`, `/data/runtime` and the rest of `/run/factory`;
  - Landlock signal and abstract-socket scoping, so the harness can't signal the runtime or connect to abstract Unix sockets outside its domain;
  - no Landlock network rules.
- **Environment and descriptors**: the wrapper removes the runtime's own `DF_*` variables, except those the factory MCP server needs, before `exec`. The harness inherits only stdin, stdout and stderr.
- **Fail closed**: the runtime requires Landlock ABI 6 or higher. Below that it fails the run with a `sandbox.landlock_unavailable` notice and outcome `FAILED`, before the harness starts.
- **Opt-out**:
  - The Helm value `sandbox.landlock: enforce | disabled` (default `enforce`) reaches the instance config and the sandbox pod as `DF_LANDLOCK`.
  - With `disabled`, the runtime starts the harness unrestricted and logs a warning at every run start.
  - The setting is instance-wide only.
- **System prompt**: with Landlock enforced, the fixed "Sandbox" section of the system prompt names the writable directories and says that everything else is read-only or inaccessible. This is the same section `iter3-hidden-paths` adds for hidden paths.
- **Docs**:
  - a security page "Sandbox filesystem isolation", with the ruleset, the threat model, the kernel requirements per runtime and a check command;
  - the `DF_LANDLOCK` reference in the runtime configuration;
  - the Helm value;
  - a troubleshooting entry for `sandbox.landlock_unavailable`.

Out of scope:
- **Seccomp profiles beyond `RuntimeDefault`**: backlog (`roadmap.md`).
- **Landlock network rules**: rejected. NetworkPolicy and the egress gateway cover network access, and Landlock has no UDP rules.
- **Hidden paths**: `iter3-hidden-paths` (iteration 3).
- **Separate UIDs for runtime and harness**: backlog (`roadmap.md`, existing row).
- **A per-project opt-out**: rejected. Any project could then turn the restrictions off.
- **Best-effort mode on older kernels**: rejected. It would give a sandbox that is silently unrestricted.

## Capabilities

### New Capabilities
- `sandbox-filesystem-isolation`: the Landlock ruleset applied to every harness, the minimum ABI and fail-closed behavior, the instance opt-out and the system prompt section.

### Modified Capabilities
- `acp-harness` ("Harness start"): the adapter command is started through the sandbox wrapper.
- `helm-deployment`: adds the `sandbox.landlock` value and its wiring into the instance config.

## Impact

- `crates/factory-agent`:
  - subcommand `sandbox` (ruleset, ABI check, `exec`);
  - `spawn_harness` wraps the adapter command;
  - an ABI probe before the spawn, and the notice;
  - `DF_LANDLOCK` in `config.rs` and `docs/configuration.md`.
- `crates/df-core`: `SandboxConfig.landlock` (`enforce` / `disabled`).
- `crates/df-k8s`: renders `DF_LANDLOCK` on the sandbox container.
- `crates/df-agent-channel`: the "Sandbox" prompt section when enforced.
- `deploy/helm/dark-factory`: value, schema and instance-config wiring, plus a Helm unit test.
- Dependencies: `landlock` (new, latest release; the maintained Rust binding by the Landlock author, safe API, no `unsafe` in our code).
- `crates/df-e2e`: the fake ACP agent tries to read `tls.key` and to write under `/usr`, and gets `EACCES` for both. The e2e kind or OrbStack kernel must provide Landlock ABI 6; otherwise the suite sets `disabled` and skips that step.
