# Design

## Context

ADR-0054 decides that Landlock protects the runtime and the system directories from the harness. Today:

- **Spawn**: `spawn_harness` (`factory-agent/src/runner.rs`) starts `Assign.harness.acp.command` directly with `tokio::process::Command`, cwd `/data/repo`, the inherited pod environment and the harness env. It sets no `pre_exec` and no restrictions.
- **Descendants**: the harness starts the Claude CLI, shell tools and the factory MCP server (`factory-agent mcp --socket /run/factory/runtime.sock`).
- **Pod**: runs as UID 1000 with `allowPrivilegeEscalation: false` (so `no_new_privs` is set), all capabilities dropped, `RuntimeDefault` seccomp, and a writable root filesystem.
- **Mounts**: `/opt/factory` (ro), `/opt/harness/<name>` (ro), `/run/factory/tls` (ro, CSI, rotated), `/run/factory` (emptyDir), `/data` (task PVC with `repo/`, `home/`, `runtime/`).
- **mTLS files**: the runtime reads `/run/factory/tls/*` on every reconnect. The harness needs only `ca.crt` (`NODE_EXTRA_CA_CERTS`, and `sslCAInfo` for its own Git commands).
- **No `pre_exec`**: the workspace forbids `unsafe_code`, so `CommandExt::pre_exec` is not available.
- **Probe result** (2026-11-05): Landlock ABI 7 on runc (Talos 6.18.54) and on Kata (guest 6.18.35), under `RuntimeDefault` seccomp, non-root, with all capabilities dropped.

## Goals / Non-Goals

**Goals:**
- The harness and every descendant can't read `tls.key` or `tls.crt`, can't write `/data/runtime` or `/run/factory`, and can't write system directories.
- The harness can't signal, trace or read the memory of the runtime.
- Project images keep working without a per-image allowlist.
- No silent downgrade: a run without enforcement happens only when the operator opted out.

**Non-Goals:**
- Seccomp profiles beyond `RuntimeDefault`: backlog (`roadmap.md`).
- Landlock network rules: rejected (NetworkPolicy and the egress gateway cover network access; Landlock has no UDP rules).
- Hidden paths: `iter3-hidden-paths`.
- Separate UIDs for runtime and harness: backlog (`roadmap.md`).

## Decisions

### D1: Re-exec wrapper instead of `pre_exec`

`spawn_harness` starts `/proc/self/exe sandbox -- <adapter command…>` (the runtime's own binary, the same pattern as `mcp`), keeping cwd, environment and pipes unchanged. The `sandbox` subcommand:

1. reads the layout from the runtime's environment (`DF_DATA_DIR`, `DF_RUN_DIR`, `DF_TLS_DIR`), which the child inherits;
2. builds and applies the ruleset (D2, D3) with the `landlock` crate's safe API;
3. removes the runtime's own variables from the environment: every `DF_*` except `DF_RUNTIME_SOCK` and `DF_RUN_DIR` (used by the MCP server) and `DF_LOG_FORMAT`;
4. replaces itself with the adapter command through `std::os::unix::process::CommandExt::exec`, which is safe.

The environment is not reduced to an allow-list, because the runtime doesn't know the project image's `ENV` (`PATH`, `JAVA_HOME`, `CARGO_HOME`, …), and dropping it would break toolchains. None of the removed variables is a secret (ADR-0050); removing them just stops pointing the agent at the control plane. Landlock doesn't restrict descriptors opened before the ruleset applies. Rust and tokio open descriptors close-on-exec, so only stdin, stdout and stderr reach the harness, and a test checks it (D7).

If any step fails, it prints the error to stderr and exits with code 125 before `exec`. The harness never runs unrestricted by accident.

Alternatives:
- `pre_exec`: rejected, because it is `unsafe`.
- Restricting the runtime itself: rejected, because the runtime must keep reading the rotated `tls.key`.

### D2: Ruleset from the root directory listing

A pure function `ruleset(layout, root_entries) -> Vec<(PathBuf, Rights)>` builds the rules:

| Path | Rights |
|---|---|
| each entry of `/` that is a directory, not a symlink, and not the top-level ancestor of the data, run or TLS directory (`/data`, `/run`) | read file, read dir, execute |
| `<data dir>/repo`, `<data dir>/home` (default `/data/repo`, `/data/home`), `/tmp`, `/dev` | all ABI 6 filesystem rights |
| `<run dir>/ca/` (default `/run/factory/ca/`) | read file, read dir |

Paths come from the layout (`DF_DATA_DIR`, `DF_RUN_DIR`, `DF_TLS_DIR`), not from constants. Entries of `/` are listed once per run, about 20 names; nothing is walked recursively.

**Symlinks.** Landlock binds a rule to the inode the path resolves to and checks accesses on the resolved path. A rule on a top-level symlink is therefore at best redundant and at worst a grant on its target: a symlink `/x → /` would cover `/run/factory/tls/tls.key`. All top-level symlinks are skipped. Debian's `/bin → usr/bin` keeps working, because `/bin/sh` resolves to `/usr/bin/sh`, which the `/usr` rule covers; `/var/run → /run` grants nothing under `/run`. Mounts below `/opt` are covered by the rule on `/opt`. Missing optional entries (`/tmp` on a distroless image) are skipped.

**Writable paths.** Each writable root must be a directory whose canonical path equals the configured one (no symlink in any of its own components). Symlinks inside the roots (`bazel-bin`, `node_modules/.bin`) aren't checked: Landlock checks the resolved path, so a link into another writable root works and a link to `/data/runtime` is denied. Otherwise the wrapper fails with exit code 125. This stops a symlink planted in an earlier unrestricted run (`/data/home → /data/runtime`, or `/tmp → /run/factory`) from turning the full-access rule into a grant on protected files.

**CA certificate.** The cert-manager CSI driver rotates `tls/` by writing a new timestamped directory and repointing `..data`, so `ca.crt` gets a new inode on every renewal (every 4 hours with the default 6 h duration). A rule on the file would bind to the old inode, and Git commands of the harness started after the renewal would fail. A rule on `tls/` would expose `tls.key`. So the runtime copies `ca.crt` into `<run dir>/ca/` (write a temporary file in that directory, then rename) at startup and whenever it reloads its TLS material. The harness gets a directory rule on `<run dir>/ca/`, which covers every replaced file. `http.<proxy>.sslCAInfo` and `NODE_EXTRA_CA_CERTS` point at the copy. The copy holds only the public CA certificate.

**Hard links.** Landlock checks the path used, so a hard link to an outbox file inside `<data dir>/home` or `<data dir>/repo` would give the harness write access to it. Under enforcement the harness can't create one: linking needs `REFER` and access to the source, and `<data dir>/runtime` grants nothing. A link left by an earlier unrestricted run (`disabled`, or a run before this change) would persist on the task volume. So before every run, the runtime replaces each file under `<data dir>/runtime` whose link count is above one with a copy (temporary file plus rename), which breaks the alias.

Alternative: a fixed list of system directories. Rejected, because toolchains under `/opt/*`, `/usr/local` or `/home/dev` would break.

### D3: ABI 6 pinned, hard requirement

The ruleset handles exactly the ABI 6 rights:
- filesystem rights including `REFER`, `TRUNCATE` and `IOCTL_DEV`;
- scopes `SIGNAL` and `ABSTRACT_UNIX_SOCKET`.

It uses the crate's hard-requirement compatibility level. In addition, the wrapper checks that `restrict_self()` reports `RulesetStatus::FullyEnforced`, and treats any other status as a setup failure (exit code 125). Newer kernels don't change behavior, because rights the ruleset doesn't handle stay allowed. One example is a future right for connecting to pathname Unix sockets, which would otherwise cut off the MCP server's connection to `runtime.sock`.

Scoping blocks signals to the runtime and `ptrace` / `/proc/<pid>/mem` access to it. The runtime can still signal and kill the harness, its child. `no_new_privs` is already set by the pod spec, and the crate sets it again.

Alternatives:
- ABI 7: rejected. It only adds audit logging of denials, which goes to the node's kernel log, out of reach of the pod.
- Best effort: rejected (ADR-0054).

### D4: Probe before spawn, notice on failure

Before writing the harness files, the runtime probes the current ABI (`landlock` crate). Below 6, it:
- emits a `sandbox.landlock_unavailable` notice ("Landlock ABI 5 found, 6 required. Set `sandbox.landlock: disabled` to run without it." or "Landlock is not available in this kernel. …");
- finalizes the run as `FAILED` with that text as detail, without starting the harness.

The wrapper's own hard requirement (D1) covers the race where the probe passes and applying the ruleset fails anyway. The harness then exits with code 125 and the existing "harness crashed" path applies.

### D5: Opt-out wiring

- **`df-core`**: `SandboxConfig.landlock: LandlockMode` (`enforce` default, `disabled`), serde `lowercase`.
- **Helm**: `sandbox.landlock` with schema enum `[enforce, disabled]`, rendered into the instance config next to `sandbox.logLevel`.
- **`df-k8s`**: `render.rs` sets `DF_LANDLOCK` on the sandbox container, the same way as `RUST_LOG`.
- **`factory-agent` config**: `DF_LANDLOCK` is parsed at startup; an invalid value exits with an error naming the variable. With `disabled`, `spawn_harness` starts the adapter directly and logs `warn!(run_id, "Landlock disabled by instance configuration; harness runs unrestricted")` at every run start.

### D6: System prompt section

With `sandbox.landlock: enforce`, `build_assign` appends to the fixed `## Sandbox` block after the persona prompt. The control plane renders every sandbox pod with the default layout (`df-k8s` never sets `DF_DATA_DIR`; it is a test override of the runtime), so the prompt names the default paths:

```
Writable: /data/repo (workspace), /data/home (your home), /tmp. Everything else is read-only or inaccessible on purpose; don't try to change permissions.
```

If `iter3-hidden-paths` has landed, the hidden-path sentence goes into the same block; otherwise this change creates the block.

### D7: Tests

- `ruleset()` is pure and tested on fake root listings:
  - `/data` and `/run` are excluded;
  - every top-level symlink is skipped;
  - `/tmp` is missing on distroless.
- Kernel-backed tests run the real wrapper (`cargo run`-built binary via `env!("CARGO_BIN_EXE_factory-agent")`). They use a layout of tempdirs and a hidden `--root` flag of the `sandbox` subcommand. The flag is harmless in production: an agent calling the wrapper can only add restrictions to its own domain, never remove them, then check that:
  - reading a fake `tls.key` gives `EACCES`;
  - the CA copy is readable, also after it was replaced by rename during the run;
  - writing to the data dir's `runtime/` fails;
  - a symlinked writable path (`home → runtime`) and a top-level symlink to `/` in the fake root make the wrapper exit with 125, or are skipped, respectively;
  - a hard link from `home/` to a `runtime/` file is broken by the pre-run step, and writes through it no longer reach the outbox;
  - `kill` of the parent fails with `EPERM`;
  - `env` shows no removed `DF_*` variable but keeps the others;
  - `/proc/self/fd` holds only 0, 1 and 2.

  They need a kernel with Landlock ABI 6. CI's runners provide it. On a machine without it, the tests fail with a message naming the kernel requirement instead of skipping silently.

## Risks / Trade-offs

- **Tools that write outside the writable set**, for example `~/.npm` with a `HOME` other than `/data/home`, or `/var/tmp`, now fail with `EACCES`. Landlock has no permissive mode, so trial runs with real Claude Code sessions (tasks 5.2) collect such failures before the change merges. The prompt section and the docs name the writable directories. A project that needs more uses its image to point caches into `/data/home` or `/tmp`.
- **Writable root filesystem left as is**: the runtime can still write anywhere it could before. Only the harness tree is restricted, which is the threat this change addresses.
- **Kernel availability on other clusters:** operators without ABI 6 must opt out explicitly. The notice text says how.
- **e2e environment**: the local kind or OrbStack kernel must offer ABI 6. If it doesn't, the e2e configures `disabled` and skips the Landlock step, stating so in its output.

## Migration Plan

The default `enforce` applies to existing instances on upgrade. The upgrade notes in `docs/content/operations/upgrades.md` name the kernel requirement (Linux 6.12 or newer with Landlock in the LSM list) and the opt-out.

## Open Questions

None.
