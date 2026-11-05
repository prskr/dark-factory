# ADR-0054: Filesystem isolation in the sandbox

- **Status:** Accepted (planned for iteration 3)
- **Date:** 2026-11-05
- **Related:** [ADR-0006](0006-agent-dial-back-grpc-mtls.md), [ADR-0009](0009-agent-managed-workspaces-git-durable.md), [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md), [ADR-0023](0023-kata-isolation-no-nested-containers.md), [ADR-0039](0039-repository-configuration-activated-by-approval.md)

## Context

The runtime, the harness and everything the harness starts run as the same UID in one container. The harness can read the runtime's mTLS key (`/run/factory/tls/tls.key`), write the transcript outbox under `/data/runtime` and write anywhere on the root filesystem. Users also want to keep files of a repository away from the agent, for example machine configs with certificate material, while the agent works on the rest.

Landlock (unprivileged, stackable LSM) is available at ABI 7 on both supported runtimes (runc on Talos 6.18, Kata guest 6.18) under `RuntimeDefault` seccomp. Landlock only grants access: it cannot deny a subpath of an allowed directory, and rules bind to inodes that exist when the ruleset is applied. The agent's shell has full access to `.git`, so committed content stays reachable through `git show` and `git log -p` whatever the filesystem allows.

## Decision

1. **Landlock protects the runtime and the system.** The runtime starts the harness through its own subcommand, which applies a Landlock ruleset and then `exec`s the harness; the runtime itself stays unrestricted and keeps reading the rotated mTLS files. The harness and its descendants get read and execute on every top-level directory except `/data` and `/run`, read and write on the workspace, the harness home, `/tmp` and `/dev`, read on the CA certificate only, and nothing else. ABI 6 is the minimum (filesystem rights plus signal and abstract-socket scoping). Without it the run fails closed, unless the operator disabled Landlock for the instance.
2. **Hidden paths use sparse checkout and the push check, not Landlock.** The runtime leaves a project's hidden paths out of the checkout, and the Git proxy rejects every push (task and WIP branch) that touches one. Hidden paths are configured on the project in the control plane, never in the repository.
3. **Hiding content from the repository history is deferred.** Hidden paths keep content out of the agent's workspace, not out of `.git`. Refusing blob fetches for hidden paths in the Git proxy is a later change.

## Consequences

- A prompt-injected harness can no longer read the runtime's private key, tamper with the outbox, signal or trace the runtime, or modify system directories.
- Landlock rules apply to the project image as it is: no per-image allowlist, and new toolchain directories under `/` work without configuration.
- Operators on kernels without Landlock ABI 6 must opt out explicitly (`sandbox.landlock: disabled`); hidden paths keep working there.
- Hidden paths are a convenience against accidental exposure, not a secret store. The documentation says so and recommends encrypting committed secrets.
- The WIP snapshot and the runtime's Git config handling have to preserve the sparse-checkout state.

## Alternatives considered

- **Landlock for hidden paths:** needs a rule on every sibling along each hidden path; files replaced by rename and new files in those directories become unreadable. It is no stronger than sparse checkout while `.git` is readable.
- **`pre_exec` hook in the runtime:** `unsafe`, which the workspace forbids.
- **A fixed allowlist of system directories:** breaks project images that install toolchains under `/opt`, `/usr/local` or a user's home.
- **Best-effort Landlock:** a sandbox that is silently unrestricted on older kernels.
- **Hidden paths in the repository config file (ADR-0039):** agent-writable, and the security context is excluded from that file.
