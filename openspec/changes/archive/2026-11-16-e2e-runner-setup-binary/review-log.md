# Review log: e2e-runner-setup-binary

- BASE: 0d92cf3858cd62e2b9c7adb0ba556d9f82a182dc
- Store: none
- Status: done
- Implementer agent: ac749b0eb59095164
- DOCKER_HOST: unix:///var/run/docker.sock
- Branch: refactor/e2e-runner-setup-binary
- Tech debt created: none (no P2 findings)

## Round 1 (2026-11-16)

- Implementer: 615c4b67, tasks 13/13, checks fmt/clippy/machete/file-length/test-names/shellcheck/bazel build/docs-build ok, container tests ran (seed, demo_seed, unlanded_lookup); e2e fresh 118 s (main 121 s), kept 65 s (main 63 s), old-format e2e-state reused
- Reviewed range: BASE...615c4b67
- Security: 0 findings (verify step skipped)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P2 | Spec | Blocking | `crates/df-e2e/src/setup/forgejo/tests.rs` | Scenario "Repository reset with a repository missing" has no test (both deletes always answer the same status) | fix-now | fixed (32bf1416) |
| P1 | Spec | P1 | `deploy/e2e/e2e-local.sh:348,489-490` | `setup()` is `cargo run`, which waits on cargo's build lock held by `build_suite &`, so `setup forgejo` runs after the test build; D5 overlap does not happen (verified in script) | fix-now | fixed (32bf1416) |
| S3+S4 | Standards | P1 | `deploy/e2e/e2e-local.sh:376`, `crates/df-e2e/src/forgejo.rs:400` | Push check deletes the branch with `curl` (D7 keeps it) while `Forgejo::delete_branch` is unused; skill rule says HTTP API → Rust | fix-now | fixed (32bf1416, artifacts 27449684) |
| P3 | Spec | P1 | `docs/demo.md:58` | Step 2 still says the version check runs first | fix-now | fixed (32bf1416) |
| P4 | Spec | P1 | `docs/demo.md:38` | Flowchart reuse edge skips the version check and `setup forgejo` | fix-now | fixed (32bf1416) |
| S1 | Standards | P1 | `crates/df-e2e/src/forgejo.rs:38-46,252-406` | Private items before public ones | fix-now | fixed (32bf1416) |
| S2 | Standards | P1 | three new `tests.rs` | Missing `//!` module doc | fix-now | fixed (32bf1416) |
| S5 | Standards | P1 | `crates/df-e2e/src/forgejo.rs:228-294` | Old `get`/`post` duplicate the new `call` request path | fix-now | fixed (32bf1416) |
| S9 | Standards | P1 | `crates/df-e2e/src/forgejo/tests.rs:505` | Test name claims delete + missing, only 404 mocked | fix-now | fixed (32bf1416) |
| S6 | Standards | P1 (user) | `crates/df-e2e/src/forgejo.rs:369-402` | Repo path prefix repeated | fix-now | fixed (32bf1416) |
| S7 | Standards | P1 (user) | `crates/df-e2e/src/setup/forgejo.rs:690-709` | Credentials tuple `.0`/`.1` | fix-now | fixed (32bf1416) |
| S8 | Standards | P3 | `crates/df-e2e/src/setup/versions.rs:948-961` | `pub` only for tests | — | cosmetic |
| S10 | Standards | P3 | `crates/df-e2e/src/setup/forgejo/tests.rs:767` | Param `p` | — | cosmetic |
| P5 | Spec | Rejected | `docs/demo.md:256` | Unrelated reword | — | rejected (needed to pass the kingfisher pre-commit hook on the staged file) |

User decision: fix Blocking + P1, plus S6 and S7 (moved from P3). S8, S10 stay cosmetic.

## Round 2 (2026-11-16)

- Implementer: 32bf1416, tasks 13/13, checks fmt/clippy/machete/file-length/test-names/shellcheck/docs-build ok, container tests ran (seed); kept-cluster setup 64 s, no cargo lock wait
- Reviewed range: 615c4b67...32bf1416
- Round 1 items P2, P1, P3, P4, S1, S2, S5, S6, S7, S9 verified fixed in 32bf1416; S3+S4 fixed in code and skill, artifacts lag (P6)
- Security: 0 findings. Spec: `merge_pull` body in error unchanged from BASE, not a finding.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P6 | Spec | P1 | `openspec/changes/e2e-runner-setup-binary/{design.md:78,proposal.md:19,tasks.md:8}` | Artifacts still list `delete_branch`, removed in 32bf1416 | fix-now | fixed (27449684) |
| S13+P7 | Standards+Spec | P1 | `deploy/e2e/e2e-local.sh:349` | `setup()` hard-codes `${CARGO_TARGET_DIR:-$root/target}/debug/setup`; a `build.target-dir` in a user cargo config is ignored (fails loudly) | fix-now | fixed (27449684) |
| S11 | Standards | P1 | `crates/df-e2e/src/setup/forgejo/tests.rs:157-168` | Third copy of the "existing users" mock block | fix-now | fixed (27449684) |
| S12 | Standards | P1 | `crates/df-e2e/src/setup/forgejo/tests.rs:34-36` | `[u16; 2]` duplicates `FORGE_REPOSITORIES` length | fix-now | fixed (27449684) |

User decision: fix all P1 (P6, S13+P7, S11, S12).

## Round 3 (2026-11-16)

- Implementer: 27449684, tasks 13/13, checks fmt/clippy/machete/test-names/file-length/shellcheck/openspec validate/docs-build ok; no container or e2e rerun (only test helpers and the target-dir lookup changed)
- Reviewed range: 32bf1416...27449684
- Round 2 items P6, S13+P7, S11, S12 verified fixed in 27449684. Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8 | Spec | P3 | `openspec/changes/e2e-runner-setup-binary/design.md:58` | D2 says `state_value` keeps one caller; proposal and code removed it (manifest-hash read inline). Untouched text this round → non-blocking | fix inline by orchestrator | fixed (finish commit) |
| S14 | Standards | P3 | `crates/df-e2e/src/setup/forgejo/tests.rs:36` | `mock_users(server, exist: bool, …)` bool flag at call sites | — | cosmetic |

User decision: finish, fix P8 inline.

## Finish (2026-11-16)

- Container tests: ran in rounds 1 and 2 (seed, demo_seed, unlanded_lookup); none skipped.
- Bazel build `--lockfile_mode=error //crates/... //plugins/...`: ok in round 1; no `Cargo.toml`/`Cargo.lock` change after.
- `just docs-build`: ok. Published docs (`docs/content/`, mounted crate references): no change, nothing user- or operator-facing changed. Contributor docs changed: `docs/demo.md`, `.agents/skills/e2e/SKILL.md`.
- `openspec validate e2e-runner-setup-binary`: valid.
- Tech debt: none. Roadmap: no deferred work beyond the proposal's rejected out-of-scope items.
