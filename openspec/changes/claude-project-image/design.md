# Design

## Context

`deploy/images/claude-project/Dockerfile` exists since `iter1-build-and-deploy` (task 3.2, design D4) and is documented in `deploy/README.md` §2 as a manually built image. The runtime spawns the adapter from `harness.claudeCode.adapterCommand` (default `["/opt/harness/bin/claude-agent-acp"]`, `crates/df-core/src/config.rs`). The control plane and runtime images are built by Bazel (`rules_oci`); CI (`.github/workflows/ci.yaml`) runs Bazel only, the release workflow pushes `//deploy:push_images`.

## Goals / Non-Goals

**Goals:**
- A reference image that a sandbox can run with the default adapter command.
- A check that fails when the adapter path, the user or the Node.js version is wrong.

**Non-Goals:**
- Building the image with Bazel or adding it to `//deploy:dev`.
- Pinning the Claude Code CLI separately from the adapter, image signing, SBOMs.
- Changes to the runtime or the adapter configuration.

## Decisions

### D1 — Dockerfile stays, built with BuildKit
The image needs `npm install` of a Node package; `rules_oci` has no good path for that without vendoring `node_modules`. The Dockerfile stays the source of truth and CI builds it with `docker/build-push-action` (pinned by commit like the other actions). *Alternative:* `rules_js` + `rules_oci` — rejected, a large new toolchain for one reference image.

### D2 — Package and base
`@agentclientprotocol/claude-agent-acp` pinned with `ARG CLAUDE_AGENT_ACP_VERSION` (0.95.0 at proposal time, requires Node ≥ 22). Base `node:24-trixie-slim` (current LTS, Debian 13 as before). The symlink `/opt/harness/bin/claude-agent-acp → $(npm prefix -g)/bin/claude-agent-acp` stays; UID 1000 (`node` user of the base image) stays.

### D3 — Structure check
A shell script `deploy/images/claude-project/test.sh <image>` runs the image and checks: `/opt/harness/bin/claude-agent-acp` resolves to an executable file of the installed package, `node --version` satisfies the package's `engines.node`, `git --version` works, `id -u` is 1000. It does not start the adapter: the adapter speaks ACP on stdio and waits for input, so a "start it" check needs a timeout and proves little. *Alternative:* `container-structure-test` — the repo already uses shell checks (`deploy/test_images.sh`), one more tool isn't worth it.

### D4 — CI and release
- New workflow `.github/workflows/claude-project-image.yaml`: on `pull_request` and `push` to `main` with `paths: deploy/images/claude-project/**` it builds for `linux/amd64` (load) and runs `test.sh`.
- The release workflow gets a second job: build for `linux/amd64,linux/arm64` with QEMU, run `test.sh` on the amd64 image, push `ghcr.io/prskr/dark-factory/claude-project:<CLAUDE_AGENT_ACP_VERSION>` and `:latest`. The tag comes from the Dockerfile `ARG` (one place for the version).

### D5 — Carried-over checks of `iter1-egress-gateway` 5.1
The four open checks run in the dev cluster with a sandbox from this image. For checks that need a sandbox that stays up longer than a run, a pod with the sandbox labels and proxy environment running `sleep` is enough; how to start it is described in the task.

## Risks / Trade-offs

- [The adapter's bin name or layout changes in a later release] → `test.sh` fails in the pull request that bumps the version.
- [`latest` moves under projects that use it] → the README recommends the version tag.
- [QEMU build for arm64 is slow] → only on release, not on pull requests.
