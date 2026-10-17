# ADR-0022: Per-project sandbox images, runtime via image volume, no warm pools

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0005](0005-agent-runtime-stack.md), [ADR-0023](0023-kata-isolation-no-nested-containers.md)

## Context

Projects need their own toolchains. agent-sandbox offers warm pools, but a pool would be needed per image, and continuation runs need the task PVC, which warm sandboxes can't carry. Paperclip's "environments" (fixed harness and image per environment) were considered as a middle ground.

## Decision

- Each project configures its sandbox image; the same image can serve as a devcontainer image.
- The runtime (static musl binary plus read-only support files) is mounted via a Kubernetes image volume, so project images don't include it. Fallback: init container copying into an `emptyDir`.
- Minimum for project images: git, a shell, glibc (for the Claude adapter). Iteration 1: Node and the Claude ACP adapter are baked into the project image.
- No warm pools: every run cold-starts.

## Consequences

- One image to maintain per project.
- Throughput comes from concurrent tasks, not start-up time.
- Image volume support must be verified with Talos' containerd and Kata.

## Alternatives considered

- **Warm pools per project or per environment**: pool management for little gain, since only a task's first run could use them.
- **Runtime baked into every project image**: couples runtime releases to project images.
