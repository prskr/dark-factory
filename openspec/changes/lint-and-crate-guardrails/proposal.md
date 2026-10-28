# Proposal

## Why

The review of `split-oversized-test-modules` showed that the lint setup has gaps that nothing detects:

- Bazel ignores `[lints]` in the Cargo manifests. `unsafe_code = "forbid"` is not enforced in the Bazel build. `too_many_lines` works there only because it is repeated by hand in `.bazelrc`.
- `--config=lint` checks only code that belongs to a Bazel target. The unit tests and `tests/smoke.rs` of `df-harness` and `df-plugins/tests/homelab.rs` have no target, so they are never linted under Bazel.
- `tools/xtask` is its own Cargo workspace without a `BUILD.bazel`, so neither the pre-push clippy run nor the Bazel lint run covers it.

The size limits (1,500 lines per file) and the test naming rule are guidance in `AGENTS.md`, not checks, so they will drift again. Nothing documents how to add a crate so that it is built, tested and linted in both build systems.

## What Changes

- Bazel reads every crate's lints from its `Cargo.toml` (`extract_cargo_lints` from rules_rust plus `lint_config` on every Rust target). The hand-written `-Dclippy::too_many_lines` in `.bazelrc` is removed.
- Missing Bazel targets are added for the `df-harness` unit tests, `df-harness/tests/smoke.rs` and `df-plugins/tests/homelab.rs`.
- `tools/xtask` gets a `[lints]` table and a clippy run in the pre-push hook.
- The `crate.from_cargo` manifest list in `MODULE.bazel` lists every workspace member.
- New `cargo xtask` checks run in the pre-commit hook and in CI:
  - `check-crates`: every workspace member inherits the workspace lints and has a `BUILD.bazel`; every Rust target there sets `lint_config`; every `tests/*.rs` has a Bazel test target; every member is in the `MODULE.bazel` manifest list.
  - `check-file-length`: source files over 1,500 lines fail unless they are on an allowlist with a reason, kept in the root `Cargo.toml`. An allowlist entry for a file that is no longer too long fails too.
  - `check-test-names`: a test named after a review round, finding or tech-debt ID (`review`, `finding`, `r<n>`, `td<n>`, `round<n>`) fails.
- Docs and agent rules:
  - `AGENTS.md`: a checklist for adding a crate, the lint rules, and the checks that enforce the size limits and the test naming rule.
  - The `rust` skill: the Bazel details of adding a crate and of the lint setup.
  - The `standards-reviewer` agent checks test names.
  - The `implement-change` skill forbids naming tests after finding IDs in fix rounds.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `build-pipeline`: adds three requirements: lint rules have one source and apply to every Rust target in both build systems; a file length check; a test name check.

## Impact

- `MODULE.bazel`, `.bazelrc`, every `crates/*/BUILD.bazel` and `plugins/*/BUILD.bazel`, `MODULE.bazel.lock`
- `tools/xtask` (new subcommands, `[lints]`), `lefthook.yml`, CI workflow
- root `Cargo.toml` (`[workspace.metadata.file-length]`)
- `AGENTS.md`, `.claude/skills/rust/SKILL.md`, `.claude/agents/standards-reviewer.md`, `.claude/skills/implement-change/SKILL.md`
- With `unsafe_code` enforced, the Bazel build may reveal violations that Cargo already reports. None are expected, since `cargo clippy` already enforces it.
