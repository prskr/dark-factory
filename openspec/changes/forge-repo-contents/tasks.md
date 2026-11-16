# Tasks

Coordination: `iter3-ci-tools` also modifies "Capability detection" in `plugin-host` and adds `unsupported(string)` to `types.error`. Whichever change lands second rebases its delta onto the other (both capabilities listed) and reuses the error.

## 1. Contract and domain types

- [ ] 1.1 Add interface `contents` and the `too-large(u64)` error (design D1) to `contract/wit/integrations/integrations.wit`, export `contents` from world `forge`, and add `unsupported(string)` if `iter3-ci-tools` hasn't landed. Add `Revision`, `RepoEntry`, `RepoListing`, `RepoFile`, `IntegrationError::TooLarge` (and `Unsupported`) to `crates/df-core/src/seams.rs`. Verify with `cargo build --workspace`.
- [ ] 1.2 Write failing tests, then add `list_repo_dir` and `read_repo_file` to `ForgeClient` (design D5) and implement them in `FakeForgeClient` (in-memory map by commit and path) and the test `ObservingForge` in `df-agent-channel`; `ForgeRouter` (`crates/df-plugins/src/router.rs`) returns `unsupported` until task 2.3. Verify with `cargo build --workspace` and `cargo test -p df-core` for the fake: listing, file, symlink, `too-large`, `not-found`.

## 2. Host (`df-plugins`)

- [ ] 2.1 Write failing tests, then make capability detection report `contents`. Verify with `cargo test -p df-plugins` for the "Forge plugin" and "Plugin without contents" scenarios, using `plugins/test-probe`.
- [ ] 2.2 Implement a scripted `contents` in `plugins/test-probe` (fixed listing, file, symlink, submodule, an over-sized file returned without `too-large`). Verify the probe builds for `wasm32-wasip2`.
- [ ] 2.3 Write failing tests, then implement conversion, routing and the two router methods: path validation before the call, `unsupported` without `contents`, the host-side 1 MiB check, existing retry rules. Verify with `cargo test -p df-plugins` for the "Path validation", "Forge without contents support", "File size limit" (host check via the probe) and "Project repository only" scenarios.

## 3. Forgejo plugin

- [ ] 3.1 Write failing tests, then implement revision resolution (design D2). Verify with recorded-response tests for branch, tag, commit and an unknown branch (`not-found`).
- [ ] 3.2 Write failing tests, then implement `list` (design D4): kinds, sizes, sorting, cut at 1,000 with `truncated`, file path → `invalid-input`. Verify with recorded-response tests for the root listing, symlink/submodule entries and a 1,500-entry directory.
- [ ] 3.3 Write failing tests, then implement `get-file` (design D3): base64 decode, `too-large` from `size` before decoding, symlink target, submodule marker, directory → `invalid-input`. Verify with recorded-response tests for a binary file, a symlink, a 2 MiB file and a directory path.

## 4. GitHub plugin

- [ ] 4.1 Write failing tests, then implement revision resolution with the sha media type (design D2). Verify with recorded-response tests for `heads/`, `tags/`, a commit and an unknown branch.
- [ ] 4.2 Write failing tests, then implement `list` (design D4): exactly 1,000 entries → `truncated`. Verify with recorded-response tests for the root listing and the 1,000-entry case.
- [ ] 4.3 Write failing tests, then implement `get-file` (design D3), including the followed-symlink detection by path mismatch and the blob read of the link target. Verify with recorded-response tests for a binary file, a symlink to a file, a symlink to a directory, a 2 MiB file and a submodule.

## 5. Docs

- [ ] 5.1 Update the forge plugin reference `crates/df-plugins/docs/configuration.md`: the `contents` capability, its limits (1,000 entries, 1 MiB) and the token permissions it needs (Forgejo: covered by `write:repository`; GitHub: repository contents read). Check `docs/content/integrations/forges/_index.md` setup steps against the same permissions. Verify with `just docs-build`.

## 6. Integration

- [ ] 6.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, `cargo machete`, `cargo test --workspace` (container-backed tests: report which couldn't run) and `openspec validate forge-repo-contents --strict`. Verify all pass.

## Workflow follow-up

- Archive the change after review; ADR-0059 and the glossary entries (**Base branch**, **Revision**) are already written.
