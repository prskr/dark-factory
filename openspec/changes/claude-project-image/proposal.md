# Proposal

## Why

Runs with the Claude harness fail with `Failed to spawn harness process '/opt/harness/bin/claude-agent-acp'`: the reference Claude Code project image (`deploy/images/claude-project/Dockerfile`) is documented as "built once and pushed manually" (`iter1-build-and-deploy` design D4) and nothing builds or checks it. It also pins `@zed-industries/claude-agent-acp@0.23.1`, which has been renamed to `@agentclientprotocol/claude-agent-acp`. Without a working image there is no long-lived sandbox, so the dev-cluster checks of `iter1-egress-gateway` (task 5.1) are still open.

## What Changes

- The reference image installs `@agentclientprotocol/claude-agent-acp` at its latest release (0.95.0 at the time of writing) on a current Node.js LTS base and keeps the adapter at `/opt/harness/bin/claude-agent-acp` (the default of `harness.claudeCode.adapterCommand`) and the user UID 1000.
- A structure check verifies the image: adapter path executable, node version the package requires, git present, UID 1000.
- CI builds and checks the image on pull requests that touch it; the release workflow pushes it to `ghcr.io/prskr/dark-factory/claude-project` for amd64 and arm64, tagged with the adapter version.
- `deploy/README.md` §2 describes the image as a CI-built reference image projects may use or copy.
- The open dev-cluster checks of `iter1-egress-gateway` task 5.1 become verification tasks of this change, run with a sandbox from this image.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `build-pipeline`: adds a requirement for the reference Claude Code project image (content, verification, publication).

## Impact

- `deploy/images/claude-project/` (Dockerfile, structure check), `.github/workflows/` (CI job, release job), `deploy/README.md`.
- No Rust code changes; `harness.claudeCode.adapterCommand` keeps its default.
- The image stays a project image (ADR-0024, `iter1-build-and-deploy` D4): it is not part of the Bazel build or `//deploy:dev`, and projects may point at another image.
