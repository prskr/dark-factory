# Review log: ui-model-provider-page

- BASE: 584e18c7e20e4eadc4c31bbf9bd56b90fdc1f68b (moved from a90a185 after the artifact update, so the review covers only the implementation)
- Store: none
- Status: untested
- Implementer agent: ae07d5ffa02a03320
- Design commit: b89c44a
- DOCKER_HOST: unix:///var/run/docker.sock
- Step: finished (7)

## Design session (2026-11-11)

All design tasks (1.1 to 1.5) approved by the user. Boards:

- 1.1, "Config - Model providers": "Model providers · settings page, general", "…, models, saved", "…, model errors" (1440 px). Models tab: one line per model across the panel width (Model ID fills, Wire API 220 px, Context and Max output 140 px, remove), full list without inner scroll. The old "edit, …" boards are deleted. Phone case dropped by the user (artifacts updated in 584e18c).
- 1.2, "Tasks - Details Sidebar": "Desktop · description collapsed" (› chevron, no body) and "Desktop · description expanded" (⌄ chevron, three-line clamp); Project and Persona values underlined as links in both.
- 1.3, "Tasks": "Desktop · user message with code" and "Preview only · user message with code · dark". Inline code in the user bubble uses `color.panel` (`bg-panel`), not `bg-code-on-agent` (task 5.3); blocks 12 px apart.
- 1.4: mapping rows in `docs/design-workflow.md` updated; all 102 board names verified against PenPot.
- 1.5, "Config - Projects": tab label "Security" on all 20 project settings boards; boards renamed "…, security" and "…, security, invalid glob".

## Round 1 (2026-11-11)

- Implementer: a730adf, tasks 22/24 (open: 7.2 visual check, 8.4 docs-build), checks: fmt, clippy, machete, check-file-length, check-test-names ok; `cargo test -p df-web` 772 pass, 33 ignored (compiled assets); container tests ran (docker socket); e2e not run (no kind in its env). `cargo deny` advisories fail on RUSTSEC-2026-0486 (xattr via playwright-rs), not caused by this change (Cargo.lock only adds shlex).
- Orchestrator: Bazel works without the remote cache (`--remote_cache= --bes_backend=`); `bazel build --lockfile_mode=error //crates/df-web` ok after regenerating MODULE.bazel.lock (bc71cdd). `bazel run //:bazel_env` provides hugo/swc/tailwindcss/kind; implementer asked to run docs-build, asset tests, e2e and the 1440 px check.
- Reviewed range: 584e18c...bc71cdd
- Implementer follow-up: 267584a (test marker fix found once the compiled assets were present). `just docs-build` ok (8.4). `cargo test -p df-web` 804 pass, 0 ignored with `.bazel_env/bin` on PATH. `just e2e-local` fails before the tests run: Forgejo deployment never becomes ready in kind (`Deployment/forgejo/forgejo not ready … context deadline exceeded`; the sandbox network also fails ALPN to BuildBuddy) — environment, e2e tests compile but are unrun. `just serve-local`: only the empty `/model-providers` page screenshotted; 7.2 board comparison not done. RUSTSEC-2026-0486 also fails on 584e18c.
- Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `config_pages/providers/models.rs:170-210` | Page widths (`w-55`/`w-35`) and removed `max-h-96` inner scroll also apply to the "New model provider" modal; Model ID shrinks to ~100 px | fix-now | fixed (34ad69b) |
| P2 | Spec | P1 | `config_pages/providers/tests.rs:1340`, `df-e2e/tests/ui.rs` | "Save stays on the page": added model not asserted in the re-rendered form | fix-now | fixed (34ad69b) |
| P3 | Spec | P1 | `specs/task-ui/spec.md:4` | Spec said the header "ends with" the chevron; code and board put it first | fix-now (orchestrator) | fixed (spec wording, uncommitted) |
| S6 | Standards | P1 | `config_pages/push_policy/tests.rs:197,384` | Test names still say "push checks" | fix-now | fixed (34ad69b) |
| S1 | Standards | P1 | `providers/tests.rs:1224`, `tests/project_page.rs:9` | `open_tab`/`tab_labels` helpers duplicated | fix-now | fixed (34ad69b) |
| S2 | Standards | P1 | `providers/page.rs:863`, `project_page.rs:103` | Out-of-band saved-status markup duplicated | fix-now | fixed (34ad69b) |
| S7 | Standards | P1 | `config_pages/providers/tests.rs` (1,455 lines) | Near the 1,500-line test-file limit | fix-now | fixed (34ad69b) |
| S8 | Standards | P1 | `providers/page.rs:824` | `PageQuery`: unused `Default`, `pub(super)` | fix-now | fixed (34ad69b) |
| S3 | Standards | P3 | `providers.rs:731` | Edit link repeats `project_link` | cosmetic | cosmetic |
| S4 | Standards | P3 | `providers.rs:467,791,826` | `""` sentinel for "no tab requested" | cosmetic | cosmetic |
| S5 | Standards | P3 | `layout.rs:195` (reviewer cited 1584) | `Nav` match repeats the settings-page set | cosmetic | cosmetic |
| U1 | User | P1 | `config_pages/tests/mod.rs` (1,470 lines) | Near the 1,500-line test-file limit (user asked for a split; `layout.rs` is only 670 lines) | fix-now | fixed (34ad69b) |

User decision: Fix blocking + P1, plus split `config_pages/tests/mod.rs` into topic files (U1).

## Round 2 (2026-11-11)

- Implementer: 34ad69b (fix round 1), checks: fmt, clippy, machete, check-file-length, check-test-names ok; `cargo test -p df-web` 806 pass, 0 ignored (container tests via the socket); `cargo check -p df-e2e --tests` ok; e2e and 7.2 still not run. Implementer removed the serve-local containers it started, including `dark-factory-playwright-1` (the Playwright MCP disconnected afterwards).
- Reviewed range: 9f1e996...34ad69b. Round-1 rows all fixed (verified by both reviewers); test moves: 88 → 89 tests, no test lost. Security: 0 findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S9 | Standards | Blocking | 10 new files in `config_pages/tests/` and `providers/tests/` | No `//!` module doc comment (AGENTS.md "File structure") | fix-now | fixed (b6d1508) |
| S10 | Standards | Blocking | `config_pages/providers/tests/lifecycle.rs` | Holds custom-header, persona-form and dialog-width tests that are not lifecycle tests | fix-now | fixed (b6d1508) |
| S11 | Standards | Blocking | `config_pages/tests/persona_pages.rs`, `tests/projects.rs` | `persona_pages.rs` duplicates the topic of `personas.rs`; adapter-command test outside `adapter_command.rs` | fix-now | fixed (b6d1508) |
| P4 | Spec | Blocking | `config_pages/providers/tests/models.rs:345` | Modal-layout test misses the preset and 422 create re-renders | fix-now | fixed (b6d1508) |
| S12 | Standards | P1 | `config_pages/providers/tests/mod.rs:49-63` | `Rows`/`with_models` used only by `models.rs` | fix-now | fixed (b6d1508) |
| S13 | Standards | P1 | `config_pages/providers/models.rs:181-214` | `wide` checked in three places | fix-now | fixed (b6d1508) |

User decision: Fix blocking + P1.

## Round 3 (2026-11-11)

- Implementer: b6d1508 (fix round 2), checks: fmt, clippy, machete, check-file-length, check-test-names ok; `cargo test -p df-web` 806 pass, 0 ignored (container tests via the socket). No containers started or removed.
- Reviewed range: 66e5989...b6d1508. Round-2 rows all fixed (verified by both reviewers); moves verbatim, 750 test functions before and after. Security: 0. Spec: 0.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S14 | Standards | Blocking (convergence rule; nit) | `config_pages/tests/personas.rs:1` | `//!` comment no longer covers the merged create/audit and card tests | fix-now | fixed (4fab6ff) |
| S15 | Standards | P3 (user moved from Blocking) | `config_pages/providers/models.rs:193` | `panel(…, wide: bool)` could take the `Layout` value | cosmetic | cosmetic |

User decision: fix S14 (fix round 3 of 3), S15 cosmetic.

## Finish (2026-11-11)

- Fix round 3: 4fab6ff (S14, one `//!` line; verified by diff). `cargo test -p df-web personas` 22 pass; fmt and clippy ok.
- Bazel: `bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error --remote_cache= --bes_backend= //crates/... //plugins/...` ok (the BuildBuddy remote cache fails ALPN in this sandbox).
- Docs: `just docs-build` ok. Pages changed: `docs/content/using/model-providers.md`, `docs/content/using/tasks.md`, `docs/content/using/setup.md`, `docs/content/using/hidden-paths.md`, `docs/content/security/git-proxy.md`; references `crates/df-web/README.md`, `crates/df-gateway/README.md`.
- Container-backed tests: all df-web DB tests ran via `unix:///var/run/docker.sock`.
- Not run (status `untested`, user chose "Finish untested"): the e2e tests of task 3.5 in `crates/df-e2e/tests/ui.rs` (provider settings page saves in place with the added model; `?tab=models` save with an `http://` upstream URL opens General; project settings `?tab=security` with an `ssh://` repository URL; `?tab=security` after choosing Security) — `just e2e-local` fails before the tests because Forgejo never becomes ready in kind here; CI runs them. The 1440 px visual check against the PenPot boards (task 7.2). Task 3.5 is ticked (its tests are written; the task allows "CI runs it" without a cluster); task 7.2 stays open. Tasks 23/24.
- Tech debt: none (no P2 findings). Roadmap: none (all non-goals are "rejected" in design.md).
- Findings: fixed P1, P2, P3, P4, S1, S2, S6–S14, U1; cosmetic S3, S4, S5, S15; rejected none. Security: 0 in all rounds.
