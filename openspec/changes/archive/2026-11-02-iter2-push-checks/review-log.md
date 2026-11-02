# Review log: iter2-push-checks

- BASE: 61dcf03
- Store: none
- Status: done (3 rounds; tech debt TD-89, TD-90; Bazel build `--lockfile_mode=error` ok)
- Implementer agent: stateless `implementer` subagent per run (VS Code); tasks in batches
- DOCKER_HOST: unix:///var/run/docker.sock
- Design tasks: 3.3 (PenPot, orchestrator), approved and committed in 7f8db8b
  - Boards: Config / "Projects · edit modal, push checks", "Projects · edit modal, push checks, invalid glob"; Components / "Request card" (push-check-exception Desktop + Phone, DF-17); Request Inbox / "Requests · inbox"; Tasks - Phone / "Phone · requests"
  - User decision: inbox keeps the DF-12 question with suggested answers, DF-14 dropped; the list may scroll past the viewport

## Implementation runs

| Batch | Tasks | Commits | Notes |
|---|---|---|---|
| 1 | 1.1, 1.2 | 6b9c41a, contract commit | Spike: vendored Kingfisher subset (56 rules) with `regex`, 0 new crates, 110 ms / 50 MiB; `kingfisher-scanner` +116 crates with native builds |
| 2 | 2.1, 2.2 | d963689, e89a0ae | `PushView` holds blob ids/sizes, matcher reads each blob; exceptions remove only approval-mode findings; `globset` pinned 0.4.15 (`rpi-tools`) |
| 3 | 2.3, 2.4 | 41ed90a, e10c761 | Gateway fails closed (`df:check-failed`) without a push policy in the snapshot; llvm-cov df-gateway 94.5 % lines |
| 4 | 3.1, 3.2 | 2ec8a9f, 283e99e | Agents cannot raise `push-check-exception`; merge fingerprint sha256 of sorted set; single-scope card shows "Approve for this task" |
| 5 | 3.4, 4.1 | ac0dcb9, 341196e | Wide dialog (768 px) with tabs; policy + project in one transaction; WIP `df:secret-scan` now mapped too |
| 6 | 5.1 | 7379df3, 84f074f | e2e ran in full (demo 1/1, flow 6/6); test token built at runtime; demo script no longer commits as `agent@example.com` |

Pre-existing, not from this change: df-gateway lib tests `…grace_period_and_serve_the_next_viewer` and `…upstream_timeout` flaky locally; `cargo deny` advisory RUSTSEC-2026-0433; `xtask outdated` kube/k8s-openapi/rpi-*.

### Visual comparison (orchestrator, 1440 px)

Push checks tab, invalid glob, push-check-exception card against the boards. One major defect (V1: protected-path textarea doesn't grow; the invalid fifth line is scrolled out of view). Minor: field label 14 px vs 13 px, select/number value 16 px vs 14/13 px, error border 1 px vs 2 px, error text 14 px vs 13 px, finding check chips auto width (paths not aligned). Cosmetic: row height, button padding, dialog border and vertical position, font family, comment placeholder size.

## Round 1 (2026-11-02)

- Implementer: 84f074f, tasks 12/12, checks fmt/clippy/machete ok, container tests ran
- Reviewed range: 61dcf03...84f074f
- Security: X1 verified (keep, confidence 9)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| X1 | Security | Blocking | `df-gateway/src/git/view.rs:113` | WIP push (secret/size only) puts a commit into the mirror; a later task-branch push of it adds no commits and skips identity, protected-path (incl. `.dark-factory/**`) and approval checks | | open |
| P1 | Spec | Blocking | `df-gateway/src/git/view.rs:151` | Merge commits diffed against each parent: merging `main` reports the human's changes (hard reject on `.dark-factory/**`, requests for others' workflow edits) | | open |
| P2 | Spec | Blocking | `df-db/src/push_checks.rs:383` | `df:push-too-large` task-branch rejection queues no `push.rejected` message | | open |
| S1 | Standards | P1 | `df-gateway/src/git/checks.rs:26`, `factory-agent/src/workspace.rs:450` | `df:` reason strings duplicated between gateway and runtime; one table in df-proto (layer-specific labels in df-db/df-web stay) | | open |
| S2+S3 | Standards | P1 | `df-db/src/push_checks.rs:292,326` | Secret detail split back into rule/preview; reason formatted twice and differently; test fingerprint format wrong | | open |
| S4 | Standards | P1 | `df-web/src/config_pages/push_policy.rs:141` | Glob construction duplicated between web validation and gateway | | open |
| S5 | Standards | P1 | `df-gateway/src/git/secret_rules.rs:137`, `view.rs:300` | `String` errors in library code; `git()` unclear name | | open |
| S6 | Standards | P1 | `df-web/src/config_pages.rs:370` | `x-data` argument built by string concatenation | | open |
| S7 | Standards | P1 | `df-gateway/src/git/secret_rules.rs:131` | Unused `pub fn regex`, `is_empty`; `len` test-only | | open |
| S10 | Standards | P1 | `df-gateway/src/git/view.rs:267` | `pub(crate) fn escape` after private helpers | | open |
| S13 | Standards | P1 | `df-db/src/push_checks/tests.rs:85`, `push_rejections.rs:140,179` | Test names point to documents, not behavior; `test_` prefix in `secret_rules.rs` | | open |
| S14 | Standards | P1 | `df-gateway/tests/push_checks.rs:280` | `LogSink` reimplements `df_telemetry::testing::buffer()` | | open |
| P3 | Spec | P1 | `factory-agent/src/workspace.rs:86` | Prepare-time push rejected by a `df:` check is untested | | open |
| P4 | Spec | P1 | `df-gateway/src/control/tests.rs` | Policy change reaching a gateway within 1 s untested | | open |
| P5 | Spec | P1 | `df-gateway/src/control/tests.rs` | Cross-replica test doesn't use replica A | | open |
| P6 | Spec | P1 | `specs/push-checks/spec.md` | Spec says exceptions pass findings; code limits it to approval-mode findings (design non-goal) — reword spec (orchestrator) | | open |
| P7 | Spec | P1 | `df-db/src/push_checks.rs:579` | "cannot be approved" text when the request was not raised because of too many pending ones | | open |
| V1 | Visual | P1 | policy form | Protected-path textarea doesn't grow; invalid line 5 scrolled out of view | | open |
| S8 | Standards | P3 | `df-gateway/src/git/push.rs:119` | Data clumps findings/approval_requested, policy/exceptions | | cosmetic |
| S9 | Standards | P3 | `df-gateway/src/git/quarantine.rs:52` | Middle man `Quarantine::push_view` | | cosmetic |
| S11 | Standards | P3 | `df-db/src/push_checks.rs:203` | `Notification::EgressPolicy` now also carries push policy | | cosmetic |
| S12 | Standards | P3 | `df-db/src/push_checks.rs:550` | All `RequestError`s mapped to `InvalidData` | | cosmetic |
| S15 | Standards | P3 | `fake-acp-agent/scripts/push-secret.yaml:16` | Duplicated fix turn (YAML anchor) | | cosmetic |
| S16 | Standards | P3 | `df-web/src/config_pages.rs:1068` | Side effect in `map_err` | | cosmetic |
| S17 | Standards | P3 | `df-web/src/components/overlay.rs:40` | `DialogStyle::Wide` class string copied | | cosmetic |
| V2 | Visual | P3 | policy form, card | 16 px control values, 1 px error border, 14 px error text, chip widths, dialog border/position | | cosmetic |

User decision: Fix blocking + P1 (all Blocking and P1 rows are fix-now; P6 is reworded by the orchestrator).

Status after the fix round: X1, P1, P2, P3, P4, P5, P7 fixed (c82d40d); S2+S3, S4, S5, S6, S7, S10, S13, S14, V1 fixed (13b660f); S1 partly fixed (rest is R2-S1); P6 fixed (683af48). Round 2 reviewers confirmed each.

## Round 2 (2026-11-02)

- Implementer: c82d40d (part A), 13b660f (part B), container tests ran
- Reviewed range: 683af48...13b660f
- Security: no findings; X1 variants checked (other tasks' branches, DF-1 vs DF-10, deleted WIP ref, tags, merges)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-P1 | Spec | Blocking | `df-gateway/src/git/view.rs:328` | `added_blobs` skips blobs the mirror has, so a blob brought in by another task's branch (approved there) gets no secret/size check for this task | | open |
| R2-P2 | Spec | Blocking | `specs/push-checks/spec.md:10`, `design.md` D1/D2 | Spec and design still describe "any mirror ref", diffs against each parent and `fingerprint: String`; no scenarios for WIP → task branch and merging the base branch (orchestrator updates the artifacts) | | open |
| R2-S1 | Standards | P1 | `factory-agent/src/workspace.rs:466`, `df-gateway/src/git/refs.rs:105` | `df:non-fast-forward` literal still duplicated | | open |
| R2-S4 | Standards | P1 | `df-db/src/push_checks.rs:347` | `ExceptionFinding::of` hides fallibility; name it `from_finding` or `TryFrom` | | open |
| R2-S2 | Standards | P2 | `df-gateway/src/git/checks.rs:16` | Gateway `Check` duplicates `pb::PushCheck`; adding a check edits three enums. Matters when a check is added | | open |
| R2-S3 | Standards | P3 | `df-db/src/push_checks.rs:114` | `protected_glob` puts `globset` into the storage crate | | cosmetic |
| R2-S5 | Standards | P3 | `df-proto/src/push_reasons.rs:44` | `finding_reason` parameter clump | | cosmetic |
| R2-S6 | Standards | P3 | `df-db/src/push_checks.rs:502` | `Rejection::Reason(&'static str)` with one value | | cosmetic |

User decision: Fix blocking + P1 (R2-P1, R2-S1, R2-S4 to the implementer; R2-P2 by the orchestrator; R2-S2 tech debt).

Status after the fix round: R2-P1, R2-S1, R2-S4 fixed (commit "fix round 2"); R2-P2 fixed (78c9f85); R2-S2 tech-debt (TD-89).

## Round 3 (2026-11-02)

- Implementer: commit "fix round 2", container tests ran
- Reviewed range: 78c9f85...(fix round 2)
- Security: no findings; X1 not reopened

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R3-P1 | Spec | P1 | `design.md:81` | Risk line claims the 256 MiB push limit bounds the scanned bytes; mirror blobs are scanned too | fix-now (orchestrator) | fixed (design.md) |
| R3-P1b | Spec | P2 | `df-gateway/src/git/view.rs` | No cap on the bytes scanned per push; blobs read whole | tech debt | tech-debt (TD-90) |
| R3-S1 | Standards | P3 | `df-proto/src/push_reasons.rs:79` | Test list uses the `df:non-fast-forward` literal | | cosmetic |
| R3-P2 | Spec | P3 | `df-gateway/tests/push_checks.rs:345` | Size check of a mirror blob only tested at view level | | cosmetic |

User decision: fix the design line, write TD-89/TD-90 and finish.
