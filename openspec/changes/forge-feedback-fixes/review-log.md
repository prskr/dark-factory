# Review log: forge-feedback-fixes

- BASE: e883b1a8c58a5bb694bf651d004962845243382b
- Store: none
- Status: done
- Implementer agent: a9c59200c93649c52

## Round 1 (2026-11-06)

- Implementer: c8a1e46a, 265ab84e (HEAD_1), tasks 28/29 (7.4 manual Forgejo check open), checks fmt/clippy/machete/deny/file-length/test-names/check-crates/docs-build/helm ok, cargo tests ok, bazel-test 51/51, container tests ran
- Reviewed range: BASE...265ab84e

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `plugins/forgejo/README.md:56` | Roles bullet still says 403 fails `list-feedback` with `unauthorized` | fix-now | fixed (9448c005) |
| P2 | Spec | Blocking | `crates/df-plugins/src/state.rs:222`, `plugins/forge-common/src/log.rs:15` | Plugin warning logged at INFO; spec requires a warning in the control plane log | fix-now | fixed (9448c005) |
| P5 | Spec | Blocking | `plugins/forgejo/src/api/feedback/tests.rs:487` | 25/3-review tests don't assert returned threads (reviews have no comments) | fix-now | fixed (9448c005) |
| P6 | Spec | Blocking | feedback tests | No test for "Older review thread beyond the listing limit" (with `since`) nor "Item created between two listings" | fix-now | fixed (9448c005) |
| P7 | Spec | Blocking (manual) | `tasks.md` 7.4 | Live Forgejo check not run; stderr→log path untested end to end | manual (user runs 7.4) | fixed (manual check by user, 2026-11-06) |
| S1 | Standards | P1 | `crates/df-core/src/config.rs:715` | `MAX_PLUGIN_CALL_TIMEOUT_SECONDS` not in constants block | fix-now | fixed (9448c005) |
| S2 | Standards | P1 | `crates/df-agent-channel/src/tools/feedback.rs:19` | `CURSOR_OVERLAP` ≥ timeout max tied only by comments | fix-now | fixed (9448c005) |
| S3 | Standards | P1 | both plugins `api.rs`/`error.rs` | Request line + Unauthorized prefix built differently per plugin | fix-now | fixed (9448c005) |
| S4 | Standards | P1 | `plugins/forge-common/src/draft.rs` | Doc names GitHub only | fix-now | fixed (9448c005) |
| S5 | Standards | P1 | `crates/df-agent-channel/src/webhooks/tests.rs:609` | Test name claims refused lookup; asserts plain `outside` | fix-now | fixed (9448c005) |
| S8 | Standards | P1 | `crates/df-agent-channel/src/tools/feedback.rs:196` | Param `next_since` shadows fn name | fix-now | fixed (9448c005) |
| P3 | Spec | P1 | `tasks.md` 5.3 | Output schema deviation not recorded | fix-now | fixed (9448c005) |
| P4 | Spec | P1 | `crates/df-agent-channel/src/tools/feedback.rs:214` | Given `since` returned with caller offset; D4/docs say UTC | fix-now | fixed (9448c005) |
| S6 | Standards | P3 | Forgejo feedback tests:504 | Test name is a topic | fix-now | fixed (9448c005) |
| S7 | Standards | P3 | Forgejo feedback tests | Duplicated test setup/assertions | fix-now | fixed (9448c005) |
| X0 | Security | Rejected | `feedback.rs:211` | chrono overflow panic on `- CURSOR_OVERLAP` | — | rejected (RFC 3339 parse limits years to 0000–9999; −5 min stays in range) |

User decision: Fix blocking + P1 + S6 + S7 (P7 stays a manual check for the user).

## Round 2 (2026-11-06)

- Implementer: 9448c005 (HEAD_2), tasks 28/29 (7.4 open), checks fmt/clippy/machete/file-length/test-names/docs-build ok, bazel-test 51/51, container tests ran
- Reviewed range: 265ab84e...9448c005

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8 | Spec | Blocking | `crates/df-agent-channel/src/tools/feedback.rs:217` | `SecondsFormat::Secs` drops fractions, so `next_since` can be earlier than a sub-second `since` (spec: later of `since` and cursor) | fix-now | fixed (dc17dd8a) |
| S9 | Standards | Blocking | `crates/df-plugins/src/state.rs:194` | `WARNING_PREFIX` not in constants block (AGENTS.md file structure; fix-round code) | fix-now | fixed (dc17dd8a) |
| S10 | Standards | P1 | `state.rs:194`, `plugins/forge-common/src/log.rs:14` | `"warning: "` protocol string written twice | fix-now | fixed (dc17dd8a) |
| S11 | Standards | P1 | `crates/df-agent-channel/src/tools/feedback.rs:212` | Local `since` shadows the `&str` param with another type | fix-now | fixed (dc17dd8a) |
| S12 | Standards | P1 | `docs/content/integrations/forges/_index.md:57` | Paragraph wrap inconsistent | fix-now | fixed (dc17dd8a) |

User decision: Fix blocking + P1.

## Round 3 (2026-11-06)

- Implementer: dc17dd8a (HEAD_3), tasks 28/29 (7.4 open), checks fmt/clippy/file-length/test-names/docs-build ok, bazel-test 51/51, container tests ran
- Reviewed range: 9448c005...dc17dd8a

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| — | — | — | — | No findings on any axis (security, standards, spec) | — | — |

## Finish (2026-11-06)

- Task 7.4 run manually by the user against a live Forgejo; confirmed, ticked.
- Container tests: ran in every round (Podman socket), none skipped.
- Bazel build `--lockfile_mode=error //crates/... //plugins/...`: ok. `just docs-build`: ok.
- Tech debt: none created (no P2 findings). TD-40, TD-80, TD-95, TD-96 resolved by the change.
- Roadmap: backlog row "Forgejo author role lookup without repository admin" added by the implementer (task 7.3).
- Status: done. Tasks 29/29, 3 review rounds, 2 fix rounds.
