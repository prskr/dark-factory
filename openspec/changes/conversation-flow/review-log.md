# Review log: conversation-flow

- BASE: 3f7b0e3d17e865a0531eed1da5d0def3c01e1860 (rebased onto 1bffe05374ceddd7c5d628a44ac3d1197c8590d5, see "Rebase and PR comments")
- Store: none
- Status: done
- Branch: conversation-flow
- Implementer agent: none for the recovered checkpoint

## Recovery checkpoint (2026-11-06)

- The initial implementation was made without a feature branch or WIP commits. The user requested correction to the repository's implement-change workflow.
- Existing changes were preserved on the `conversation-flow` branch. The checkpoint is `wip(conversation-flow): iteration 1`.
- Tasks: 8/15 complete, covering principal display names, readable UI authors, prompt batch delivery, runtime batching, author lines and their documentation.
- The separately authorized final-report test fix waits for a usable post-outage handshake rather than accepting a buffered Hello from an expired stream. The original test passed ten repeated runs and the full runtime suite.
- Cargo checks completed: df-core 208 tests, df-proto 12 tests, df-agent-channel 318 tests, factory-agent 144 tests with one existing ignored test, df-web 673 tests, fake-acp-agent 24 tests; df-e2e test targets compile.
- The docs build passed through `build/with_tools.sh docs/publish.sh target/docs-site main`; `just` is unavailable locally.
- Container-backed channel and UI tests ran with OrbStack at `unix:///Users/martinhusgen/.orbstack/run/docker.sock`.
- Checkpoint checks passed: Rust formatting, strict workspace Clippy, cargo-machete, Buildifier and all eight affected Bazel test targets.
- The runtime test fixture now uses the helper binaries supplied as Bazel runfiles, with Cargo output paths as fallback; the idle-batch regression executes under both build systems.
- The existing task-view tests moved to `view/tests.rs` to keep `view.rs` below the source-length limit. All 17 view tests, the full UI suite and its Bazel target passed after the move.
- No review round or finding classification has run yet. The checkpoint is not a completion or merge approval.

## Design Approval (2026-11-06)

- Penpot was connected to `App Design`; its expected pages were verified.
- The user approved the Light and Dark comparison of pending/answered questions and pending/approved/denied approvals.
- Styling: existing `sunk` surface and `line-strong` border, 12 px radius, 16 px padding and 12 px gaps. Existing typography, semantic badges and controls are retained.
- The reference boards are on `Tasks`, named `Desktop · conversation flow · <state>`, with the state names recorded in task 5.1 and the design mapping.
- Dark comparison boards are explicitly `Preview only` snapshots using the existing Dark token values; the reference remains token-bound. This avoids the observed export inconsistencies when switching themes in the plugin.
- Tasks: 9/15 complete. The approved design is not yet implemented in the application.

## Implementation Iterations (2026-11-06)

- HEAD_2: a8bd8d3b4fcdd47163b7d1b5a2f6633b9f754ddd (`wip(conversation-flow): iteration 2`). Hidden decision messages, question/approval regressions and published request/task documentation. Tasks 12/15; Cargo df-db 238 and df-web 673 passed, affected Bazel targets 2/2, docs and static checks passed.
- HEAD_3: b864b3e0ee5bdc60b9d7e9947a68a78164cecb23 (`wip(conversation-flow): iteration 3`). Approved request-card frame and five-state render regression. Tasks 13/15; Cargo df-web 674 and its Bazel target passed. Eight browser screenshots cover pending/answered questions, Light/Dark, desktop 1440x1000 and phone 390x844; no overflow or overlap, fonts and assets loaded.
- HEAD_4: e772f8bdf4e90a9e9085b21b692a81947efe49e7 (`wip(conversation-flow): iteration 4`). The full E2E suite passed 16/16. Its conversation-flow case verifies the queued human message and question answer as the first two ordered prompt IDs of one next turn, with actual ACP author lines. Tasks 14/15; task 6.2 remains open.
- All iteration commits used the required author identity and enabled hooks. Container-backed tests ran with OrbStack; the E2E cluster and port-forwards were cleaned up.
- The unfiltered local `bazel --bazelrc=.bazelrc.agents test //...` failed analysis on macOS: no matching `patchelf` toolchain for Linux harness-image targets. No targets were excluded and this gate is not recorded as passed.

## Linux Gate Approval (2026-11-06)

- User decision: push `conversation-flow` and create a draft PR to run the existing Linux CI gate.
- The `bazel` job in `.github/workflows/ci.yaml` runs the full `bazel test //...` on `ubuntu-latest` for pull requests. Its result is pending.
- Draft PR: https://github.com/prskr/dark-factory/pull/141, author and SSH identity verified as `Izzetee`. The published checkpoint is `wip(conversation-flow): verification ledger`.
- Linux CI: https://github.com/prskr/dark-factory/actions/runs/42259318660. Docs and OpenSpec checks passed; Bazel and E2E are still pending.
- No squash, archive or merge was authorized. At this approval, no three-axis review round had run yet.

## Round 1 (2026-11-06)

- Reviewed range: BASE through `wip(conversation-flow): verification ledger`, frozen before review.
- Standards, Spec and Security ran in parallel as the named review agents, with the requested Opus model. Standards: 10 judgement findings, no hard violations. Spec: 2 findings, 5/5 requirements and 14/14 scenarios inspected. Security: no new exploitable vulnerability.
- The orchestrator verified the major upgrade-documentation finding and the doubtful standards findings against the cited code and published references.
- The same-release upgrade requirement is already documented in the published runtime reference. An explicit release-specific upgrade entry is useful and cheap, but its absence is not classified as an entirely undocumented protocol change.

| ID | Axis | Class | Where | Finding | Proposed action | Decision | Status |
|----|------|-------|-------|---------|-----------------|----------|--------|
| P2 | Spec | Blocking | `crates/df-agent-channel/tests/agent_port.rs:410` | D1 promises one ordered multi-prompt batch after reconnect; existing reconnect tests only cover one unconfirmed prompt. | Extend the cross-replica reconnect test with two unconfirmed prompts and assert one batch with both IDs in order. | fix-now | fixed (`wip(conversation-flow): fix round 1`) |
| P1 | Spec | P1 | `docs/content/operations/upgrades.md:45` | The runtime reference documents the protocol break, but the release-specific upgrade guide has no entry for `Prompts`. | Add the new protocol break, same-release runtime/control-plane requirement, pinned-image caveat and active-run handling to the upgrade guide. | fix-now | fixed (`wip(conversation-flow): fix round 1`) |
| S7 | Standards | P2 | `crates/df-agent-channel/tests/agent_port.rs:255` | Repeated single-prompt extraction and batch construction in test fixtures. | Consider shared test helpers when extending these fixtures; preserve explicit cardinality assertions. | defer | tech-debt (TD-119) |
| S2 | Standards | P3 | `crates/df-core/src/domain.rs:480` | Reason-code literals are not shared constants. The open reason vocabulary and fallback are intentional. | Optional constants cleanup, not a new enum or correctness fix. | cosmetic | cosmetic |
| S4 | Standards | P3 | `crates/df-web/src/task_pages/view.rs:6` | Test-only import remains in the parent after extracting the tests. | Optionally move the import into the test module. | cosmetic | cosmetic |
| S5 | Standards | P3 | `crates/df-agent-channel/src/session.rs:485` | The explanatory busy-until-persisted-TurnEnded comment was removed. | Optionally restore the invariant comment. | cosmetic | cosmetic |
| S8 | Standards | P3 | `docs/content/architecture/agent-runs.md:31` | Diagram indentation and paragraph wrapping differ from nearby prose. | Optional local formatting cleanup; docs build already passes. | cosmetic | cosmetic |
| S9 | Standards | P3 | `docs/design-workflow.md:157` | The new mapping omits the fixture source. The key-based route is accurate. | Optionally clarify the manual design fixture; retain `<key>`. | cosmetic | cosmetic |
| S10 | Standards | P3 | `crates/df-e2e/tests/flow.rs:30` | New test and constants have fewer scenario comments than their neighbours. | Optional concise scenario documentation. | cosmetic | cosmetic |
| S1 | Standards | Rejected | `crates/df-db/src/requests.rs:571` | Two text-block helpers exist. | Both helpers predate the diff; the changed decision path correctly reuses the helper accepting `hidden`. Unrelated push-check cleanup is outside this fix. | rejected | rejected (pre-existing duplication, not introduced by this diff) |
| S3 | Standards | Rejected | `crates/df-web/src/transcript/mod.rs:461` | A domain principal is constructed to use its label. | D2 explicitly makes `Principal::display_name` the shared owner; a second public helper is unnecessary. | rejected | rejected (matches the owning abstraction and D2) |
| S6 | Standards | Rejected | `crates/factory-agent/src/runner.rs:580` | Empty-batch guard overlaps the connection filter. | Keep the cheap local no-op guard; it is boundary validation, not speculative abstraction. | rejected | rejected (defensive no-op, no excess generality) |

User decision: fix Blocking + P1. Only P2 (Spec, Blocking) and P1 (Spec, P1) are authorized for Fix round 1. Defer S7 to the final tech-debt preview; retain P3 only in the ledger. The user did not authorize the rejected fixes.

## Fix round 1 (2026-11-06)

- Commit: `wip(conversation-flow): fix round 1`; approved findings P2 and P1 fixed.
- Focused reconnect test: initial GREEN 1/1. A temporary singleton-delivery probe in `send_new_prompts` produced RED 0 passed, 1 failed: the first replay batch contained only the assignment instead of both observed prompts in order. After restoring the sender exactly, GREEN 1 passed, 0 failed; no production-code diff remains.
- Cargo `df-agent-channel`: 318 passed, 0 failed (290 unit, 27 agent-port, 1 spans). All container-backed tests ran with `DOCKER_HOST=unix:///Users/martinhusgen/.orbstack/run/docker.sock`; none skipped.
- Bazel `//crates/df-agent-channel:agent_port_test`: 1/1 target passed with the same socket and its sandbox writable path. Nonfatal BuildBuddy anonymous-cache deprecation warnings did not fail the target.
- Checks passed: `cargo fmt --all --check`, strict Clippy for the touched crate and the workspace, `cargo machete`, `openspec validate conversation-flow --strict`, and `git diff --check`. The formatter's initial pattern-layout failure was repaired locally.
- Published docs build and link checks passed through `mkdir -p target/docs-site && build/with_tools.sh docs/publish.sh target/docs-site main`. Hugo emitted the existing raw-HTML warning on the upgrade page.
- Tasks remain 14/15 complete and status remains in-progress. Task 6.2 still requires actual success of the full Linux Bazel gate; the known macOS-blocked unfiltered gate was not rerun.
- Only the reconnect test, published upgrade entry and ledger checkpoint changed. No other findings, design tasks, UI, configuration or dependencies were changed.

## Round 2 (2026-11-06)

- Reviewed range: `wip(conversation-flow): verification ledger` through `wip(conversation-flow): fix round 1`, including the Round 1 ledger check.
- The named Standards, Spec and Security agents ran in parallel with the requested Opus model. All confirmed P2 and P1 fixed; no new Spec or Security findings. Standards found one additional judgement-only test-fixture duplication.

| ID | Axis | Class | Where | Finding | Proposed action | Decision | Status |
|----|------|-------|-------|---------|-----------------|----------|--------|
| B1 | Verification | Blocking | `//crates/df-plugins:df_plugins_routing_test` | Initial Linux CI built all targets but timed out this unchanged test target after 300 seconds; 50/51 Bazel targets passed. | Run the existing full gate again on the reviewed fix checkpoint. Do not alter test timeouts or exclude targets. If it repeats, diagnose before proposing an additional fix. | retry approved | fixed (Linux CI 42266384689 passed) |
| S11 | Standards | P2 | `crates/df-agent-channel/tests/agent_port.rs:435` | The new test repeats the inline `TurnEnded` fixture shape. | Track with S7 in the test-helper maintenance follow-up; no correctness fix or new abstraction required now. | defer | tech-debt (TD-119) |

- CI source checkpoint: `wip(conversation-flow): verification ledger`, not the later local fix commit.
- CI https://github.com/prskr/dark-factory/actions/runs/42259318660 completed with the full Bazel Test step failed. All 138 build targets completed; 50 of 51 test targets passed. The remaining routing test was `two_forges_side_by_side_each_serve_only_their_projects`; the target exceeded its 300-second budget. No assertion or compilation error was reported, and no test was excluded.
- Linux E2E, Docs and OpenSpec passed for the same source checkpoint. The initial Bazel job did not run its later Clippy and repository-check steps after the Test failure; local strict checks remain separately verified.
- No source fix outside the approved P2 and P1 has been authorized. Task 6.2 remains unchecked; overall status remains in-progress.

User decision: full CI retry and the previewed TD-119 entry approved. Push the reviewed fix checkpoint without changing test timeouts or excluding targets. If the routing timeout repeats, diagnose before asking approval for any additional source fix. S7/S11 are tracked in TD-119; no other follow-up was added.

## Completion (2026-11-06)

- Tasks: 15/15 complete. Two parallel review rounds and one approved fix round completed; no blocking finding remains open.
- Verified source: 12f031e32eee46b64aca21c75ac723be3b098be3 (`wip(conversation-flow): review round 2`). Linux CI https://github.com/prskr/dark-factory/actions/runs/42266384689 passed the unfiltered `bazel test //...`: all 138 build targets completed and 51/51 test targets passed. The unchanged routing test passed without higher timeouts or exclusions.
- The same CI job passed Clippy, Rustfmt, repository checks and xtask Clippy. Local strict workspace Clippy, cargo-machete, cargo deny and strict OpenSpec validation passed separately.
- Linux E2E, Docs and OpenSpec checks passed for the same source checkpoint. The full local E2E suite and the eight browser checks are recorded above. No container-backed tests were skipped; the runtime suite retains one existing manual ignored test.
- Published documentation covers the built behavior in `docs/content/architecture/agent-runs.md`, `docs/content/using/requests.md`, `docs/content/using/tasks.md`, `docs/content/operations/upgrades.md` and the mounted `crates/factory-agent/docs/configuration.md` reference. Docs build and link checks passed.
- No Cargo manifest or lock file changed, so the dependency-change lock regeneration gate does not apply.
- P2 and P1 fixed; S7/S11 deferred as TD-119; six cosmetic findings retained only in this ledger; S1/S3/S6 rejected with reasons. No additional roadmap item was added.
- This completion checkpoint changes only task and review metadata after the verified source checkpoint. The PR remains a draft; no squash, archive or merge was authorized or performed.

## Rebase and PR comments (2026-11-06)

- User decision: rebase onto `origin/main` (1bffe053, 6 commits ahead: plugins, OpenSpec, tech debt) and push with `--force-with-lease`. `git merge-tree` showed no conflicts; overlapping files `assign.rs` and `docs/tech_debt.md` had no semantic collision (TD IDs on main end at TD-118). New BASE: 1bffe05374ceddd7c5d628a44ac3d1197c8590d5.
- PR #141 review by prskr: nine inline comments. Eight fixed in `wip(conversation-flow): PR comments – system reasons and queued author prefix` and `… – test helpers, names and docs`: design-system reference, reason constants in df-core, shared `prompts_body` helper, `let … else` skip, test rename, restored `busy` comment, Mermaid indentation. The D1 reconnect comment was already covered by `reconnect_to_another_replica_resends_assign_and_unconfirmed_prompts` (two prompts, one ordered batch, fix round 1).
- User decision on the scope question: no author prefix on human rows in the queued stack.

## Round 3 (2026-11-06)

- Reviewed range: `origin/main...HEAD` after the rebase and the PR-comment commits. Named Standards, Spec and Security agents ran in parallel with Opus. Security: 0 findings. Spec: 3 findings, 5/5 requirements and 14/14 scenarios checked. Standards: 9 judgement findings; PR comments 1, 2, 4–7 and 9 addressed, 3 partially.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | major | `crates/df-web/src/task_pages/view.rs:375` | The removed human-only filter also lists agent-authored messages; the main spec still says "A human's undelivered messages". | fix-now: keep agent rows, prefix every non-human row, MODIFIED requirement in the delta | fixed (fix round 3) |
| P2 | Spec | minor | `docs/content/using/tasks.md:36` | Claims every stack row shows the author. | fix-now | fixed (fix round 3) |
| P3 | Spec | nit | `docs/design-system.md:190` | Omits agent-authored rows. | fix-now | fixed (fix round 3) |
| S1 | Standards | minor | `crates/df-core/src/task_transition.rs:17` | `task.child_ended` reason outside `system_reason`, shown as fallback. | fix-now: `system_reason::CHILD_ENDED`, "Subtask update" | fixed (fix round 3) |
| S2 | Standards | minor | `crates/df-core/src/domain.rs:438` | Reasons kept in three lists. | fix-now: one `NAMES` table | fixed (fix round 3) |
| S3 | Standards | minor | `crates/df-core/src/domain.rs:508` | Fallback name is a literal. | fix-now: `FALLBACK_NAME` | fixed (fix round 3) |
| S4 | Standards | minor | `crates/factory-agent/src/tests/harness.rs:568` | PR comment 3 incomplete: inline `Prompts` wrappers. | fix-now | fixed (fix round 3) |
| S5 | Standards | nit | `crates/factory-agent/src/tests/runner_runtime.rs:203` | Inline `Prompt` literals. | fix-now | fixed (fix round 3) |
| S6 | Standards | nit | `crates/df-db/src/requests/tests.rs:247` | Duplicated decision-message query. | fix-now | fixed (fix round 3) |
| S7 | Standards | nit | `docs/tech_debt.md:1036` | TD-119 partly resolved by this change. | fix-now: narrow to `df-agent-channel` | fixed (fix round 3) |
| S8 | Standards | nit | `crates/df-web/src/task_pages/view.rs:398` | Feature envy on `Principal`. | fix-now: `Principal::non_human_name()` | fixed (fix round 3) |
| S9 | Standards | nit | `crates/df-web/src/transcript/tests.rs:110` | Two near-identical tests. | fix-now: table-driven | fixed (fix round 3) |

User decision: fix all findings.

## Fix round 3 (2026-11-06)

- Commits: `wip(conversation-flow): fix round 3 – queued stack authors` and `… – reason table and test helpers`.
- Cargo: df-core 211, df-db 238 (container-backed), df-web 674, df-agent-channel lib 301, factory-agent 143 (1 existing ignored) passed. Strict workspace Clippy, cargo-machete, `openspec validate conversation-flow --strict` and the docs build passed.
- Final gate after the fixes: df-agent-channel 329 (incl. 27 agent-port, container-backed), dark-factory 53 (2 existing ignored), df-e2e compiles, strict workspace Clippy, Bazel 10/10 test targets of the affected crates. One dark-factory webhook test first failed with a `forgejo_plugin.wasm` built before the rebase; it passed after rebuilding the plugin (`cargo build --target wasm32-wasip2 -p forgejo-plugin`).

## Next Step

- Push with `--force-with-lease`, wait for CI, then ask before replying to PR comments, squashing the WIP commits or archiving the OpenSpec change. Keep the draft PR unmerged until explicit approval.