# Tasks

Each group is one commit and leaves the workspace green. TDD for the xtask checks: write the unit tests (temp dir fixtures with `tempfile`) first.

## 1. Bazel lint parity

- [x] 1.1 Add `extract_cargo_lints(name = "lints", …)` to every `crates/*/BUILD.bazel` and `plugins/*/BUILD.bazel` and `lint_config = ":lints"` to every Rust target (design D1); verify `bazel --bazelrc=.bazelrc.agents query 'kind("rust_(library|binary|test|shared_library)", //crates/... + //plugins/...)'` lists only targets with the attribute (`--output=build | grep -c lint_config` equals the target count)
- [x] 1.2 Drop `-Dclippy::too_many_lines` from `build:lint` in `.bazelrc` (keep `-Dwarnings`) and update its comment; verify `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/... //plugins/...` passes, then fails with a temporary 101-line function (clippy part of `LintsInfo` reaches the aspect) and, separately, with a temporary `unsafe {}` block in a `crates/*` crate (rustc part); revert both. If the 101-line check passes, stop and report BLOCKED (design D1)
- [x] 1.3 Add the `df-harness` unit-test and `tests/smoke.rs` targets and `homelab` to the `df-plugins` integration test list (design D2), none tagged `manual`; verify `bazel --bazelrc=.bazelrc.agents test //crates/df-harness/... //crates/df-plugins/...` passes and the targets appear in the query of 1.1
- [x] 1.4 List every workspace member's `Cargo.toml` in `crate.from_cargo` `manifests` in `MODULE.bazel`; regenerate `MODULE.bazel.lock` as described in the `rust` skill; verify `bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error //crates/... //plugins/...` passes

## 2. xtask lints

- [x] 2.1 Add a `[lints]` table to `tools/xtask/Cargo.toml` mirroring `[workspace.lints]` (design D3), fix any finding; add the xtask clippy run to `pre-push` in `lefthook.yml`; verify `cargo clippy --manifest-path tools/xtask/Cargo.toml --all-targets -- -D warnings` passes

## 3. `cargo xtask check-crates`

- [x] 3.1 Implement `check-crates` (design D4) in its own module of `tools/xtask`, with unit tests per rule (missing `workspace = true`, plugin table missing a workspace clippy lint, target without `lint_config`, `tests/*.rs` without target, missing manifest entry, valid crate passes); verify `cargo test --manifest-path tools/xtask/Cargo.toml` passes and `cargo xtask check-crates` passes on the repository
- [x] 3.2 Extend `check-crates` to compare the xtask `[lints]` table with the workspace table (design risk "drift"), with a test; verify the check passes on the repository and fails after removing a lint from the xtask table (reverted)

## 4. `cargo xtask check-file-length`

- [x] 4.1 Implement `check-file-length` (design D5) with unit tests (over limit, allowlisted, stale entry, missing file, empty reason, non-source extension ignored, explicit file arguments); add `[workspace.metadata.file-length]` with `crates/df-k8s/src/crd.rs` to the root `Cargo.toml`; verify `cargo xtask check-file-length` passes on the repository and fails for a temporary 1,501-line `.rs` file (reverted)

## 5. `cargo xtask check-test-names`

- [x] 5.1 Implement `check-test-names` (design D6) with unit tests (`#[test]`, `#[tokio::test(flavor = …)]`, attribute followed by other attributes before `fn`, each forbidden word, a word only as a substring such as `test_error_kind` passes); verify `cargo xtask check-test-names` passes on the repository

## 6. Hooks and CI

- [x] 6.1 Add the three checks to `pre-commit` in `lefthook.yml` (design D7); verify `lefthook run pre-commit --all-files` passes
- [x] 6.2 Add a CI step to `.github/workflows/ci.yaml` that runs the three checks and the xtask clippy run with the rustup toolchain (design D3, D7); verify locally with the same commands (`cargo xtask check-crates` etc.)

## 7. Documentation and agent rules

- [x] 7.1 Update `AGENTS.md` (design D8): enforced limits and allowlist in "File structure", new section "Adding a crate", lint source rule in "Function length"; verify each command named in the text runs as written
- [x] 7.2 Update `.claude/skills/rust/SKILL.md` with "Adding a crate" and "Lints" and extend its description (design D8); verify it does not repeat `AGENTS.md` but points to it
- [x] 7.3 Add the test-naming check to `.claude/agents/standards-reviewer.md` and the fix-list rule to step 6 of `.claude/skills/implement-change/SKILL.md`; verify the wording matches `AGENTS.md`

## 8. Integration

- [x] 8.1 Run `cargo clippy --all-targets --all-features -- -D warnings`, the xtask clippy run, the three xtask checks, `bazel --bazelrc=.bazelrc.agents build --config=lint //crates/... //plugins/...`, `--config=fmt`, and `bazel --bazelrc=.bazelrc.agents test //crates/df-harness/... //crates/df-plugins/...`; verify all pass

## Workflow follow-up

- Implement `split-long-functions` next, then `clippy-strictness` (it relies on Bazel reading the lints from the manifests).
