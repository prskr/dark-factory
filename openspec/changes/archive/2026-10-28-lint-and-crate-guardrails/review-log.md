# Review log: lint-and-crate-guardrails

- BASE: 1690408f0115523b93bc1a87da3752e3a03756f0
- Store: none
- Status: done
- Implementer agent: one implementer subagent for all rounds

## Round 1 (2026-10-28)

- Implementer: 5b4e791 (groups 6ec262b, df6e264, 5a22e6d, 0118f0c, 83bd6f6, 8462d9f, 5e6869a, 5b4e791), tasks 17/17, checks fmt/clippy (cargo, xtask, Bazel lint)/machete ok, xtask 47 tests, Bazel df-harness+df-plugins 7/7; container tests ran for df-harness/df-plugins only (other crates: BUILD-only changes)
- Implementer deviations: D6 dropped bare `review`/`round` from the forbidden words (9 legitimate tests matched); CI xtask clippy uses rustup cargo (Bazel `//:cargo -- clippy` fails E0463), untested in CI; TDD red phase not run for check-file-length and check-test-names
- Reviewed range: BASE...5b4e791

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | P1 | `tools/xtask/src/check_test_names.rs:96-116` | Test names are topics, not behaviors | rename | fixed (aecc1b1) |
| S2 | Standards | P1 | `.claude/skills/rust/SKILL.md:60` | "never `#[allow]`" contradicts AGENTS.md test-module exception | align | fixed (aecc1b1) |
| S3 | Standards | P1 | `AGENTS.md:151` | "Never add lint flags to `.bazelrc`" vs kept `-Dwarnings` | add "beyond `-Dwarnings`" | fixed (aecc1b1) |
| S4 | Standards | P1 | `AGENTS.md:146` | Extension list misses `yml`; missing-file/empty-reason failures undocumented | complete doc | fixed (aecc1b1) |
| S5+S6+S7 | Standards | P1 | `tools/xtask/src/check_*.rs` | Duplicated files-or-tracked/report logic, `tracked_files` in wrong module, TOML lookup reimplemented | shared module | fixed (aecc1b1) |
| S8 | Standards | P1 | `tools/xtask/src/check_crates.rs:41-46` | `read_dir` errors silently skipped; a broken tree passes | propagate non-NotFound | fixed (aecc1b1) |
| SP4 | Spec | P1 | `lefthook.yml:45` | `**/Cargo.toml` may not match root `Cargo.toml` | add `'Cargo.toml'` | fixed (aecc1b1) |
| SP6 | Spec | P1 | `tools/xtask/src/check_crates.rs:88-90` | Plugin without `[lints.rust]` (unsafe_code) passes | check rust table too | fixed (aecc1b1) |
| SP3 | Spec | P1 | `crates/df-{web,proto}/BUILD.bazel` | `cargo_build_script` targets lack `lint_config` | set it if accepted, else document gap in design.md | fixed (gap recorded in design D1) |
| SP1 | Spec | P1 | design.md D6, proposal.md | Forbidden words changed (bare review/round dropped), artifacts not updated | accept deviation; /opsx:update | fixed (artifacts updated) |
| SP2 | Spec | P1 | design.md D3/D7, task 6.2 | CI xtask clippy uses rustup cargo; untested in CI | accept deviation; /opsx:update; confirm on first PR | fixed (artifacts updated; CI unconfirmed) |
| S9 | Standards | P3 | `tools/xtask/src/check_crates.rs:73-80` | bool param + data clump in `check_member` | — | cosmetic |
| SP5 | Spec | P3 | spec.md lint scenarios | Only manual negative check in task 1.2 | — | cosmetic |
| TDD | Standards | — | xtask check-file-length/test-names | Red phase not run (declared); tests exist, cannot be redone | — | rejected (not actionable) |

User decision: Fix blocking + P1 (SP1/SP2 via /opsx:update after the fix round)

## Round 2 (2026-10-28)

- Implementer: aecc1b1, tasks 17/17, checks fmt/clippy (workspace, xtask, Bazel lint)/machete ok, xtask 50 tests; container tests not needed (xtask, docs, lefthook only)
- SP3: `cargo_build_script` rejects `lint_config` (no such attribute); BUILD files reverted
- Reviewed range: 5b4e791...aecc1b1

Ledger check: S1-S8, SP4, SP6 confirmed fixed by both reviewers. Spec review: 0 new findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S10 | Standards | P1 | `tools/xtask/src/check_crates.rs:155` | Missing `unsafe_code` in a plugin reports "must contain `unsafe_code = \"forbid\"`", but any level is accepted | presence-only message + test asserting it | fixed (d62d3e7) |
| S11 | Standards | P1 | `AGENTS.md:151,159` | Plugin `[lints.rust]` comparison (all equal, `unsafe_code` presence only) undocumented | document | fixed (d62d3e7) |
| S12 | Standards | P3 | `tools/xtask/src/check_crates.rs:134-140` | `any_value` 6th positional param (extends S9) | — | cosmetic |
| S13 | Standards | P3 | `tools/xtask/src/check_file_length.rs:41-52` | Non-string allowlist reason on over-limit file yields two violations; not a regression | — | cosmetic |

User decision: Fix blocking + P1 (S10, S11)

## Round 3 (2026-10-28)

- Implementer: d62d3e7, checks fmt/xtask clippy/xtask 50 tests/check-crates ok; container tests not needed
- Reviewed range: aecc1b1...d62d3e7 (14 lines; checked by the orchestrator, no reviewer agents spawned). S10, S11 confirmed fixed, no new findings.
- Finish: `bazel build --lockfile_mode=error //crates/... //plugins/...` ok. No P2 findings, no TD entries.
- Open for `/opsx:update` (artifacts only, not blocking): SP1 (D6 word list, proposal), SP2 (D3/D7/task 6.2 CI clippy via rustup; confirm on first PR), SP3 (record gap: `cargo_build_script` has no `lint_config`, build scripts of df-proto and df-web not linted by Bazel).
