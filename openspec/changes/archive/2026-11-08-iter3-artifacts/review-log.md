# Review log: iter3-artifacts

- BASE: f3debb9 (parent of the design commit; moved from 3ef0965 because three unrelated commits landed before the design commit)
- Store: none
- Status: done
- Implementer agent: a6bdbd0010e8cf67a (iteration 1 started 2026-11-07, DOCKER_HOST=unix:///var/run/docker.sock)

## Design (2026-11-07)

- PenPot page `Tasks - Artifacts` (8 boards), `Icon button` variant `Maximize` (Desktop, Default and Hover), roadmap board `Later · details tabs, artifacts` deleted. Approved by the user.
- Mapping rows in `docs/design-workflow.md`: design commit (see `git log --grep "wip(iter3-artifacts): design"`).
- Design decisions that the artifacts must take over before implementation (`/opsx:update`):
  - The Artifacts tab (and the details tab bar) appears only once the task has an artifact; without artifacts the details column keeps its "Details" header. The live reload brings the tab in with the first `write_artifact`. Replaces the `task-ui` scenario "No artifacts".
  - Opening an artifact in the split view collapses the details column to its strip.
  - The artifact header of the split view has an "Open full page" button (icon `Maximize`) next to Close; it opens `/tasks/{key}/artifacts/{name}`.
  - Source of a Markdown artifact is highlighted (syntect, Markdown syntax).
  - Image artifacts have no Preview/Source toggle; the header shows the pixel size.
  - Card previews show the first lines as plain mono text.
- Next step: implementation starts after `/opsx:update`; the skill resumes from this ledger.

## Round 1 (2026-11-07)

- Implementer: debe2ee (domain, storage, runtime, factory tools), 7e7ae83 (artifact view, task page, docs, e2e); HEAD_1 = 7e7ae83. Tasks 28/28. Checks: fmt, clippy (`--exclude df-e2e`), machete, check-file-length, check-test-names, check-crates, Helm lint and unittest, hooks, `just docs-build`, Bazel tests of 6 crates with `--lockfile_mode=error` pass. 7e7ae83 was amended with `--no-verify` only to tick task 2.4 (hooks had passed on the same tree).
- Container tests: ran df-db, df-agent-channel, df-web with `/var/run/docker.sock`. Skipped: `df-e2e` (does not compile here, openssl-sys needs system openssl; needs the kind cluster), including the new RS delegation test; not clippy-checked.
- Browser check (6.3): Playwright against a temporary TestApp at 1440 and 390 px; PenPot not reachable, only a partial visual comparison.
- Reviewed range: f3debb9...7e7ae83

Security: 0 findings (find mode); no verify step.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/scripts/artifactSplit.ts:70` | Between 768 and 1279 px opening an artifact leaves the details slide-over open instead of collapsing it; 900 px was not checked | fix | fixed (d000077) |
| P2 | Spec | Blocking | `crates/df-web/src/artifacts/view.rs:240` | Pane has a fixed px width, so a collapsed task list gives all freed width to the transcript ("Room for an artifact") | fix | fixed (d000077) |
| P8 | Spec | Blocking | `docs/content/using/artifacts.md:34`, `product/agent-playbook.md:47`, `contract/mcp/factory-tools.v2.json:427` | Docs and tool description promise reading the base task; `readable_task` has no base-task clause yet | fix | fixed (d000077) |
| P4 | Spec | Blocking | `crates/df-db/src/tasks/artifact_tests.rs:154` | "Cancelled task" test does not check that content stays readable | fix | fixed (d000077) |
| P9 | Spec | Blocking (finish) | `crates/df-e2e/tests/delegation.rs:72` | e2e step 5 never compiled or ran | finish step 1 | fixed (e2e passed in CI) |
| S1 | Standards | P1 | `crates/df-web/src/components/icons.rs:134` | `#[expect(too_many_lines)]` reason names no follow-up | fix | fixed (d000077) |
| S3 | Standards | P1 | `crates/df-web/src/artifacts/routes.rs:89` | Reimplements `auth::is_htmx` | fix | fixed (d000077) |
| S9 | Standards | P1 | `crates/factory-agent/src/runtime_socket.rs:257` | Stale name `content_strings` | fix | fixed (d000077) |
| S11 | Standards | P1 | `crates/df-agent-channel/src/tools/artifacts.rs:200` | `text_block` duplicates the block built in `tools.rs` `result()` | fix | fixed (d000077) |
| S13 | Standards | P1 | `crates/factory-agent/src/runtime_handler.rs:116` | Test always waits a 200 ms timeout | fix | fixed (d000077) |
| P3 | Spec | P1 | `crates/df-web/src/artifacts/view.rs` `artifact_row` | Image rows use the `FileText` icon | fix | fixed (d000077) |
| P6 | Spec | P1 | `crates/df-web/src/artifacts/tests.rs:135` | No test for "Markdown with a diagram" | fix | fixed (d000077) |
| P10 | Spec | P1 | `openspec/changes/iter3-artifacts/design.md:153` | D9 says the list collapse state is in `artifactSplit.js`; it is in `workspace.ts` | fix | fixed (d000077) |
| P11 | Spec | P1 | `crates/df-agent-channel/src/tools.rs` `audit_details` | Refused-read audit entry records only `is_error`, not the error text | fix | fixed (d000077) |
| S2 | Standards | P2 | `crates/df-db/src/artifacts.rs:20` | `kind: String` compared with string literals instead of `ArtifactKind` | matters when a third kind is added | open |
| S7 | Standards | P1 (user) | `crates/df-web/src/artifacts/view.rs:136` | Details tablist does not reuse `tabs.ts` (no arrow, Home or End keys) | matters for keyboard users, and once task-tree adds Subtasks | fixed (d000077) |
| S12 | Standards | P1 (user) | `crates/factory-agent/src/artifact_path.rs:13` | Size limit and image magic duplicated from df-core | matters when the limits or the types change | fixed (fbc05fd) |
| S14 | Standards | P1 (user) | `crates/df-db/src/tasks/artifact_tests.rs` | Artifact tests live under `tasks/` | matters when the module grows | fixed (d000077) |
| S4 | Standards | P3 | `routes.rs:89-110` | Two `artifact_view` calls differ only in `Place` | - | cosmetic |
| S5 | Standards | P3 | `routes.rs:45,68,130` | Repeated task-load match | - | cosmetic |
| S6 | Standards | P3 | `view.rs:43,248` | Two type-label maps | - | cosmetic |
| S8 | Standards | P3 | `markdown.rs:225` | Bool flag param `by_extension` | - | cosmetic |
| S10 | Standards | P3 | `tools/artifacts.rs:157` | Nested `Result<Result<..>>` | - | cosmetic |
| P5 | Spec | P3 | `artifact_tests.rs:217` | Child cancelled, not completed (same path) | - | cosmetic |
| P7 | Spec | Rejected | - | No chat-specific test | covered by `iter3-chats`, which builds chats | rejected (chats not built yet) |

User decision: Blocking + P1, and also fix S7, S12 and S14 (moved from P2 to fix-now). S2 stays tech debt.

Fix round 1: d000077 fixes P1, P2, P8, P4, S1, S3, S9, S11, S13, P3, P6, P10, P11, S7, S14. S12 BLOCKED: the implementer asked how to share the limits and sniffing between df-core and factory-agent. User decision: a new leaf crate (only `image` and `thiserror`) used by both.

## Round 2 (2026-11-07)

- Implementer: d000077 (round 1 fixes), fbc05fd (new crate `df-artifact-types`, S12); HEAD_2 = fbc05fd. Tasks 28/28. Checks: fmt, clippy, machete, xtask checks, cargo tests with containers, Bazel build `--lockfile_mode=error` and Bazel tests of touched crates, `just docs-build` pass. Browser at 900 and 1280 px. Container tests skipped: `df-e2e` (system openssl missing).
- Reviewed range: 7e7ae83...fbc05fd

Security: 0 findings. Standards and Spec confirm every row marked fixed in round 1; no regressions.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P12 | Spec | Blocking | `crates/df-web/src/task_pages/view.rs:903` | Overview panel lacks `role="tabpanel"` / `aria-labelledby` (S7 fix incomplete); test passes only through `#artifacts-tab` | fix | fixed (bfefda9) |
| P13 | Spec | Blocking | `docs/content/using/artifacts.md:70` | Docs describe only the strip collapse; between 768 and 1279 px the slide-over closes instead | fix | fixed (bfefda9) |
| P14 | Spec | P1 | `factory-tools.v2.json:427`, `agent-playbook.md:47`, `artifacts.md:67` | Lists lost their "and" after the base-task removal | fix | fixed (bfefda9) |
| S15 | Standards | P1 | `crates/df-artifact-types/src/lib.rs:282` | `sniff` guesses the format again after `sniff_kind` discarded it; `map_err` repeated | fix | fixed (bfefda9) |
| S16 | Standards | P1 | `crates/factory-agent/src/artifact_path.rs:13` | Alias `MAX_ARTIFACT_FILE_BYTES` gives the limit a second name | fix | fixed (bfefda9) |
| S17 | Standards | P1 | `crates/df-core/src/artifacts.rs:9` | Glob re-export `pub use df_artifact_types::*` | fix | fixed (bfefda9) |
| S19 | Standards | P1 | `view.rs:236`, `overlay.rs:103` | Initial tab literal `overview` repeated; panel ids `panel-overview` vs `artifacts-tab` | fix (with P12) | fixed (bfefda9) |
| S20 | Standards | P1 | `factory-agent/Cargo.toml:15`, `df-core/Cargo.toml:35` | Dependency entries not sorted | fix | fixed (bfefda9) |
| S18 | Standards | P3 | `df-artifact-types/src/tests.rs`, `df-core/src/artifacts/tests.rs` | PNG/JPEG fixtures copied in both crates | - | cosmetic |
| S21 | Standards | P3 | `artifactSplit.ts:21` | Header comment line over the wrap width | - | cosmetic |
| P15 | Spec | Rejected | `crates/df-web/src/artifacts/tests.rs:455` | "Room for an artifact" has only a string test | tasks.md 6.3 verifies these scenarios in a browser; done at 1280 px | rejected (browser-verified per 6.3) |

User decision: Fix blocking + P1 (P12, P13, P14, S15, S16, S17, S19, S20).

## Round 3 (2026-11-07)

- Implementer: bfefda9 (round 2 fixes); HEAD_3 = bfefda9. Tasks 28/28. Checks: fmt, clippy, machete, xtask checks, typos, cargo tests with containers, Bazel tests `--lockfile_mode=error`, `just docs-build` pass. Browser at 1440, 900 and 390 px for the tabpanels. Container tests skipped: `df-e2e` (system openssl missing).
- Reviewed range: fbc05fd...bfefda9

Security: 0 findings. Standards and Spec confirm P12, P13, P14, S15, S16, S17, S19, S20 fixed; no spec regressions.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P16 | Spec | P1 | `crates/df-web/src/task_pages/view.rs:902`, `crates/df-web/src/artifacts/view.rs:168` | Without artifacts there is no tablist, but both panels still carry `role="tabpanel"` and `aria-labelledby` to tabs that do not exist | fix | fixed (8923845) |
| S22 | Standards | P1 | `crates/df-artifact-types/src/lib.rs:123` | Private `Detected`/`detect` sit between public functions (AGENTS.md "File structure") | fix | fixed (8923845) |
| S23 | Standards | P1 | `crates/df-artifact-types/src/lib.rs:96-118` | Text kind/MIME mapping written twice in `sniff_kind` and `sniff` | fix | fixed (8923845) |
| S24 | Standards | P1 | `crates/df-web/styles/input.css:208` | Comment about the Artifacts tab no longer sits above the rule it describes | fix | fixed (8923845) |

User decision (2026-11-08): fix P16, S22, S23, S24 in one more round; the orchestrator checks that commit itself instead of a full round-4 review, then finish.

Fix round 3: 8923845 fixes P16, S22, S23, S24 (checked by the orchestrator: `adopt()` in `tabs.ts` sets the panel roles when the tab bar first appears; `Detected::kind_and_mime`; private items after public ones). Checks: fmt, clippy, machete, xtask, tsc, cargo tests, Bazel tests of df-artifact-types and df-web, `just docs-build` pass; browser check of the first-artifact case.

## Finish (2026-11-08)

- Bazel build `--lockfile_mode=error //crates/... //plugins/...` passes (110 targets); `just docs-build` passes.
- Docs pages added or changed: `docs/content/using/artifacts.md` (new), `docs/content/using/delegation.md`, `docs/content/using/tasks.md`, `docs/content/architecture/agent-runs.md`, `crates/df-agent-channel/docs/configuration.md`, `crates/factory-agent/docs/claude-acp.md`, `product/agent-playbook.md`.
- Status `untested`: `df-e2e` (including the demo step 5 test in `crates/df-e2e/tests/delegation.rs`) never compiled or ran here (no system openssl, no kind cluster). P9 stays open until the user runs it in another environment.
- S2 (P2) is not yet written to `docs/tech_debt.md`; the user stopped before approving TD-122.
- No new roadmap rows: downscaling is in the backlog; base-task reads are task 2.5 of `iter3-task-tree`.
- Commits squashed onto BASE for the e2e run.

## E2E in CI (2026-11-08)

- `df-e2e` ran in CI on the squashed branch. Two runs first failed on flaky tests outside this change: `follow_up_resumes_the_session_and_completing_marks_the_pr_ready` (the PR is opened by the `pr.sync` outbox job after the status changes; it also failed once on `main` at 3ef0965) and the df-web test `streams_end_when_the_session_token_is_due_for_refresh` (the stream can end before its first event). 5a3c062 makes both wait on the condition. The next CI run passed, including `a_researcher_hands_its_findings_to_the_coordinator_as_an_artifact` (demo step 5). P9 is fixed.
- The e2e script now downloads the cert-manager manifest with `gh release download` (plain `releases/download` URLs returned 504).
- Status `done`. S2 (P2, `ArtifactMeta.kind` as a string) has no `docs/tech_debt.md` entry: the TD-122 proposal was not approved.
