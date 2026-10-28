# Review log: hide-system-prompt-blocks

- BASE: fa51d777e21229b0dc9bb6bc749fb9d78537d525
- Store: none
- Status: done
- Implementer agent: a4e354907d8b185f8

## Round 0 (implementation, 2026-10-28)

- Implementer: 7c6d0ac, tasks 19/19, checks fmt/clippy/machete/buf/helm ok; crate tests passed before Podman state broke
- Container tests: ran until Podman state broke (`unable to open database file`); later reruns failed in df-web auth/task_pages and df-agent-channel assign tests. Fixture `df-test-postgres-17-alpine-r1` needs `podman rm -f` by the user.
- Not run: browser check (needs working Postgres/Dex), 4 `dark-factory` serve tests (wasm plugin not built, unrelated)

## Round 1 (2026-10-28)

- Implementer: 7c6d0ac, tasks 19/19
- Reviewed range: fa51d77...7c6d0ac

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP2 | Spec | Blocking | `df-web/src/task_pages.rs:276,307`, `sse.rs:64,225`, `testkit.rs:248` | No handler/SSE test sets `show_hidden_blocks=true`; config-to-page wiring untested (scenario "option on, transcript shows muted+label") | fix-now | open |
| SP1 | Spec | P2 | `df-web/src/task_pages/view.rs:372` | Queued stack shows hidden text plain, no label/muted; spec says "both places". Unreachable today (only human messages queued) | tech-debt | open |
| S1 | Standards | P1 | `factory-agent/src/tests.rs:4093` | `let ... else { panic! }` guard in test | fix-now | open |
| S5 | Standards | P1 | `df-db/src/tasks.rs:502` | `edit_queued_message` builds block inline instead of `text_block` | fix-now | open |
| S7 | Standards | P1 | `factory-agent/src/idle_tests.rs:51` | `hidden: false` in pattern makes `chunk_text` drop hidden chunks; use `..` | fix-now | open |
| S2 | Standards | P1 | `df-web/src/task_pages/tests.rs:1699` | New CSS test duplicates the edited one | fix-now | open |
| S6 | Standards | P2 | 48 sites | `hidden: false` everywhere; add `ContentBlock::text()` constructor | tech-debt | open |
| S3 | Standards | P3 | `transcript/mod.rs:39` | `(String, bool)` tuple parts | cosmetic | open |
| S4 | Standards | P3 | `task_pages/view.rs:347` | bool threaded through signatures | cosmetic | open |
| SP3 | Spec | P3 | `transcript/tests.rs:320` | Test uses hand-written JSON | cosmetic | open |
| SP4 | Spec | P3 | `transcript/tests.rs` | No test names the "no hidden key" scenario; covered indirectly | cosmetic | open |

User decision: Fix blocking + P1 (SP2, S1, S5, S7, S2). SP1 and S6 go to tech debt; S3, S4, SP3, SP4 cosmetic.

## Round 2 (2026-10-28)

- Implementer: 7e4be30, tasks 19/19, fmt/clippy/machete ok; fixed SP2, S1, S5, S7, S2
- Container tests: skipped (Podman broken). New SP2 tests compile but have NOT been seen to pass: `task_pages::tests::hidden_blocks_are_labelled_on_the_task_page_only_with_the_option`, `sse::tests::hidden_blocks_are_labelled_in_the_stream_with_the_option` (failed at container start)
- Reviewed range: 7c6d0ac...7e4be30

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP2 | Spec | Blocking | wiring tests | Fixed by 7e4be30; handler/SSE tests exist but have not been seen to pass (container) | verify in CI | fixed (7e4be30) |
| S1, S5, S7, S2 | Standards | P1 | see round 1 | Verified fixed | | fixed (7e4be30) |
| R2-1 (S3', SP-P1) | Both | P1 | `df-web/src/sse/tests.rs:78` | SSE test covers option on only; a hardcoded `true` in `sse.rs:65` would pass. Add option-off case | fix-now | fixed (53b35c9) |
| R2-2 (S1') | Standards | P3 | `df-web/src/testkit.rs:232` | Default forge repeated in `new()` and `with_hidden_blocks()` | cosmetic | cosmetic |
| R2-3 (S2') | Standards | P3 | `components/transcript.rs:40` + 3 test files | Label literal copied into tests; no const | cosmetic | cosmetic |
| R2-4 (SP-P2) | Spec | P2 | `df-web/src/sse.rs:225` | Pending-region flag path untested; same as SP1 | tech-debt (with SP1) | open |

User decision (round 2): Fix blocking + P1 (R2-1). R2-4 merges into SP1 as tech debt; R2-2, R2-3 cosmetic.

## Round 3 (2026-10-28)

- Implementer: 53b35c9; fixed R2-1 (SSE test now covers option on and off). Orchestrator read the diff (one test file): no findings.
- Container tests: Podman restored after the review; all touched crates pass on 53b35c9 including both SP2 tests. Browser check done by hand (see Outcome).

## Outcome

- Status: done (no blocking finding open)
- Fixed: SP2, S1, S5, S7, S2, R2-1
- Tech debt: SP1 and R2-4 -> TD-69, S6 -> TD-70
- Cosmetic: S3, S4, SP3, SP4, R2-2, R2-3
- Rejected: none
- Verification after Podman was restored (2026-10-28): `cargo test` for df-web, df-db, df-agent-channel, df-core, df-proto, factory-agent all pass at 53b35c9 (350 df-web incl. both SP2 tests, 54 df-db, 114+25+1 df-agent-channel, 107 factory-agent). Local control plane with a seeded task: option on shows the description then a muted "Instruction for the agent" block; option off (default) shows only the description.
