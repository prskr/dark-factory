# Review log: egress-log-initial-backlog

- BASE: a6aa12e7f495eda3fac21d40f7f5fdd4c771dccb
- Store: none
- Status: done
- Implementer agent: one implementer for all rounds (session-local ID omitted)

## Round 1 (2026-10-26)

- Implementer: 63d4fe2, tasks 4/4, checks fmt/clippy/machete ok, df-web 319 pass, openspec validate --strict ok; manual `just serve-local`/obscura check of task 1.3 not run, container tests ran (podman socket)
- Reviewed range: BASE...63d4fe2

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1a | Standards | P1 | `crates/df-web/src/egress/mod.rs:20` | Page now waits for `backlog_end`; bound (relay 2 s) is deliberate per design D3 but `FlowSource::subscribe` doc does not promise it | document the bound on `subscribe` | fixed (9642630) |
| S1b | Standards | P2 | `crates/df-web/src/egress/log/mod.rs:53` | Page subscribes and drops; `/events` may start a cold relay session again → first viewer can wait 2× 2 s | matters when page loads feel slow on a cold relay | fixed (0fccfe7) |
| P1 | Spec | P1 | `openspec/changes/egress-log-initial-backlog/tasks.md:13` | Task 1.3 checked but the `just serve-local` + obscura check was not run | run it (socket available) or uncheck and note | fixed (9642630) |
| P2 | Spec | P1 | `openspec/changes/egress-log-initial-backlog/design.md:40` | Filter changes (`hx-get` + `hx-select="#traffic"`) now also wait for the backlog; Risks only mention page loads | add one sentence to Risks | fixed (9642630) |
| S2+S3+S4 | Standards | P1 | `crates/df-web/src/egress/tests.rs:1703` | `element` helper duplicates `part()`, hand-rolled `panic!`, doc names nonexistent params | extract shared `inner(html, open, end) -> Option<&str>`, build `part` on it, fix doc | fixed (9642630) |
| S5 | Standards | P1 | `crates/df-web/src/egress/log/page.rs:218` | `filter_bar` takes `&Backlog` but only reads `counts` | pass `&Counts` | fixed (9642630) |
| S6 | Standards | P3 | `crates/df-web/src/egress/log/viewer.rs:74` | `graph: String` next to `rows: Markup` without a reason | optional field comment | cosmetic |
| P3 | Spec | P3 | `crates/df-web/src/egress/tests.rs:1712` | "each flow once" follows from `innerHTML` swap, not asserted directly | none | cosmetic |

User decision: Fix blocking + P1 (S1a, P1, P2, S2+S3+S4, S5); S1b to tech debt.

## Round 2 (2026-10-26)

- Implementer: 9642630, tasks 4/4, checks fmt/clippy/machete ok, df-web 319 pass; obscura check of `/egress/log` run (42 rows, 13 graph nodes, counts 41), container tests ran (podman socket)
- Reviewed range: 63d4fe2...9642630

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-S1+R2-P1 | Standards+Spec | Blocking | `crates/df-web/src/egress/tests.rs:1742` | Segment-count assertion compares `Option`s (`inner(..)` vs `part(..)`); a span missing on both sides passes as `None == None`. Regression from fix round (old helper panicked) | `.expect(..)` / `.unwrap()` on both sides | fixed (bde3bed) |
| R2-S2 | Standards | P1 | `crates/df-web/src/egress/mod.rs:23` | Trait doc hard-codes the relay's 2 s; goes stale if the relay changes | keep "within a short bound", point to the relay instead of the number | fixed (bde3bed) |

User decision: Fix blocking + P1 (R2-S1+R2-P1, R2-S2).

## Round 3 (2026-10-26)

- Implementer: bde3bed, tasks 4/4, checks fmt/clippy ok, df-web 319 pass, container tests ran (podman socket)
- Reviewed range: 9642630...bde3bed

No new findings (standards and spec). R2-S1+R2-P1 and R2-S2 confirmed fixed.

User decision: S1b moved from P2 to fix-now (relay grace period in `df-gateway`), one more fix round.

## Round 4 (2026-10-26)

- Implementer: 0fccfe7, tasks 4/4, checks fmt/clippy/machete ok, df-gateway 76+2 pass, df-web 319 pass, container tests ran (podman socket); tests written together with the fix (red phase not observed)
- Reviewed range: bde3bed...0fccfe7

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R4-P1 | Spec | Blocking | `openspec/specs/egress-log/spec.md:41,47` vs change delta | Main spec: streams "exist only while viewers are connected", "Last viewer leaves → closes `Tail`"; code keeps them 5 s. Delta does not modify "Relay across gateway pods" | add MODIFIED requirement + grace scenario to delta | fixed (100eb33) |
| R4-P2 | Spec | Blocking | `design.md:42`, `proposal.md:26` | "One more `Tail` per page load, ends with `backlog_end`" now wrong; grace decision not recorded | D4 + rewrite Risks/proposal line | fixed (100eb33) |
| R4-P4 | Spec | Blocking | `crates/df-gateway/src/control/relay/tests.rs:401` | No test that pod `Tail` streams actually close after the real grace (all close tests use `grace = ZERO`) or that a returning viewer skips `BACKLOG_WAIT` | one `FakeDirectory` test with real grace | fixed (100eb33) |
| R4-P3 | Spec | P1 | `tasks.md`, `proposal.md:25` | Relay change has no task; Impact lists only df-web | add task 1.4, list relay.rs + design/egress.md | fixed (100eb33) |
| R4-S1 | Standards | P1 | `crates/df-gateway/src/control/relay.rs:166` | `Option<Arc<Session>>` + `expect` in `Deref` unnecessary; keep `Arc<Session>`, clone into linger task | simplify | fixed (100eb33) |
| R4-S2 | Standards | P1 | `crates/df-gateway/src/control/relay.rs:184` | Silent return without runtime undocumented | one-line comment | fixed (100eb33) |
| R4-S4 | Standards | P1 | `crates/df-gateway/src/control/relay.rs:118` | `session` binding is a `Lease` | rename `lease` | fixed (100eb33) |
| R4-S5 | Standards | P1 | `crates/df-gateway/src/control/relay/tests.rs:402` | `std::sync::atomic::…` spelled out 3× | `use` import | fixed (100eb33) |
| R4-S3 | Standards | P3 | `crates/df-gateway/src/control/relay.rs:151` | Lease outliving Relay spawns an unaborted linger task (bounded by grace) | none | cosmetic |

Fix-round limit (3) reached with blocking findings open.

User decision: one more fix round past the limit: R4-P1, R4-P2, R4-P4 (blocking) + R4-P3, R4-S1, R4-S2, R4-S4, R4-S5 (P1).

## Round 5 (2026-10-26)

- Implementer: 100eb33, tasks 5/5 (1.4 added), checks fmt/clippy/machete ok, df-gateway 77+2 pass, df-web 319 pass, openspec validate --strict ok, container tests ran (podman socket); red phase of new close test observed (grace = ZERO fails)
- Reviewed range: 0fccfe7...100eb33

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R5-S1 | Standards | P3 | `crates/df-gateway/src/control/relay.rs:182` | No-runtime comment says the session "closes at once"; only this lease is released at once | none | cosmetic |

All round-4 rows confirmed fixed by both reviewers.

User decision: Finish.

## Result

- Status: done, no blocking finding open. Tasks 5/5, 5 review rounds, 4 fix rounds (one past the limit, approved by the user).
- Tech debt: none created (S1b, the only P2, was fixed in round 3/4).
