# ADR-0009: Agent-managed workspaces with Git as the only durable state

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0010](0010-credentials-in-the-sandbox.md), [ADR-0025](0025-pr-and-branch-handling.md)

## Context

Paperclip's control plane manages workspaces, copies them into sandboxes and merges them back. That causes security and correctness problems: symlink handling, stale Bazel symlinks breaking runs, and the most privileged component parsing untrusted archives.

## Decision

- The runtime owns the workspace; the control plane only tracks metadata (repo, task branch, PVC name, pushed SHAs, run records).
- Each task gets a ReadWriteOnce PVC (Longhorn StorageClass with one replica — it's a cache), mounted into every sandbox of the task's strictly sequential runs. The RWO volume doubles as a per-task lock.
- The runtime does a blobless clone, maintains the task branch (default `factory/task/<key>`, see ADR-0025), fast-forwards when the remote moved and never resolves divergence automatically.
- It pushes after every turn with new commits; at the end of each run it pushes committed work and a snapshot of uncommitted work to `factory/wip/<key>`. The snapshot respects `.gitignore` and drops paths matching a secret-file denylist (`.env*`, `*.pem`, `*.key`, `id_*`, …).
- Secret scanning is not done in the sandbox. It happens once, centrally, in the Git proxy (F1 in `product/context.md`), which covers every push regardless of which process in the sandbox makes it.
- Everything on the PVC is untrusted at run start: the runtime regenerates `.git/config` from an allowlisted template and the harness configuration from what the control plane sent.
- Git config entries can be added at instance and project scope (e.g. `rebase.*`, `pull.*`, `merge.conflictStyle`, `rerere.*`). They are applied in the order template → instance → project → runtime-owned keys, so they can't override the remote, hooks path, credential helper or commit identity. Keys that execute programs or redirect traffic are rejected when an entry is saved and again when the runtime renders the config (denylist; see `design/agent-run.md` §7.1).
- Commit hooks for the harness's own commits (`pre-commit`, `prepare-commit-msg`, `commit-msg`) are configured as commands at instance and project scope. The runtime generates a hook directory on tmpfs from them every run and points the harness's `core.hooksPath` at it; the runtime's own Git operations never run hooks. They give agents fast feedback; enforcement stays with CI and the Git proxy's push checks.

## Consequences

- Losing a PVC costs at most caches and the work since the last push.
- Until the Git proxy exists, pushes are not secret-scanned. Accepted for now: the forge is private, and the sandbox holds few secrets (ADR-0010).
- No workspace content ever crosses into the control plane.
- PVC garbage collection is safe at any time.
- Details: `design/agent-run.md`.

## Alternatives considered

- **Secret scanning in the runtime before its own pushes** (e.g. gitleaks or Kingfisher as a binary next to the runtime): bypassable, because the harness can push itself through the credential helper; adds a binary to every sandbox and a second scanner configuration.
- **Tarballs streamed through the control plane (Paperclip-style)**: the most privileged component parses untrusted archives (path traversal, symlink/hardlink escapes, archive bombs); merging two workspace copies has no correct answer; duplicates storage Git already provides.
