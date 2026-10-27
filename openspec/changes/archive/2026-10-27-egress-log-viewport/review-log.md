# Review log: egress-log-viewport

- BASE: ac45d41be51762593278651e6a927aab1990f06b
- Store: none
- Status: done
- Implementer agent: afb97f0082ab6b7b9

## Round 1 (2026-10-27)

- Implementer: a03a4dc (86d1407, 479c62b, a03a4dc), tasks 10/10, checks fmt/clippy/machete ok, container tests ran via podman (df-gateway control::tests 4/4, df-web egress 93/93); not run: dark-factory crate tests
- Reviewed range: ac45d41...a03a4dc

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1+P2 | Spec | Blocking | `crates/df-web/src/egress/log/page.rs:139` | Scrollbar guard uses `offsetX/offsetY` (relative to `e.target`, not the scroll box); drag on a tall graph's background is ignored below one panel height. Drag logic untested (scenario "Tall graph") (verified) | fix-now | fixed (ae6f6f2) |
| S8+P3 | Both | P1 | `crates/df-core/src/config.rs:216` | `bufferMaxAgeSeconds = 0` accepted; design says minimum 1, only Helm schema enforces it | fix-now | fixed (ae6f6f2) |
| S1 | Standards | P1 | `crates/df-gateway/src/flow.rs:39` | Private `FlowKey`/`flow_key` above public items | fix-now | fixed (ae6f6f2) |
| S2 | Standards | P1 | `crates/df-gateway/src/flow.rs:415,430` | `open_flows`/`expired` pub but only used in flow.rs | fix-now | fixed (ae6f6f2) |
| S5 | Standards | P1 | `crates/dark-factory/src/main.rs:19` | `Duration::from_secs(buffer_max_age_seconds)` twice | fix-now | fixed (ae6f6f2) |
| P4 | Spec | P1 | `design.md` Decision 3 | Design says a running open-flow set; code recomputes from the buffer | fix-now (update design.md) | fixed (ae6f6f2) |
| P5 | Spec | P1 | `crates/df-gateway/src/flow.rs:561` | No test at the 61 min edge | fix-now | fixed (ae6f6f2) |
| S4 | Standards | P2 | `flow.rs` / `relay.rs` | `FlowLog` and `Merged` repeat cutoff/prune/unexpired steps | tech-debt | tech-debt (TD-61) |
| S3 | Standards | P3 | `flow.rs:437` | `AgedEvents.events` pub field | cosmetic | cosmetic |
| S6 | Standards | P3 | `flow.rs:390` | 3600 default in two places (follows `DEFAULT_CAPACITY` pattern) | cosmetic | cosmetic |
| S7 | Standards | P3 | `relay/tests.rs` | `with_max_age(FOREVER)` repeated in 6 tests | cosmetic | cosmetic |

User decision (round 1): Fix blocking + P1. S4 to tech debt, P3 cosmetic.

## Round 2 (2026-10-27)

- Implementer: ae6f6f2, tasks 10/10, checks fmt/clippy/machete ok, container tests via podman (relay); not run: dark-factory crate tests
- Reviewed range: a03a4dc...ae6f6f2

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S9+P6 | Both | Blocking | `crates/df-gateway/src/control/relay.rs:113` | Relay clamp (`MIN_MAX_AGE`) has no regression test; only FlowLog does (AGENTS.md: fixed bug needs a test) | fix-now | fixed (a644e41) |
| P7 | Spec | P1 | `crates/df-gateway/README.md:87`, `deploy/helm/dark-factory/README.md:117` | Reference rows omit "minimum 1" | fix-now | fixed (a644e41) |
| S11 | Standards | P1 | `crates/df-web/src/egress/log/page.rs:139` | `r` should be `rect` (line changed this round) | fix-now | fixed (a644e41) |
| S10 | Standards | P3 | `flow.rs:100`, `relay.rs:113` | Clamp in two builders (defence in depth) | cosmetic | cosmetic |

User decision (round 2): Fix blocking + P1.

## Round 3 (2026-10-27)

- Implementer: a644e41, tasks 10/10, checks fmt/clippy/machete ok
- Reviewed range: ae6f6f2...a644e41

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S12+P1r3+S13 | Both | Blocking | `crates/df-gateway/src/control/relay/tests.rs:561-577` | New clamp test reads the wall clock (`timestamp_now().seconds - 1`) and flakes if a second boundary falls between reads; AGENTS.md forbids timing-based tests. Also builds a Relay + Merged just to read one field and duplicates the FlowLog test | fix-now: `assert_eq!(relay.max_age, MIN_MAX_AGE)` | fixed (c509fd1) |

User decision (round 3): Fix blocking + P1.

User decision (round 3 final): fix round 3 reviewed by orchestrator directly (one-assertion diff c509fd1); no blocking finding open.
Tech debt created: TD-61.
