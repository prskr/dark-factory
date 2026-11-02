# Review log: iter2-egress-approvals

- BASE: 90437bf2a36999aa76bce1a635835a29a67abe20
- Store: none
- Status: done
- Implementer agent: subagent (stateless, one per round)
- Scope decision (2026-10-31): backend first (sections 1 and 2); design and UI (3.x) later with PenPot connected; 4.1 after 3.x.
- DOCKER_HOST: unix:///var/run/docker.sock

## Round 1 (2026-10-31)

- Attempt 1: implementer stopped without report or commit (partial contract edits only); rerun in parts.
- Part A: c2c20ca, tasks 1.1 and 1.2, checks ok, container tests ran.
- Part B: first attempt stopped without report; B1: 8a2a7f1, task 2.1 (+ most of 2.2), checks ok, container tests ran.
- Part C: ae3be0c, tasks 2.2 and 2.3, checks ok, container tests ran.
- Part D: first attempt stopped without report; continuation: 0d1046a (gateway reporting), 9894ca8 (ReportDenial server), task 2.4, checks ok, container tests ran.
- Reviewed range: BASE...9894ca8

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1+C2 | Standards+Spec | Blocking | `crates/df-gateway/src/denials.rs:59`, `crates/df-db/src/network_access.rs:141` | agent-controlled hosts reported unvalidated (incl. `*.suffix` via CONNECT → wildcard request raised "by the control plane"); dedup map only pruned per run | fix-now | fixed (21141e4) |
| C1 | Spec | Blocking | `crates/df-db/src/requests.rs`, `crates/df-agent-channel/src/tools.rs:516` | `request_approval` stores the raw payload, not normalized `{host, port}` with default 443 | fix-now | fixed (21141e4) |
| C3 | Spec | Blocking | `crates/df-gateway/src/policy.rs:213`, `crates/df-gateway/src/proxy.rs:291` | plain HTTP on ports other than 80 reported as `not-allowlisted`; the request can never be satisfied | fix-now | fixed (21141e4) |
| C4 | Spec | Blocking | `crates/df-db/src/network_access/tests.rs:293` | no test that approval puts the task rule into the snapshot and publishes `egress_policy` | fix-now | fixed (21141e4) |
| S2 | Standards | P1 | `crates/df-gateway/src/denials.rs:148` | retry also repeats non-transient errors (INVALID_ARGUMENT) | fix-now | fixed (23630bc) |
| S3 | Standards | P1 | `crates/df-db/src/network_access.rs:140` | host/port validation duplicates `requested_target` | fix-now | fixed (23630bc) |
| S5 | Standards | P1 | `crates/df-db/src/egress.rs:363`, `crates/df-gateway/src/control/snapshot.rs:77` | scope ↔ string mapping repeated | fix-now | fixed (23630bc) |
| S6 | Standards | P1 | `crates/df-db/src/network_access.rs:149`, `crates/df-db/src/egress.rs:333` | new queries not compile-checked | fix-now | fixed (23630bc) |
| S7 | Standards | P1 | `crates/df-db/src/network_access.rs:225` | `settle` builds its own `ApprovalTypes::builtin()` | fix-now | fixed (23630bc) |
| S9 | Standards | P1 | `crates/df-db/src/network_access.rs:117` | duplicated `Raised(Box, bool)` with positional bool | fix-now | fixed (23630bc) |
| C6 | Spec | P1 | `design.md` D2 | design says `Empty`, contract has `ReportDenialResponse` | orchestrator | fixed (design text) |
| C5 | Spec | P1 | `crates/df-db/src/network_access.rs:171` | no `cancelled` task status until iter2-cancel | orchestrator | fixed (note in iter2-cancel task 2.3) |
| S4 | Standards | P2 | `crates/df-agent-channel/src/tools.rs:489` | `request_approval` duplicates the raise logic of `ask_question`; matters with a third raising tool | tech-debt | tech-debt (TD-84) |
| S8 | Standards | P3 | `crates/df-db/src/requests.rs:430` | flag argument `apply=false` | – | cosmetic |
| S10 | Standards | P3 | `crates/df-db/src/lib.rs:25` | root re-exports not yet used outside df-db | – | cosmetic |
| S11 | Standards | P3 | `crates/df-db/src/egress.rs:661` | duplicated test fixture | – | cosmetic |
| C7 | Spec | Rejected | – | 50-denials scenario covered in two parts | – | rejected (gateway dedup + control-plane merge each tested) |

User decision: Fix blocking + P1

## Round 2 (2026-10-31)

- Implementer: 21141e4 (fix round 1a: S1, C2, C1, C3, C4), 23630bc (fix round 1b: S2, S3, S5, S6, S7, S9; first attempt stopped without report, continued), checks ok, container tests ran.
- Reviewed range: 9894ca8...23630bc
- Round-1 rows: all fix-now rows confirmed fixed by both reviewers (S1+C2 21141e4, C1 21141e4, C3 21141e4, C4 21141e4, S2/S3/S5/S6/S7/S9 23630bc), no regressions.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S20 | Standards | P1 | `crates/df-gateway/src/proxy/tests.rs:674` | `other_denial_reasons_are_not_reported` also asserts the HTTP:8080 case; name no longer matches | fix-now | fixed (257c4c2) |
| S15 | Standards | P1 | `crates/df-db/src/requests.rs:454` | `record_approval_in` relies on the caller holding the allowlist lock, undocumented | fix-now | fixed (257c4c2) |
| S16 | Standards | P1 | `crates/df-gateway/src/denials.rs:67`, `crates/df-db/src/network_access.rs:137` | "exact host, no wildcard" rule written twice | fix-now | fixed (257c4c2) |
| S18 | Standards | P1 | `crates/df-gateway/src/denials.rs:30` | `is_transient` imported from `git::records`; classification duplicated | fix-now | fixed (257c4c2) |
| S19 | Standards | P1 | `crates/df-gateway/src/policy.rs:59` | "plain HTTP only on 80" rule written twice | fix-now | fixed (257c4c2) |
| C8 | Spec | P1 | `crates/df-db/src/requests.rs:463` | no test for a request decided while settling runs | fix-now | fixed (257c4c2) |
| C9 | Spec | P1 | `design.md` D2 | design text: retry only transient, pruning by age | orchestrator | fixed (design text) |
| C10 | Spec | P1 | `design.md` D3 | design text: no `apply` flag any more | orchestrator | fixed (design text) |
| S12 | Standards | P2 | `crates/df-db/src/requests.rs:432` | `decide` repeats the `record_approval_in` sequence; type looked up three times | tech-debt | tech-debt (TD-85) |
| S13 | Standards | P2 | `crates/df-db/src/requests.rs:486` | `(state, stored, action)` tuple travels (Data Clump) | tech-debt | tech-debt (TD-85) |
| S14 | Standards | P3 | `crates/df-db/src/requests.rs:454` | unused `Option<RequestRow>` return | – | cosmetic |
| S17 | Standards | P3 | `crates/df-db/src/network_access.rs:136` | JSON built only to be parsed again | – | cosmetic |

User decision: Fix P1

## Round 3 (2026-10-31)

- Implementer: 257c4c2 (fix round 2: S20, S15, S16, S18, S19, C8), checks ok, container tests ran.
- Reviewed range: 23630bc...257c4c2
- Round-2 rows: S20, S15, S16, S18, S19, C8 fixed (257c4c2); C9, C10 confirmed (design text). No regressions; no spec behavior changed.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S21 | Standards | P1 | `crates/df-db/src/requests.rs:225` | `ApprovalType::lock` doc doesn't say it must be re-entrant | orchestrator | fixed (doc line) |
| C11 | Spec | P1 | `design.md` D3 | text says requests are selected by fingerprint; code matches with the proxy matcher | orchestrator | fixed (design text) |
| S22 | Standards | P2 | `crates/df-gateway/src/git/records.rs:191` | `Failure` duplicates `CallError`; connect errors mapped by hand in both senders | tech-debt | tech-debt (TD-86) |
| S23 | Standards | P3 | `crates/df-db/src/network_access.rs:138` | host validated twice | – | cosmetic |

User decision: finish the backend phase; S21 and C11 as doc edits by the orchestrator; P2 to tech debt.

## Status after the backend phase

- Tasks 1.1–2.4 done; 3.1, 3.2 (PenPot design, UI) and 4.1 (e2e) open.
- No blocking finding open. Container-backed tests ran in every round.
- Tech debt: S4 → TD-84, S12+S13 → TD-85, S22 → TD-86.
- Next: connect the PenPot MCP, then task 3.1 (design, user approval), 3.2, 4.1, and a review round over their diff.

## Round 4 (2026-11-01): task 4.1

- PenPot: the user's own instance has no "App Design" file; 3.1/3.2 wait for access to Peter's instance. User decision: do 4.1 first (it approves through the existing `/requests` inbox).
- Implementer: e9216e5 (e2e demo step 4, fake-agent `fetch` step), ed0bccf (e2e run, `design/egress.md` §4/§8, 4.1 ticked); full `deploy/e2e/e2e-local.sh` passed (demo 1/1, flow 4/4); kind cluster deleted.
- Seed test change verified: the seed's allowlist entry was dropped by 8e454b4 (git proxy) without updating the test; not a regression of this change.
- Reviewed range: 106f60f...ed0bccf

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C12 | Spec | Blocking | `crates/df-e2e/tests/flow.rs:283` | "same run / without a restart" not proven (harness falls back to a new session when resume fails) | fix-now | fixed (85b9c6c, 6a6a5e6) |
| C13 | Spec | P1 | `crates/df-e2e/tests/flow.rs:15` | "about a second" only bounded by 30 s | fix-now | fixed (85b9c6c, fix round 4) |
| S24 | Standards | P1 | `crates/fake-acp-agent/src/agent.rs:434` | `Instant + Duration` panics on huge script timeout | fix-now | fixed (85b9c6c) |
| S26 | Standards | P1 | `crates/fake-acp-agent/src/agent.rs:440` | retry sleep ignores cancel; attempt overruns the deadline | fix-now | fixed (85b9c6c) |
| S25 | Standards | P1 | `crates/fake-acp-agent/src/agent.rs:451` | `curl` without `kill_on_drop` | fix-now | fixed (85b9c6c) |
| S28 | Standards | P1 | `crates/fake-acp-agent/src/script.rs:109` | `fetch` step missing from the fake-acp-agent step reference | orchestrator | fixed (delta spec `specs/fake-acp-agent/spec.md`) |
| C14 | Spec | P1 | `Justfile:54` | seed recipe comment stale | fix-now | fixed (85b9c6c) |
| C15 | Spec | P1 | `crates/df-e2e/src/data.rs:19` | "both projects" stale | fix-now | fixed (85b9c6c) |
| C16 | Spec | P1 | `design/egress.md:40` | task entries not yet listed on the allowlist page (3.2) | fix-now | fixed (85b9c6c) |
| C17 | Spec | P1 | `design/egress.md:118` | wording suggests the decision arrives inside the running turn | fix-now | fixed (85b9c6c) |
| S29 | Standards | P2 | `crates/fake-acp-agent/src/script.rs:19` | pre-existing: unknown/malformed step silently loads an empty script | tech-debt | tech-debt (TD-88) |
| S27, S30, S31, S32 | Standards | P3 | `agent.rs:31`, `tests.rs:786`, `tests.rs:822`, `flow.rs:287` | `&str` constant, test server handle, real-time wait, duplicated texts | – | cosmetic |

User decision: Fix blocking + P1

## Round 5 (2026-11-01)

- Implementer: 85b9c6c (fix round 3), 6a6a5e6 (fix round 3b, e2e test after the runs); full e2e passed (demo 1/1, flow 4/4).
- Reviewed range: ed0bccf...6a6a5e6
- Round-4 rows: C12, C13, S24, S26, S25, C14, C15, C16, C17 fixed (85b9c6c/6a6a5e6); no regressions.
- Observations: (a) the run stays `preparing` during the first turn (pre-existing, `factory-agent` sends `Running` only after the idle wait); (b) the decision message arrives as the next prompt of the same session; the e2e script's `loaded` turn never runs (script assumption).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C18 | Spec | P1 | `crates/fake-acp-agent/scripts/network-access.yaml:10` | script expects a new run for the decision; it is the next prompt of the same session | fix-now | fixed (fix round 4) |
| C19 | Spec | P1 | `docs/demo.md:128` | "next run started by the decision message" wrong | fix-now | fixed (fix round 4) |
| C20 | Spec | P1 | `docs/demo.md:102` | doc says 30 s, test enforces 10 s | fix-now | fixed (fix round 4) |
| S33 | Standards | P1 | `crates/df-e2e/tests/flow.rs:298` | separate wall-clock check after a 30 s wait | fix-now | fixed (fix round 4) |
| (a) | – | P2 | `crates/factory-agent/src/runner.rs:1226` | first turn never reports `Running`; run stays `preparing` (pre-existing) | tech-debt | tech-debt (TD-87) |
| S34, S35 | Standards | P3 | `flow.rs:16`, `agent.rs:466` | naming; cancel-wait pattern duplicated | – | cosmetic |

User decision: Fix P1; P2 (a) and S29 to tech debt

- Fix round 4 (C18, C19, C20, S33), full e2e passed (demo 1/1, flow 4/4, including the decision reply in the same run); small diff checked by the orchestrator, no further review round.

## Status after task 4.1

- Tasks 1.1–2.4 and 4.1 done; 3.1 and 3.2 open (waiting for access to the PenPot file "App Design").
- No blocking finding open. Container-backed tests and the full e2e suite ran.
- Tech debt: TD-84 to TD-88.

## Design (2026-11-01): task 3.1

- PenPot "App Design" on Peter's instance (exporter was OOM, restarted). New boards on the Network page: "Network · allow from a row, with scope", "Network · narrow table, blocked row menu", "Network · allow from a graph node (hover)", "Network · allow from a graph node, across tasks", "Egress rules · table with task entries". Approved by the user; mapping rows in `docs/design-workflow.md`; commit 991a211.

## Round 6 (2026-11-01): task 3.2

- Implementer: 1fa5782 (allowlist task entries), 249096c (allow from the log, narrow menu, graph node popover), db3b6cc (docs); df-web 414 tests pass. Tests written together with the code, not test-first.
- Visual check by the orchestrator (local `serve --no-cluster --demo-flows`, Playwright at 1440 and 1280): dialog → "Create rule" → toast "Rule created … allowed for task DF-7" → rules page "Task DF-7"; narrow menu matches.
- Reviewed range: fca7d7a...db3b6cc

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C23 | Spec | Blocking | `crates/df-web/src/egress/log/graph.rs:216` | node scopes derived only from edges of the shown sandboxes | fix-now | fixed (fix round 5a, completed in fix round 6) |
| V1 | Design | P1 | allow dialog | scope radios stacked; board has one row | fix-now | fixed (fix round 5b) |
| V2 | Design | P1 | allow dialog | divider under the header missing | fix-now | fixed (fix round 5b) |
| V3 | Design | P1 | allow dialog | destination value too large; board 13 px mono, compact field | fix-now | fixed (fix round 5b, fix round 6) |
| V6 | Design | P1 | allow dialog | width 576 px, board 480 px | fix-now (if the modal has a size option) | accepted (no size option; known deviation in `docs/design-workflow.md`) |
| V4 | Design | P1 | row action | text button instead of the board's outlined button | deviation | accepted (known deviation in `docs/design-workflow.md`) |
| V5 | Design | P1 | node popover | opens below the node; solid button instead of outlined | fix button; placement as deviation | fixed (button, fix round 5b and 6); placement accepted |
| C21 | Spec | P1 | `crates/df-web/src/egress/tests/allow.rs:39` | "next attempt succeeds" not checked through effective rules | fix-now | fixed (fix round 5a) |
| C22 | Spec | P1 | `crates/df-web/src/egress/log/allow.rs:250` | settling through the log action untested | fix-now | fixed (fix round 5a) |
| C24 | Spec | P1 | `design/egress.md` §4/§8 | docs still say planned; log action not named as a way to create task entries | fix-now | fixed (fix round 5b) |
| C26 | Spec | P1 | `design.md` D5 | posts to its own allow handler | orchestrator | fixed (design text) |
| S36 | Standards | P1 | `allow.rs:139` | port check duplicates `validate_port` | fix-now | fixed (fix round 5b) |
| S37 | Standards | P1 | `allow.rs:177` | scope as `String`/match | fix-now | fixed (fix round 5b) |
| S38 | Standards | P1 | `allow.rs:265`, `allowlist.rs:89` | scope label mapping three times | fix-now | fixed (fix round 5b) |
| S42 | Standards | P1 | `allow.rs:87` | row buttons share one accessible name | fix-now | fixed (fix round 5b) |
| S43 | Standards | P1 | `crates/df-web/src/egress/tests/allow.rs` | invalid port and duplicate entry untested | fix-now | fixed (fix round 5b) |
| S39, S40, S41, S44, S45, S46, S47, C25 | Standards/Spec | P3 | various | return type, data clump, HX headers, tests not written first, test name, route constant, dialog query check, across-projects only a unit test | – | cosmetic |

User decision: Fix blocking + P1 (V4 and the V5 placement as known deviations)

## Round 7 (2026-11-01)

- Implementer: e49dc12 (fix round 5a: C23, C21, C22), 77e3849 (fix round 5b: V1, V2, V3, V5, C24, S36, S37, S38, S42, S43); V6 left as a known deviation (shared modal width, recorded in `docs/design-workflow.md`).
- Visual re-check at 1440: dialog matches the board (radios in one row, header divider, compact destination).
- Reviewed range: db3b6cc...77e3849
- Round-6 rows: C23, C21, C22, C24, V1, V2, V3, V5 (button), S36, S37, S38, S42, S43 fixed; V6 deviation; no regressions.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C27 | Spec | P1 | `crates/df-web/src/egress/log/graph.rs:228` | node whose only denial comes from a hidden sandbox offers Allow but is not drawn as blocked | fix-now | fixed (fix round 6) |
| S48 | Standards | P1 | `crates/df-web/src/egress/log/allow.rs:229` | destination field hand-built instead of `readonly_field` | fix-now | fixed (fix round 6) |
| S49 | Standards | P1 | `crates/df-web/src/egress/log/graph.rs:448` | popover button copies the secondary button classes | fix-now | fixed (fix round 6) |
| S53 | Standards | P1 | `crates/df-web/src/egress/tests/allow.rs:71` | test checks more than its name says | fix-now | fixed (fix round 6) |
| C28, C29, S50, S51, S52, S54, S55 | Spec/Standards | P3 | various | divider inset, comment wording, `ScopeName::of` signature, serde round-trip test, assertion wording, magic number in a comment, helper name | – | cosmetic |

User decision: Fix P1, then finish

- Fix round 6 (C27, S48, S49, S53): small diff checked by the orchestrator; df-web 422 tests pass, container tests ran. No further review round.

## Final status

- Status: done. All tasks complete (1.1–4.1); no blocking finding open; container-backed tests and the full e2e suite ran; visual comparison at 1280 and 1440 px done.
- Tech debt: TD-84 to TD-88.
- Known deviations recorded in `docs/design-workflow.md`: row action style, node popover placement, dialog width.
