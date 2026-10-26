# ADR-0022: Per-project sandbox images, runtime via image volume, no warm pools

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0005](0005-agent-runtime-stack.md), [ADR-0023](0023-kata-isolation-no-nested-containers.md)

## Context

Projects need their own toolchains. agent-sandbox offers warm pools, but a pool would be needed per image, and continuation runs need the task PVC, which warm sandboxes can't carry. Paperclip's "environments" (fixed harness and image per environment) were considered as a middle ground.

## Decision

- Each project configures its sandbox image; the same image can serve as a devcontainer image.
- The runtime (static musl binary plus read-only support files) is mounted via a Kubernetes image volume, so project images don't include it. There is no init-container fallback: image volumes (Kubernetes 1.36 or newer) are the only delivery.
- The harness is delivered the same way, as a second image volume per harness at `/opt/harness/<harness>` (`harness.claudeCode.image`, built from `deploy/images/claude-harness`). It bundles its own Node.js, the adapter, the Claude Code CLI and a musl runtime behind a static launcher, so it runs in glibc and Alpine project images. A project whose image already contains an adapter sets an adapter command; then no harness image is mounted.
- Minimum for project images: git and a shell. No Node.js and no adapter inside the project image.
- No warm pools: every run cold-starts.

## Consequences

- One image to maintain per project.
- Throughput comes from concurrent tasks, not start-up time.
- Image volume support must be verified with Talos' containerd and Kata.

## Alternatives considered

- **Warm pools per project or per environment**: pool management for little gain, since only a task's first run could use them.
- **Runtime baked into every project image**: couples runtime releases to project images.
- **Harness baked into every project image** (iteration 1): every project image needs Node.js and a pinned adapter; replaced by the harness image volume.
- **Init-container delivery of the runtime**: needs a copy tool in the `scratch` runtime image; removed.
