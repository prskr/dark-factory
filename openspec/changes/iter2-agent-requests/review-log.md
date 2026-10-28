# Review log: iter2-agent-requests

- BASE: 4af5e3944cf489dfc5e86c1f9c8d5336a5cc1224
- Store: none
- Status: done
- Implementer agent: a3bb35d913c9a83d7

## Round 1 (2026-10-28)

- Implementer: 26b8ddb, tasks 11/13, checks fmt/clippy/machete ok, cargo tests all pass; Bazel not run (MODULE.bazel.lock hand-edited), lefthook not on PATH
- Container tests: ran (podman socket)
- Orchestrator check: `bazel build --lockfile_mode=error //crates/df-db/...` analysis ok (lock file accepted); compile hit a transient ENOSPC in the sandbox
- Open: 5.3 manual 390 px phone check (Playwright MCP down); 6.1 df-e2e part (depends on unimplemented `iter1-demo-e2e`; in-process substitute test in `df-agent-channel/tests/agent_port.rs`)
- Reviewed range: 4af5e39...26b8ddb

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-db/src/tasks.rs:694` | `mark_task_done` does not close pending requests; deciding one later re-queues the completed task | close in the same tx + test | fixed (0c6be7f) |
| P2 | Spec | Blocking | `crates/df-web/src/sse.rs:66-80` | SSE seeds request cards from DB as "already rendered"; changes between page render/reconnect and stream start are never sent | start empty or resend all on `start > 0`; reconnect test | fixed (ff98633 via P8/S15) |
| P3 | Spec | Blocking (security) | `crates/df-web/src/transcript/mod.rs:504` | agent Markdown renders `<img src>` → browser fetches agent-chosen URL on view; spec forbids auto-following links | drop/neutralize images, `rel="noopener noreferrer nofollow"` on links; test | fixed (0c6be7f) |
| S1 | Standards | P1 | `crates/factory-agent/docs/configuration.md:50-55` | Idle wait doc says `Release` ends `END_TURN`; after a blocking request it ends `BLOCKED` | update doc | fixed (0c6be7f) |
| S11 | Standards | P1 | `crates/df-db/src/requests.rs:273-277` | `raise_question` doesn't check per-answer length/emptiness (only on MCP path) | validate in `raise_question` + test | fixed (ff98633 via S14) |
| P5 | Spec | P1 | `crates/df-core/src/domain.rs:188` | `RunState::Blocked` + DB check value now unreachable | remove (pre-MVP cleanup) in migration and enum | fixed (0c6be7f) |
| S2 | Standards | P1 | `crates/df-db/src/requests.rs:340-352` | private `NewRequest`/`raise` between public fns | reorder | fixed (0c6be7f) |
| S3 | Standards | P1 | `crates/df-db/src/requests.rs:25-30` | `macro_rules! columns` before constants | reorder | fixed (0c6be7f) |
| S4 | Standards | P1 | `crates/df-web/src/requests.rs:105,128,296-312` | public/private items interleaved | reorder; make forms private | fixed (0c6be7f) |
| S7 | Standards | P1 | `crates/df-db/src/requests.rs:303-314` | `raise_approval` 9 params + `allow(too_many_arguments)` | public `NewRequest` struct param | fixed (0c6be7f) |
| S12 | Standards | P1 | `crates/df-agent-channel/tests/agent_port.rs:463-469` | hand-written `panic!` in test | `bail!`/`.context()?` | fixed (0c6be7f) |
| S13 | Standards | P1 | `crates/df-web/src/requests.rs:419` | `last: (Option<i64>, String)` tuple | named fields | fixed (0c6be7f) |
| S5 | Standards | P2 | `df-web/src/requests.rs:255-273`, `df-db/src/requests.rs:650-682` | outcome/decider text built twice | shared helper on `RequestRow`; matters when outcome wording changes | tech-debt (TD-71) |
| S8 | Standards | P2 | `df-db/src/requests.rs:606`, `df-web/src/sse.rs:109` | `(RequestRow, String, String)` / key+title clump | `TaskRef`; matters when more task fields travel with requests | tech-debt (TD-72) |
| P7 | Spec | P1 (user) | `crates/dark-factory/src/main.rs:874` | approval-type registry only in web state; channel has none | one registry shared; matters when iter2-egress-approvals lands | fixed (0c6be7f) |
| P4 | Spec | P2 | `crates/df-agent-channel/tests/agent_port.rs:817` | 6.1 e2e substitute bypasses UI handler / Release / run 2 start | e2e once iter1-demo-e2e lands (task 6.1 stays open) | tech-debt (TD-73) |
| P6 | Spec | P2 | `crates/df-web/src/layout.rs:87` | no manual 390 px check; count hidden on phone task view (bottom bar hidden there) | manual check; decide count on phone task view | accepted (user, 390 px check 2026-10-28) |
| S6 | Standards | P3 | `crates/df-db/src/requests.rs:738-750` | hand-matched kind/state strings | `FromStr` | cosmetic |
| S9 | Standards | P3 | `crates/df-agent-channel/src/tools.rs:239,271` | `Principal::agent(...)` built twice | helper | cosmetic |
| S10 | Standards | P3 | `crates/df-web/src/testkit.rs:263,323` | test registry rebuilt | store on `TestApp` | cosmetic |

User decision: Fix blocking + P1, plus P7 moved from P2 to P1. Fix list: P1, P2, P3, S1, S11, P5, S2, S3, S4, S7, S12, S13, P7.

## Round 2 (2026-10-28)

- Implementer: 0c6be7f, tasks 11/13, checks fmt/clippy/machete ok, all crate tests pass (0 failures), hooks ran
- Container tests: ran (podman socket); none skipped
- `.sqlx`: two files hand-regenerated (only a `'blocked'` literal removed from WHERE; describe unchanged). Orchestrator verified hash = sha256(query)
- Reviewed range: 26b8ddb...0c6be7f

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P8/S15 | Spec+Standards | Blocking (regression) | `crates/df-web/src/sse.rs:279-297`, `transcript/mod.rs:535` | `#live-requests` sits before all streamed items → a late card shows above events streamed after page load (incl. the `ask_question` that raised it) | `delete #request-{id}` + `#transcript beforeend` for cards newer than cut-off; drop `#live-requests`/`live`/`live_requests()` | fixed (ff98633) |
| S14 | Standards | Blocking | `crates/df-db/src/requests.rs:287-295` | suggested answers validated trimmed but stored untrimmed → 200-char limit bypassable with padding (reopens S11) | trim into a Vec, validate and store it; padding test | fixed (ff98633) |
| P9 | Spec | Blocking | `crates/df-web/src/requests/tests.rs` | no reconnect test (round-1 decision asked for one) | connect, receive, drop, decide, reconnect same URL; assert single card with decided state | fixed (ff98633) |
| P10/S16 | Spec+Standards | Blocking (changed code) | `crates/df-web/src/sse.rs:284`, `task_pages.rs:236-245` | cut-off = newest card `created_at`; concurrent raise can be classed "early" and morphed to a missing id → never shown | cut-off = page render time (DB `now()`); with P8 fix | fixed (ff98633) |
| S17 | Standards | P1 | `crates/df-db/migrations/20261028000001_agent_requests.sql:5-23` | existing dev DBs with `blocked` runs fail new CHECK / unique index | `UPDATE runs SET state='succeeded' WHERE state='blocked'` first | fixed (ff98633) |
| S18 | Standards | P1 | `crates/df-db/src/tasks.rs:690-693` | `mark_task_done` doc omits request closing | add to doc comment | fixed (ff98633) |
| S19 | Standards | P1 | `crates/df-db/src/requests.rs:777-814` | test wrappers re-add long param lists + allow(too_many_arguments) | build `Raise` via small helper | fixed (ff98633) |
| S21 | Standards | P1 | `crates/df-web/src/sse.rs:49-50` | query param `requests: Option<i64>` holds unix µs | rename (e.g. `cards_until`) | fixed (c14f726 via S25) |
| S20 | Standards | Rejected | `crates/df-agent-channel/src/lib.rs:62` | unused `approval_types` field on channel | — | rejected (user moved P7 to fix-now) |
| S22 | Standards | P3 | `crates/factory-agent/docs/configuration.md` | new doc line ~170 cols | rewrap | cosmetic |

User decision: Fix blocking + P1. Fix list: P8/S15, S14, P9, P10/S16, S17, S18, S19, S21.

## Round 3 (2026-10-28)

- Implementer: ff98633, checks fmt/clippy/machete ok, df-db 67 + df-web 368 pass; `sse::tests::streams_end_when_the_session_token_is_due_for_refresh` flaked once (pre-existing test)
- Container tests: ran (podman socket); none skipped
- Implementer notes: residual narrow race for P10 (tx started before cut-off, committed after listing); S14 test written after fix; S17 not run against a DB with blocked runs
- Reviewed range: 0c6be7f...ff98633

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P11 | Spec | Blocking (changed code) | `crates/df-web/src/sse.rs:276-291`, `requests/tests.rs:384-395` | late card re-appended (delete+beforeend) on every change → jumps to bottom on decision; test locks it in (caused by orchestrator's round-2 fix instruction) | delete+append only on first send by this connection, else outerMorph; fix test | fixed (c14f726) |
| S24 | Standards | Blocking (changed code) | `crates/df-web/src/requests/tests.rs:283-297` | `delete < append && …` on `Option`s passes when delete partial is missing (`None < Some`) | unwrap each `find` and compare `usize` | fixed (c14f726) |
| P12/S23 | Spec+Standards | Blocking (changed code) | `crates/df-web/src/task_pages.rs:324`, `sse.rs:286` | residual race: raise tx starting before `clock_timestamp()` cut-off but committing after card listing → card silently invisible on that page | treat cards within a margin (e.g. 5 s) before cut-off as late on first send (idempotent delete+append); `ponytail:` comment; test | fixed (c14f726) |
| S28 | Standards | P1 | migration `:6` | migrated blocked runs keep outcome unset | `outcome = COALESCE(outcome, 'blocked')` | fixed (c14f726) |
| S25 | Standards | P1 | `crates/df-web/src/sse.rs:75,105` | stream field still `rendered_until` | rename `cards_until` | fixed (c14f726) |
| S26 | Standards | P1 | `crates/df-web/src/requests/tests.rs:358` | stale comment ("newest card time") | "database time at render" | fixed (c14f726) |
| S27 | Standards | P1 | `crates/df-db/src/tasks.rs:692` | ~150-col doc line | rewrap | fixed (c14f726) |
| S29 | Standards | P1 | `crates/df-db/src/requests.rs:789` | test helper `from(...)` reads like `From::from` | rename `raise_by` | fixed (c14f726) |

User decision: Fix blocking + P1. Fix list: P11, S24, P12/S23, S25, S26, S27, S28, S29.

## Round 4 (2026-10-28)

- Implementer: c14f726, checks fmt/clippy/machete ok, df-db 67 + df-web 369 pass; tests seen failing first
- Container tests: ran (podman socket); none skipped
- Implementer notes: cards created within 5 s before render may move once to the end on first send; S28 migration not run against DB with blocked runs
- Reviewed range: ff98633...c14f726

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S31 | Standards | Blocking (MUST: panic on input) | `crates/df-web/src/sse.rs:296` | `cards_until - CUT_OFF_MARGIN_MICROS` with `cards_until` from the query string overflows (debug panic, release wrap) | `saturating_sub` + test | fixed (c7bba53) |
| P13 | Spec | Blocking (major regression) | `crates/df-web/src/sse.rs:294-299` | page-rendered card raised ≤5 s before render is deleted and appended at the end on stream start → out of time order | place each first-sent card by `created_at` against folder items (delete, then `beforebegin` first newer item, else `#transcript beforeend`); drop margin + late distinction | fixed (89ccd17) |
| P14 | Spec | Blocking (changed code) | `crates/df-web/src/sse.rs:286-299` | on every reconnect, cards raised after render jump below later events | same fix as P13 | fixed (89ccd17) |
| S33 | Standards | P1 | `crates/df-web/src/requests/tests.rs:386-406` | no test left for in-place first send of a page-rendered card | test: rendered card stays in place on stream start | fixed (c7bba53) |
| S30 | Standards | P1 | `crates/df-web/src/sse.rs:105-107` | `cards_until` field doc ignores margin | update doc (moot if P13 removes margin) | fixed (c7bba53) |
| S32 | Standards | P3 | `crates/df-web/src/sse.rs:45-47` | margin const as raw µs vs `Duration` neighbours | — | cosmetic |

Fix-round limit (3) reached with blocking findings open: S31, P13, P14.

User decision: one extra fix round (4) beyond the limit. Fix list: S31, P13, P14, S33, S30 (place cards by time; drop margin/cut-off if no longer needed).

## Round 5 (2026-10-28)

- Implementer: c7bba53, checks fmt/clippy/machete ok, df-db 67 + df-web 369 pass; pre-existing flaky `streams_end_when_the_session_token_is_due_for_refresh` failed once
- Container tests: ran (podman socket); none skipped
- Cards now placed by time (`beforebegin` first newer folder item); `cards_until`, margin, `database_time` removed
- Reviewed range: c14f726...c7bba53

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P15 | Spec | Blocking (major) | `crates/df-web/src/sse.rs:160` | first step places cards after folding only the first BATCH (500) rows → with longer transcripts cards jump to the end on every load/reconnect | `if self.check_requests && !full`; test with > BATCH events | fixed (89ccd17) |
| P16/S35 | Spec+Standards | Blocking (changed code) | `crates/df-web/src/sse.rs:289` vs `transcript/mod.rs:530` | tie-break differs (`>` vs render's `<=`) → equal-timestamp card moves on first send | one shared predicate in `transcript`, used by both; equal-timestamp test | fixed (89ccd17) |
| P17 | Spec | P1 | `crates/df-web/src/requests/tests.rs:327` | no tests for > BATCH resume / equal timestamps | tests with P15/P16 | fixed (89ccd17) |
| S34/P18 | Spec+Standards | P1 | `crates/df-web/src/requests/tests.rs:408` | stale comment about `cards_until` | drop clause | fixed (89ccd17) |

User decision: one more fix round (5). Fix list: P15, P16/S35, P17, S34/P18.

## Round 6 (2026-10-28)

- Implementer: 89ccd17, checks fmt/clippy/machete ok, df-web 371 pass (df-db untouched, not rerun)
- Container tests: ran (podman socket); none skipped
- Reviewed range: c7bba53...89ccd17

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P19 | Spec | P1 | `crates/df-web/src/requests/tests.rs:326` | no test for first connect (`after=0`) with > BATCH rows (code correct by reading) | add `after=0` variant | fixed (7f7e5b8) |
| S36 | Standards | P1 | `crates/df-web/src/requests/tests.rs:524-537` | `card_goes_before` unit test in the requests suite | move to `transcript/tests.rs` | fixed (7f7e5b8) |
| P20 | Spec | P1 | `crates/df-web/src/transcript/mod.rs:529`, `sse.rs:262` | docs say "before the first newer item"; rule is `<=` | reword / point to `card_goes_before` | fixed (7f7e5b8) |
| S37 | Standards | P1 | `crates/df-web/src/transcript/mod.rs:525` | `card_goes_before` is `pub` | `pub(crate)` | fixed (7f7e5b8) |
| S38 | Standards | P3 | `crates/df-web/src/requests/tests.rs:331-345` | `BATCH + 10` repeated | name it | cosmetic |

No blocking findings open.

User decision: fix P1 (P19, S36, P20, S37) in a final round; orchestrator verifies the diff without full re-review; then finish.

## Round 7 (2026-10-28)

- Implementer: 7f7e5b8, checks fmt/clippy/machete ok, df-web 372 pass; container tests ran, none skipped
- Verified by the orchestrator (user decision, no reviewer pass): P19, S36, P20, S37 fixed. Cosmetic: `crates/df-web/src/sse.rs:263` doc line ~120 cols.

## Result

- Status: done. No blocking finding open.
- Tech debt: TD-71 (S5), TD-72 (S8), TD-73 (P4). P6 accepted after the 390 px check (Playwright, 2026-10-28); recorded under "Accepted decisions" in `docs/tech_debt.md`
- Open tasks: 6.1 (e2e part, TD-73; depends on `iter1-demo-e2e`)
