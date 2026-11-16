# Proposal

## Why

The control plane can't read files of a project's repository without a clone, and it has no Git library. Three planned features need single files or directory listings at a revision: the image builder (`.dark-factory/Containerfile` and its build context), the repository configuration file (ADR-0039, read from the base branch) and path completion in the composer. ADR-0059 decides these reads as a forge plugin capability.

## What Changes

- **Integrations WIT:** a new optional capability interface `contents`. A plugin without it doesn't export it, and features that need it report "not supported by this forge".
  - `revision` variant: `branch(name) | tag(name) | commit(sha)`.
  - `list(repo, revision, path)`: one directory level. Each entry has name, path, kind (`file`, `dir`, `symlink`, `submodule`), Git object ID and size, so a caller can detect changed content without downloading it. Returns the commit it read, at most 1,000 entries and `truncated`.
  - `get-file(repo, revision, path)`: `file(bytes) | symlink(target) | submodule`, with size, blob SHA and the commit it read. Files over 1 MiB fail with `too-large(size)`. A directory path fails with `invalid-input`.
  - **BREAKING:** the shared `error` variant gains `too-large(u64)`.
- **Host:** detects `contents`. `ForgeClient` gains `list_repo_dir` and `read_repo_file`, keyed by `&ProjectId`, so only the project's repository is reachable. The host enforces the 1 MiB cap on the returned bytes as well.
- **Forgejo plugin:** `contents` through the contents API and the raw file endpoint.
- **GitHub plugin:** `contents` through the contents API, raw media type for files.
- **Test probe plugin and fake forge client:** `contents` for tests.

Out of scope:
- **Image builder:** backlog (`roadmap.md`, decided).
- **Reading `.dark-factory/config.toml` (F11):** iteration 5 (`roadmap.md`).
- **Composer path completion:** backlog (`roadmap.md`, decided).
- **Recursive listing or fuzzy path search:** backlog (`roadmap.md`).
- **Agent tools and reads of other repositories:** backlog (`roadmap.md`).
- **Hidden-path filtering in the plugin or host:** rejected. Callers that show content to agents filter it themselves; humans see everything.
- **A `default-branch` revision:** rejected. The control plane always knows the project's base branch.
- **Paging through directories above 1,000 entries:** rejected. No consumer gets near it.
- **Reading from the gateway's mirrors:** rejected (ADR-0059).

## Capabilities

### New Capabilities
- `repository-contents`: listing a directory and reading a file of a project's repository at a revision, through the forge plugin, with its limits and errors.

### Modified Capabilities
- `plugin-host` ("Capability detection"): detects `contents`.
- `forgejo-integration`, `github-integration`: implement `contents`.

## Impact

- `contract/wit/integrations/integrations.wit`: `contents` interface, `too-large` error, new world export.
- `crates/df-core` (`seams.rs`, `testing.rs`): `ForgeClient` methods, domain types, fake.
- `crates/df-plugins`: detection, conversion, router, size check.
- `plugins/forgejo`, `plugins/github`, `plugins/forge-common`, `plugins/test-probe`.
- Every other `ForgeClient` implementation (`df-agent-channel` test `ObservingForge`).
- Docs: plugin reference (`contents` interface, required forge token scopes).
- ADR-0059.
