# Review log: iter3-review-tools

- BASE: 1a514a1
- Store: none
- Status: done
- Implementer agent: stateless `implementer` subagent per run (VS Code); tasks in batches
- DOCKER_HOST: unix:///var/run/docker.sock
- Design tasks: none

## Implementation runs

| Batch | Tasks | Commits | Notes |
|---|---|---|---|
| 1 | 1.1–1.3, 2.1, 2.2 | 2dd5682, commit "iteration 1, host" | wasm-tools 1.245.0 installed into `target/tools`; plugin stubs return `Error::Internal` until 3.x/4.x; `reply` returns `df_core::Comment`; timestamps stay RFC 3339 strings |
| 2 | 3.1–3.3 | c979bda, 892a6fb | BLOCKED on two design problems found live (Forgejo 15.0.11 container): threads are per (review, path, line); the permission API needs repository admin. Live test `review_reply_joins_the_thread` (ignored, env-driven) in `crates/df-plugins/tests/homelab.rs` passed; `resolver` not verified live; own RFC 3339 parser `timestamp.rs` |

User decisions after batch 2: group Forgejo threads by (review, path, line); the bot's credential needs repository admin (documented). Spec `forgejo-integration` and design updated by the orchestrator.

| 3 | Forgejo grouping, 4.1, 4.2 | f7fc69f, eccc4e0 | Live Forgejo test passed again (two reviews on one line stay two threads); GitHub GraphQL documents checked read-only against github.com; RFC 3339 parser copied into the GitHub plugin; DISMISSED review → `commented` |
| 4 | 5.1–5.3 | 570c079 | Run cut off by a tool timeout after this commit |
| 4b | 5.4 (code), 6.1, 6.2 | 24e549c | fake-acp-agent `mcp` step `save` + `${name}` (delta spec added by the orchestrator); e2e failed twice in cluster setup (`cert-manager-csi-driver` DaemonSet not ready) before any test; `bazel build //...` fails only in `//deploy/images/claude-harness:layer` on the case-insensitive macOS filesystem |

## Round 1 (2026-11-03)

- Implementer: 24e549c, tasks 15/16 (5.4 coded, e2e not reached), container tests ran
- Reviewed range: 1a514a1...702c85d
- Security: no findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `plugins/forgejo/src/api/feedback.rs:~95` | `reply` accepts a valid review id with a made-up path/line and starts a new thread instead of `invalid-input` | | open |
| P2 | Spec | Blocking | `plugins/github/src/api/feedback.rs:18` | `first: 100` returns the oldest items; with >100 items `since` finds nothing new and truncation drops the newest (spec: most recently active kept) | | open |
| S1 | Standards | P1 | `crates/df-plugins/docs/configuration.md:270` | Operator reference still says write access; repository admin (and possibly `read:user` for `GET /user`) missing | | open |
| S2 | Standards | P1 | both plugins' `feedback.rs` | `touched_since` duplicated outside the copied `timestamp.rs` | | open |
| S3 | Standards | P1 | `df-agent-channel/src/tools/feedback.rs:113` | `fit_to_budget` rank indirection and binary search hard to read; sort once and drop oldest | | open |
| S5, S6, S8 | Standards | P1 | seams `AuthorRole::Member`, GitHub `ApiComment`, Forgejo README | Missing doc line, name clash with the REST `ApiComment`, unwrapped README line | | open |
| P4 | Spec | P1 | `tools/feedback/tests.rs:365` | Repeated `call_id` test only for the PR-comment path, not for a thread reply | | open |
| P5 | Spec | P2 | `crates/df-e2e/tests/flow.rs:142` | e2e review is submitted with the bot token: role `member` and the admin requirement are only checked in the ignored homelab test | | open |
| P3 | Spec | P3 | `tools/feedback.rs:44` | Budget counts the structured JSON; the text block mirrors it (MCP convention for structured results) | | open |
| S4, S7 | Standards | P3 | seams, test-probe | `PrComment`/`ReviewComment` identical shape (mirrors WIT); probe returns body as url | | cosmetic |
| E1 | Process | Blocking | task 5.4 | e2e never reached the tests (cluster setup failed twice); must run before done | rerun | fixed (see below) |

User decision: Fix blocking + P1; P5 tech debt (TD-93); P3, S4, S7 accepted.

Status after the fix round: P1, P2, P4, S1, S2, S3, S5, S6, S8 fixed (35187b7); confirmed by the round 2 reviewers. Forgejo review paging (oldest first, 20 pages of 50) stays; `truncated` is set beyond it.

## Round 2 (2026-11-03)

- Implementer: 35187b7, container tests ran
- Reviewed range: 692d947...35187b7
- Security: no findings

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-P1 | Spec | P1 | `design.md:69`, `tasks.md` 4.1 | Still say `first: 100` / `hasNextPage` | fix-now (orchestrator) | fixed (artifacts) |
| R2-P2 | Spec | P1 | `plugins/forgejo/README.md:35` | `reply` row misses the comments lookup | fix-now (orchestrator) | fixed (README) |
| R2-P3 | Spec | P3 | `plugins/github/src/api/feedback.rs:30` | `reviewThreads(last: 100)` picks threads by creation, not activity | accepted, documented in the GitHub README | cosmetic |
| R2-S1 | Standards | P3 | `df-agent-channel/src/tools/feedback.rs:157` | Closure in `assemble` shadows the parameter name `kept` | | cosmetic |

## e2e (E1)

- Before the rebase: 7/9 passed; `rate_limited_task_resumes_at_the_retry_time_without_a_message` failed alone too, `stopping_a_run_…` passed alone. Both outside this change; fixed on main (`fix(run-orchestration): prompt the agent to continue when a task resumes`, `fix(e2e): room for one volume per task in the sandbox quota`).
- After the rebase onto main: 9/9 passed, including the feedback read and thread reply in `follow_up_resumes_the_session_and_completing_marks_the_pr_ready`.

No Blocking or P1 findings left; done.
