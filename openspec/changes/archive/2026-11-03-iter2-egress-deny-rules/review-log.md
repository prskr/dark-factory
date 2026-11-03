# Review log: iter2-egress-deny-rules

- BASE: dee5f0baceebbd0987f15269ae6689a9b931f9e5
- Store: none
- Status: done
- Implementer agent: a838e539e4604e764
- Step: finished

## Design (2026-11-03)

- Commit: 74b7cb0 (`wip(iter2-egress-deny-rules): design`), task 3.1 done.
- Boards: Network / Egress rules · new rule, allow; · new rule, deny; · deny rule listed; Network · deny rule and deny always; Tasks / Requests · deny always; Tasks - Phone / Phone · requests, deny always.
- User revisions (approved): colored Allow/Deny toggle; Effect and "Applies to" in one row; Deny disables "Applies to" and Port (`any`) instead of hiding them; Effect column (Allow/Deny badge) in the rules table; deny rows show scope "Everywhere".
- Follow-up (user): Effect column (Allow badge) added to Egress rules · table with task entries, · new rule, allow/deny (background table), · edit (from traffic view). Superseded board "Egress rules · table" deleted. User moved "Network · deny rule and deny always" to page "Network - Graph"; mapping updated.
- Follow-up 2 (user, approved): "Egress rules · edit (from traffic view)" rebuilt on the current table; its modal has the Effect + Applies to row, the match stats line is removed. Scope extended: search (`?q=`, host or scope, case-insensitive, right-aligned like the traffic page) and pagination (25 rows, Previous/Next and "Page x of y" only with more than one page, count line always, `highlight=` opens the page holding the row, new search resets to page 1, empty result "No rules match \"<q>\"") on every rules board; new boards "Egress rules · search and pages" and "· search results". "Last matched" column removed from all rules boards (no data).
- Spec drift: `specs/egress-allowlist/spec.md` says port and scope are hidden for Deny and has no Effect column; design.md D6 says the component hides port and scope. Search and pagination of the rules table are not in the change yet. Run `/opsx:update` before implementation.
- Artifacts updated in e2b6aa6 (`/opsx:update`).

## Round 1 (2026-11-03)

- Implementer: 6789975 (55a3f24, 47cb42f, 6e57e43, 6789975), tasks 12/12, checks fmt/clippy/machete/file-length ok; tests df-db 133, df-web 512, df-agent-channel 225, df-gateway all pass; `cargo llvm-cov` not run. Container tests ran (podman socket).
- Reviewed range: dee5f0b...6789975

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S3 | Standards | Blocking | `crates/df-web/src/egress/tests/deny_rules.rs:88,162`, `crates/df-db/src/network_access/tests.rs:753` | "No egress_policy publish" assertion is vacuous (probe channel not listened, `try_recv` right away) | fix-now | fixed (cc10c09) |
| SP1 | Spec | Blocking | `crates/df-db/src/network_access/tests.rs` | Scenario "Concurrent deny rule and denial" untested | fix-now | fixed (cc10c09) |
| SP2 | Spec | Blocking | `crates/df-db/src/egress/deny/tests.rs` | Scenario "Allow rule still wins" untested | fix-now | fixed (cc10c09) |
| SP5, S8 | Spec, Standards | Blocking | `crates/df-web/src/egress/tests/deny_log.rs:119` | Scenario "Rule added while the page is open" untested (stream not exercised); test name overclaims | fix-now | fixed (cc10c09) |
| SP4 | Spec | Blocking | `crates/df-web/src/egress/tests/paging.rs:109` | Scenario "Search by scope" (task scope `df-7`) untested | fix-now | fixed (cc10c09) |
| SP6 | Spec | Blocking | `crates/df-db/src/network_access/tests.rs:718` | Scenario "Deny rule added": actor not asserted | fix-now | fixed (cc10c09) |
| S2, SP9 | Standards, Spec | Blocking | `tasks.md` 5.2 | `cargo llvm-cov` not run although 5.2 is ticked | fix-now | fixed (cc10c09) |
| SP10 | Spec | P1 | `crates/df-web/assets/egress-rule.js` | Alpine toggle untested | fix-now | fixed (cc10c09) |
| S1 | Standards | P1 | `crates/df-web/assets/egress-rule.js:10` | Component name `ruleEffect` does not match file | fix-now | fixed (cc10c09) |
| S5 | Standards | P1 | `crates/df-web/src/egress/log/deny.rs:60-86`, `allow.rs:300-320` | Toast, retarget headers and highlight href duplicated | fix-now | fixed (cc10c09) |
| S4 | Standards | P1 | `crates/df-web/src/egress/tests/deny_log.rs:24` | Copy of `rules::row_of` | fix-now | fixed (cc10c09) |
| S10 | Standards | P1 | `crates/df-web/src/egress/tests/paging.rs` | Delete-URL extraction duplicated, decoding inconsistent | fix-now | fixed (cc10c09) |
| S9 | Standards | P1 | `crates/df-web/src/egress/mod.rs:81` | Route literal instead of `DENY` constant | fix-now | fixed (cc10c09) |
| S6 | Standards | P1 | `crates/df-web/src/egress/allowlist.rs` | `effect: String` instead of an enum | fix-now | fixed (cc10c09) |
| S7 | Standards | P1 | `crates/df-db/src/egress.rs:525` | `audit_and_notify` name misleading for deny rules | fix-now | fixed (cc10c09) |
| SP7, SP8, SP3 | Spec | P1 | `design.md` D5, D6; `tasks.md` 4.2 | Drift: board-member role, `POST /egress/log/deny`, Phone check is a markup test | fix-now | fixed (cc10c09) |
| O1 | Security (orchestrator) | P2 | `crates/df-db/src/network_access.rs` `deny_always` | "Deny always" copies a wildcard request host into a deny rule (silences requests, never grants) | tech-debt | tech-debt (TD-97) |

Security reviewer: 0 findings.

User decision: Fix blocking + P1.

## Round 2 (2026-11-03)

- Implementer: cc10c09, fixed S3, SP1, SP2, SP5/S8, SP4, SP6, S2/SP9, SP10, S1, S5, S4, S10, S9, S6, S7, SP7/SP8/SP3; checks fmt/clippy/machete/file-length/test-names ok; tests df-db 135, df-web 516, df-agent-channel 225 pass; container tests ran; llvm-cov lines df-db 95.94 %, df-web 97.19 %.
- Reviewed range: 6789975...cc10c09

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP11 | Spec | Blocking | `crates/df-web/src/egress/tests/deny_log.rs:39,143` | "No allow action" asserted with `!contains("Allow http-intake")`, which can never fail | fix-now | fixed (f98c276) |
| S11 | Standards | P1 | `crates/df-web/src/egress/allowlist.rs:411` | `"allow"`/`"deny"` literals in `host_form` duplicate `Effect` | fix-now | fixed (f98c276) |
| S13 | Standards | P1 | `crates/df-web/src/egress/tests/paging.rs:238-262` | Task entry setup duplicates `tests/rules.rs:264` | fix-now | fixed (f98c276) |
| S14 | Standards | P1 | `crates/df-web/src/egress/tests/deny_log.rs:148` | `add_deny_rule`/`Actor` fixture repeated | fix-now | fixed (f98c276) |
| S12 | Standards | P3 | df-db, df-web test helpers | Two `notifications_before_marker` copies | accept | cosmetic |
| S15 | Standards | P3 | `crates/df-db/src/egress/deny/tests.rs:127` | Raw task INSERT | accept | cosmetic |
| S16 | Standards | P3 | `crates/df-web/src/egress/allowlist.rs:281` | Private `Effect` before public `HostForm` | accept | cosmetic |
| SP12 | Spec | P3 | `crates/df-web/assets/egress-rule.js` | Toggle only tested at source level | accept | cosmetic |
| SP9b | Spec | Rejected | — | Coverage not verifiable read-only | — | rejected (implementer ran llvm-cov: 95.94 % / 97.19 %) |

Security reviewer: 0 findings.

User decision: Fix blocking + P1.

## Round 3 (2026-11-03)

- Implementer: f98c276, fixed SP11, S11, S13, S14; checks fmt/clippy/machete/file-length/test-names ok; df-web 516 pass; container tests ran.
- Reviewed range: cc10c09...f98c276

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S17 | Standards | P3 | `crates/df-web/src/egress/tests/mod.rs:110-136` | Actor construction in two shared fixtures | accept | cosmetic |
| SP13 | Spec | P3 | `crates/df-web/src/egress/tests/deny_log.rs:111-123` | Served-page test checks only the deny endpoint | accept | cosmetic |

Security reviewer: 0 findings. No blocking findings in round 3.

### Visual check (1440 px, Playwright, `just serve-local --demo-flows`, 33 seeded rules)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| V4 | Visual | Blocking | rules page search (`#rules-search` inside swapped `#config-list`) | Typing filters the rows but the field ends up empty (`value=""`); the server renders `value="github"` | fix-now | fixed (3a4a6a1) |
| V1 | Visual | P1 | rule dialog, Deny state | Disabled "Applies to" and Port look enabled (white bg, dark text); board greys them out | fix-now | fixed (3a4a6a1) |
| V3 | Visual | P1 | pagination | Disabled Next looks like enabled Previous | fix-now | fixed (3a4a6a1) |
| V2 | Visual | P1 | rule dialog | Effect toggle takes half the row (259 px); board sizes it to content | fix-now | fixed (3a4a6a1) |
| V5 | Visual | P1 | rules table | Column positions jump between pages and searches | fix-now | fixed (3a4a6a1) |

Matches the boards: deny row, highlight `:any` with page jump, pagination footer, search count line, traffic "Deny always", toast, denied rows with "Deny rule" only. Not checked visually: request card (no pending `network-access` request in the demo data; markup test only).

User decision: Fix blocking + P1.

## Round 4 (2026-11-03)

- Implementer: 3a4a6a1, fixed V4, V1, V3, V2, V5; checks fmt/clippy/file-length/test-names ok; df-web 518 pass; container tests ran; implementer browser re-check at 1440 px ok.
- Reviewed range: f98c276...3a4a6a1

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP14 | Spec | P1 | `openspec/changes/iter2-egress-deny-rules/design.md:78` | D8 still says search targets `#config-list` | fix-now | fixed (10512ab) |
| SP15 | Spec | P1 | `crates/df-web/src/egress/tests/paging.rs:152` | No-match test does not check the text is inside `#rules-results` | fix-now | fixed (10512ab) |
| SP16 | Spec | P1 | `crates/df-web/src/egress/tests/paging.rs:280` | No test that the paging nav is inside `#rules-results` | fix-now | fixed (10512ab) |
| S18 | Standards | P1 | `crates/df-web/src/egress/tests/deny_rules.rs:37,60`, `task_pages/tests/detail.rs:731` | Disabled-attribute check inlined three times | fix-now | fixed (10512ab) |
| S19 | Standards | P1 | `crates/df-web/src/egress/allowlist.rs:302-341` | Wrapped maud blocks not re-indented | fix-now | fixed (10512ab) |
| S20 | Standards | P1 | `crates/df-web/src/egress/allowlist.rs:306` | `nth-child` widths coupled to column order | fix-now | fixed (10512ab) |
| SP17 | Spec | P2 | `crates/df-web/src/egress/allowlist.rs:227,235` | Truncated long hosts have no `title` | tech-debt | tech-debt (TD-98) |
| S21 | Standards | P3 | `crates/df-web/src/egress/tests/paging.rs:265` | Test name omits paging buttons | accept | cosmetic |
| S22 | Standards | P3 | `crates/df-web/src/egress/tests/paging.rs:292` | Class-string test without message | accept | cosmetic |

Security reviewer: 0 findings. No blocking findings in round 4.

User decision: Fix P1, then finish.

## Round 5 (2026-11-03)

- Implementer: 10512ab, fixed SP14, SP15, SP16, S18, S19, S20 (S20 as a comment on the `nth-child` widths); checks fmt/clippy/file-length/test-names ok; df-web 518 pass; container tests ran.
- Reviewed range: 3a4a6a1...10512ab

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S23 | Standards | P3 | `crates/df-web/src/testkit.rs:667-675` | The two disabled-attribute helpers repeat the match patterns | accept | cosmetic |
| S24 | Standards | P3 | `crates/df-web/src/egress/tests/deny_rules.rs:37` | `assert!(… == 0)` instead of `assert_eq!` | accept | cosmetic |
| S25 | Standards | P3 | `crates/df-web/src/egress/tests/paging.rs:41,152` | `#rules-results` slice repeated | accept | cosmetic |
| SP18 | Spec | P3 | `crates/df-web/src/egress/tests/paging.rs:41,152` | Slice runs to page end, not to the region's closing tag | accept | cosmetic |

Security reviewer: 0 findings. No blocking findings in round 5.

User decision: Finish as proposed.

## Finish (2026-11-03)

- Status: done. No blocking finding open; container-backed tests ran in every round (podman socket).
- Bazel lockfile build: not needed (no `Cargo.toml`/`Cargo.lock` change).
- Tech debt: TD-97 (O1), TD-98 (SP17).
- Roadmap: iteration-2 line and five backlog rows already added by the implementer (`product/iterations/roadmap.md`).
- Visual check: done at 1440 px (Playwright); V1–V5 fixed. Not checked visually: the request card (no pending `network-access` request in the demo data).
