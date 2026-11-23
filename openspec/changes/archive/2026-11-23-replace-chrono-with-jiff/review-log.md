# Review log

## Round 1 — plan review (DF-24, Grace Hopper), verdict FAIL

| # | Severity | Finding | Resolution |
|---|---|---|---|
| 1 | major | `sqlx.toml` needs feature `sqlx-toml`; override key is case-sensitive (`TIMESTAMPTZ`) | D2, tasks 2.1, 2.2, 4.1; spot-checked in `sqlx-core` 0.9.0 `config/macros.rs` |
| 2 | major | Spike in `df-db` breaks the crate (override is crate-wide) | Spike in a throwaway crate (2.2); override and full `df-db` swap in one task (4.1) |
| 3 | major | Pin tests cannot run on `chrono` after `df-core` moved | New group 1 before any migration; D6 reworded |
| 4 | major | `to_rfc3339()` prints `+00:00`, `jiff` prints `Z` | Accepted: D4 lists the changed texts (`datetime` attribute, artifact tools line); proposal states it |
| 5 | major | Routines-first path breaks the build | Decided: `iter3-routines` lands after this change; its artifacts updated |
| 6 | minor | Test code outside `df-db` needs `jiff-sqlx` | D2 and tasks 5.1–5.4 |
| 7 | minor | Fallback incomplete (binds, `.sqlx/`) | D2 fallback, task 4.1 |
| 8 | minor | Missed uses (`factory-agent/src/transcript.rs`, `tools/artifacts.rs`, views, `tools/xtask`) | Context table, tasks 1.1, 5.2–5.5, 6.1 |
| 9 | minor | `to_proto` must normalize negative nanos | D3, task 3.1 |
| 10 | minor | Out-of-scope item without target or "rejected" | "Rejected for now" |
| 11 | nit | `Timestamp - Timestamp` is `SignedDuration` in `jiff` 1.0 | D1 table, extra mappings |
| 12 | nit | `query_as!` already calls `.into()` | D2, task 4.1 |
| 13 | nit | `jiff` 1.0 not compiled today; two `jiff` majors after | Context, proposal Impact, Risks, Out of Scope |
| 14 | nit | Wrong commit hash, "four dependencies" wording | Proposal Why |
| 15 | nit | Contributor docs (skill `rust`) | Task 6.4 |
| 16 | nit | `iter3-routines` names `df-agent-channel` for the scheduler | Fixed to `df-orchestrator` |

# Implementation review log: replace-chrono-with-jiff

- BASE: 90fa138cb97b7f7a908c371fd7a5a1273ffd920b
- Store: none
- Status: done
- Implementer agent: a63cedc7682b75d33
- DOCKER_HOST: unix:///var/run/docker.sock

## Round 1 (2026-11-23)

- Implementer: 9575846c, tasks 20/20, checks clippy/machete/xtask checks/deny/docs-build/doc tests/bazel build+test ok; nextest 3634/3635 (df-web `installed_packages_have_the_versions_of_package_json` fails: local node_modules htmx 4.0.1 vs package.json 4.0.2, unrelated; passes under Bazel); container tests ran
- Spike 2.2: `sqlx.toml` works under cargo and Bazel; no fallback
- Reviewed range: 90fa138c...9575846c

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/factory-agent/src/runner.rs:1486` | `now() + retry_after` panics on overflow; `retry_after_seconds` is unbounded harness input | fix-now | open |
| S2, X-note | Standards, Security | Blocking | `crates/df-agent-channel/src/tools/feedback.rs:211` | `n - CURSOR_OVERLAP` panics for a forge timestamp near `Timestamp::MIN` | fix-now | open |
| S3 | Standards | P1 | `crates/factory-agent/src/runner.rs:1487` | proto timestamp built by hand instead of `df_proto::time::to_proto` | fix-now | open |
| P1 | Spec | P1 | `crates/df-web/src/chat_pages/archive.rs:180`, `project_subscriptions.rs:259` | no pin test (task 1.1); `%b %d` unpinned; As-built note wrong | fix-now | open |
| S7 | Standards | P1 | `crates/df-web/src/requests/tests.rs:12`, `crates/df-db/src/lifecycle_events/tests.rs:12` | new tests above constants/fixtures | fix-now | open |
| S4 | Standards | P3 → fix (user) | `crates/df-db` record structs | conversion noise; `FromRow` structs could use `#[sqlx(try_from)]` | fix-now | open |
| S6 | Standards | P3 → fix (user) | `df-web` `chat_pages/view.rs:40`, `task_pages/view.rs:1124`, `requests.rs:361` | three identical strftime helpers | fix-now | open |
| S5 | Standards | P3 | `event_subscriptions.rs`, `chat_views.rs` | mixed `jiff::Timestamp` / `jiff_sqlx::Timestamp` in one struct (D2 pattern) | none | cosmetic |
| P2 | Spec | P3 | df-web views | `datetime` attribute pinned only at some sites | note in As-built (with P1) | open |
| P3 | Spec | P3 | `crates/df-web/src/egress/log/rows.rs:64` | negative nanos now gives empty label instead of a time | note in As-built (with P1) | open |

Security: 0 findings (no verify pass needed).

User decision: fix blocking + P1 + S4 + S6

Statuses after fix round 1: S1, S2, S3, P1, S7, S4, S6, P2, P3 → fixed (19291a44).

## Round 2 (2026-11-23)

- Implementer: 19291a44, fix round 1; clippy/machete/xtask checks/doc tests ok; nextest df-db, factory-agent, df-agent-channel 1033/1033, df-web 1003/1004 (same unrelated node_modules failure); container tests ran; Bazel not rerun (no BUILD/Cargo change)
- Reviewed range: 9575846c...19291a44

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S2 | Standards | P1 | `crates/factory-agent/src/runner.rs:1491` | fallback `now + 1h` uses panicking `+` (only reachable with `now` near `Timestamp::MAX`) | fix-now | open |
| R2-P1 | Spec | P1 | `crates/factory-agent/src/runner.rs:1485`, design.md As built | `retry_after` now keeps sub-second nanos (was whole seconds); harmless, undocumented | fix-now (document) | open |
| R2-P2 | Spec | P1 | `design.md:114` | `short_time` site list misses task run header | fix-now | open |
| R2-S1 | Standards | P1 | `crates/df-web/src/task_pages/view/tests.rs:7` | test name says "title", helper used in three places | fix-now | open |
| R2-S3 | Standards | P1 | `crates/factory-agent/src/tests/runner.rs:13` | `a_huge_retry_delay_…` unclear vs. sibling test | fix-now | open |

Security: 0 findings.

User decision: fix blocking + P1

Statuses after fix round 2: R2-S2, R2-P1, R2-P2, R2-S1, R2-S3 → fixed (42d03cd0).

## Round 3 (2026-11-23)

- Implementer: 42d03cd0, fix round 2; clippy/check-file-length/check-test-names ok; nextest factory-agent + df-web 1293/1294 (same unrelated node_modules failure); container tests ran
- Reviewed range: 19291a44...42d03cd0 (15 lines). Reviewed inline by the orchestrator: `retry_time` fallback uses `checked_add(...).unwrap_or(now)` with a `Timestamp::MAX` test; renames and design.md notes match the fix list. No findings.

## Finish (2026-11-23)

- Container tests: ran in every round against `/var/run/docker.sock`; none skipped.
- `bazel --bazelrc=.bazelrc.agents build --lockfile_mode=error //crates/... //plugins/...`: ok.
- `just docs-build`: ok. No published docs page changed (verified by spec review rounds 1 and 2).
- Tech debt: none (no P2 findings). Roadmap: backlog row narrowed in task 6.3.
- Known unrelated failure: df-web `assets::tests::installed_packages_have_the_versions_of_package_json` (local `node_modules` htmx 4.0.1 vs `package.json` 4.0.2; passes under Bazel).
