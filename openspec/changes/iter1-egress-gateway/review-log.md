# Review log: iter1-egress-gateway

- BASE: e0cd5aa96059313474a0e62d4f6bd082f2143918
- Store: none
- Status: done
- Implementer agent: a2ca61ad60d64eb4a

Standing user decision (2026-10-23): unattended run. Every gate: "Fix blocking + P1". Fix-round limit raised to 5; then stop and wait. Finish (tech debt + ledger commit) allowed; squash needs explicit approval.

## Round 1 (2026-10-23)

- Implementer: 462ae67, tasks 12/13 (5.1 open: needs dev cluster, manual), checks fmt/clippy/machete/deny/cargo build/tests/helm all ok, df-gateway coverage 94.9 %, container tests ran (podman socket)
- Reviewed range: BASE...462ae67

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/df-gateway/src/proxy.rs:210,383` | Tunnel tasks and hyper client connection spawned without owner | fix-now | fixed (9002249) |
| S2 | Standards | Blocking | `crates/df-gateway/src/control/policy_service.rs:70` | PolicyWatch stream task spawned without owner | fix-now | fixed (9002249) |
| S3 | Standards | Blocking | `crates/df-gateway/src/proxy.rs:379-387` | Upstream HTTP handshake and response wait without timeout | fix-now | fixed (9002249) |
| S4 | Standards | Blocking | `crates/df-gateway/src/watch.rs:38-40` | `unwrap_err()` in production watch loop | fix-now | fixed (9002249) |
| C2+S6 | Spec+Standards | Blocking | `crates/df-gateway/src/proxy.rs:262,276-278,330-352` | bytes in/out missing for plain HTTP and cancelled tunnels; tunnel I/O errors swallowed | fix-now | fixed (9002249) |
| S7 | Standards | P1 | `crates/df-gateway/src/control/snapshot.rs:59,81` | Corrupt version/port silently defaulted; bad port becomes 443 (widens policy) | fix-now | fixed (9002249) |
| S5 | Standards | P1 | `crates/df-web/src/egress/mod.rs:27` | `ServiceLookup::exists` returns `Result<bool, String>` | fix-now | fixed (9002249) |
| S8+S9 | Standards | P1 | `crates/df-gateway/src/control/snapshot.rs:36-57`, `crates/df-db/src/egress.rs` | Runtime `query_as` and raw SQL in df-gateway; move to `df_db::egress` with `query_as!` + `.sqlx` | fix-now | fixed (9002249) |
| S10 | Standards | P1 | `crates/df-gateway/src/proxy.rs:184-206,299-322` | Duplicated authorize/resolve logic in CONNECT and HTTP paths | fix-now | fixed (9002249) |
| S15 | Standards | P1 | `proxy/tests.rs:118`, `gateway/tests.rs:150,231`, `control/relay/tests.rs:58,194` | Sleep-polling tests; relay test passes silently on timeout | fix-now | fixed (9002249) |
| C3 | Spec | P1 | `crates/df-gateway/src/proxy.rs:262`, `gateway.proto:108` | Run-ended tunnels logged as `removed-from-allowlist`; proto reason list incomplete | fix-now | fixed (9002249) |
| C4 | Spec | P1 | `crates/df-gateway/src/proxy/tests.rs:263` | Plain HTTP from unknown source untested | fix-now | fixed (9002249) |
| C6a | Spec | P1 | `crates/dark-factory/src/main.rs:166-180` | Namespace restriction on Service lookup untested | fix-now (keep + test) | fixed (9002249) |
| C1 | Spec | P2 | `crates/df-gateway/src/proxy.rs:256-272` | Allowed tunnels appear in live log only on close; matters when long-lived streams must be visible while open | defer | tech-debt (TD-48) |
| S13 | Standards | P2 | `snapshot.rs:82`, `df-db/src/egress.rs:642`, proxy.rs reasons | Scope and reason strings as scattered literals; matters when adding reasons | defer | tech-debt (TD-49) |
| C5 | Spec | P3 | `design.md` D3 | Code is stricter than D3 (any blocked address refuses); update D3 via /opsx:update | artifact note | cosmetic |
| C6b | Spec | P3 | `design.md` D6 | Denied-hosts summary built in df-web, not relay; update D6 | artifact note | cosmetic |
| S11 | Standards | P3 | `tail.rs:73`, `policy_service.rs:158` | Duplicated DNS-SAN guard | — | cosmetic |
| S12 | Standards | P3 | `df-core/src/config.rs`, `flow.rs:11-13`, `relay.rs:33,112` | Duplicated defaults and gap-event mapping | — | cosmetic |
| S14 | Standards | P3 | `policy.rs:33`, `proxy.rs:441` | `status()` returns u16 instead of `StatusCode` | — | cosmetic |
| S16 | Standards | P3 | `proxy.rs:61,78-89` | Private items before public ones | — | cosmetic |

User decision: Fix blocking + P1 (standing unattended decision)

## Round 2 (2026-10-23)

- Implementer: 9002249, tasks 12/13, checks fmt/clippy/machete/cargo build/tests/helm ok, container tests ran (podman socket)
- Reviewed range: 462ae67...9002249

Ledger check: S1–S15, C2–C6a verified-fixed by both reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S17+C7 | Standards+Spec | Blocking | `crates/df-gateway/src/proxy.rs:139-144` | `serve` aborts tunnel tasks right after `cancel()`; `gateway-shutdown` event never recorded, untested | fix-now | fixed (5b9a2e0) |
| S18 | Standards | Blocking | `crates/df-gateway/src/proxy.rs:852-884` | After upstream timeout the spawned hyper connection task keeps waiting on the silent peer (socket + task leak per timeout) | fix-now | fixed (5b9a2e0) |
| S19 | Standards | P1 | `crates/df-gateway/README.md:19-29` | Behaviour table misses new reasons/timeout and skipped invalid-port entries | fix-now | fixed (5b9a2e0) |
| C8 | Spec | P1 | `crates/df-gateway/src/proxy.rs` `send_request` Err branch | Failed HTTP request records 0 bytes although body was partly sent | fix-now | fixed (5b9a2e0) |
| S20 | Standards | P3 | `crates/df-gateway/src/count.rs:376-401` | Boxed `OnEnd` closure stored on struct | — | cosmetic |
| S21 | Standards | P3 | `crates/df-gateway/src/gateway.rs:518` | `policy_tx` parameter exists for tests | — | cosmetic |

Note: reviewer mention of `tokio::spawn` in `tail.rs:116` (test code) and `relay.rs:139` (owned `JoinHandle` on `Session`) checked: no finding.

User decision: Fix blocking + P1 (standing unattended decision)

## Round 3 (2026-10-23)

- Implementer: 5b9a2e0, tasks 12/13, checks fmt/clippy/machete ok, df-gateway tests 59 ok (only df-gateway changed), container tests ran
- Reviewed range: 9002249...5b9a2e0

Ledger check: S17+C7, S18, S19, C8 verified-fixed by both reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C9 | Spec | Blocking | `crates/df-gateway/src/proxy.rs:273-274` | `gateway-shutdown` recorded as `Verdict::Denied`; every rollout floods denied-hosts summary and denied-only filter with allowed hosts (regression from C7 fix) | fix-now | fixed (b2895a1) |
| S22+C10 | Standards+Spec | Blocking | `crates/df-gateway/src/proxy.rs:143-149` | During the 2 s drain, `connections` keep serving; HTTP upstream tasks spawned then land in the fresh `self.tasks` set and are never aborted (no owner, Arc cycle) | fix-now | fixed (b2895a1) |

User decision: Fix blocking + P1 (standing unattended decision)

## Round 4 (2026-10-23)

- Implementer: b2895a1, tasks 12/13, checks fmt/clippy ok, df-gateway proxy tests 20 ok, no container tests involved
- Reviewed range: 5b9a2e0...b2895a1

Ledger check: C9 verified-fixed; S22+C10 fixed but regressed (S23+C11).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S23+C11 | Standards+Spec | Blocking | `crates/df-gateway/src/proxy.rs:145` | `connections.abort_all()` before the drain drops in-flight plain-HTTP `forward` futures: no flow event (regression from S22 fix) | fix-now | fixed (0a9f8bb) |
| C12 | Spec | P1 | `crates/df-gateway/src/proxy/tests.rs` ~519, ~567 | Denied verdict of `removed-from-allowlist` / `unknown-source` tunnel closes not asserted | fix-now | fixed (0a9f8bb) |

User decision: Fix blocking + P1 (standing unattended decision)

## Round 5 (2026-10-23)

- Implementer: 0a9f8bb, tasks 12/13, checks fmt/clippy/machete/cargo build ok, df-gateway 60 tests ok (proxy tests 8 runs stable), container tests ran
- Reviewed range: b2895a1...0a9f8bb

Ledger check: S23+C11 and C12 verified-fixed by both reviewers. No blocking finding open.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C13+S24 | Spec+Standards | P1 | `crates/df-gateway/src/proxy.rs:414-425,495-503`, `README.md:31` | Streaming plain-HTTP body cut at shutdown is recorded with empty reason, not `gateway-shutdown`; README overstates | fix-now | fixed (584fb10) |
| S26 | Standards | P1 | `crates/df-gateway/src/proxy/tests.rs:695` | `read_to_string` without timeout; test hangs on regression | fix-now | fixed (584fb10) |
| S25 | Standards | P3 | `crates/df-gateway/src/proxy.rs:429-443` | Status chosen by comparing reason strings | — | cosmetic |

User decision: Fix blocking + P1 (standing unattended decision); fix round 5 of 5

## Round 6 (2026-10-23)

- Implementer: 584fb10, tasks 12/13, checks fmt/clippy/machete ok, df-gateway 61 tests ok, container tests ran
- Reviewed range: 0a9f8bb...584fb10

Ledger check: C13+S24 and S26 verified-fixed by both reviewers. Spec axis: 0 new findings.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S27 | Standards | P2 | `crates/df-gateway/src/proxy/tests.rs:720-724` (+ :646, :692) | Origin-side waits in shutdown tests have no timeout; matters when a regression makes tests hang | defer (fix-round limit reached) | tech-debt (TD-50) |
| S28 | Standards | P3 | `crates/df-gateway/src/proxy.rs:419,427` | Token clone named `stop` instead of `root` | — | cosmetic |
| S29 | Standards | P3 | `crates/df-gateway/src/proxy/tests.rs:739` | `assert_eq!` without message | — | cosmetic |

## Result

- Status: done. No blocking finding open. Fix rounds used: 5 of 5.
- Tasks 12/13; 5.1 (dev-cluster verification) is manual and open.
- Tech debt: TD-48 (C1), TD-49 (S13, S25), TD-50 (S27).
- Artifact notes for /opsx:update: C5 (design D3: any blocked address refuses the name), C6b (design D6: denied-hosts summary built in df-web); C6a namespace restriction of the Service lookup is not in the spec.

# Iteration 2 (2026-10-24)

- BASE_2: f0b7d0e (spec update f6b746e: opened events; tasks 6.1–6.3 and 7.1–7.2)
- Trigger: first cluster run. Egress log duplicated rows after SSE reconnect; `git clone` of a dual-stack forge failed (hickory default prefers IPv6, cluster has no IPv6 egress).
- User decision: implement sections 6 and 7 now.
- Note (user): make the IP family preference configurable (prefer IPv6 once nodes have IPv6) and add address fallback: TD-51, added by the implementer in the iteration 2 commit.

## Iteration 2, Round 1 (2026-10-24)

- Implementer: 625e42e, tasks 17/18 (5.1 manual; git-clone check of 7.2 manual), checks fmt/clippy/machete/cargo build/buf ok, tests df-proto 3, df-gateway 65, df-web 235, dark-factory 11, container tests ran
- Reviewed range: f0b7d0e...625e42e

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C15 | Spec | Blocking | `crates/df-web/src/egress/log.rs:162-165` | `opened` row keeps the "open" badge after its connection closed; page can't show which connections are open now (design D6, TD-48 goal) | fix-now | fixed (e595f37) |
| C17 | Spec | Blocking | `crates/df-web/src/egress/tests.rs` | Scenario "Gateway rollout" (summary excludes `gateway-shutdown`) has no test for the summary | fix-now | fixed (e595f37) |
| C16+S31 | Spec+Standards | P1 | `crates/df-gateway/src/resolve.rs:41,127` | Test pins the helper, not `SystemResolver::new`; asserts hickory's own default | fix-now | fixed (e595f37) |
| S30 | Standards | P1 | `crates/df-gateway/src/control/relay/tests.rs:~300` | `viewer.next().await` without timeout | fix-now | fixed (e595f37) |
| S32 | Standards | P1 | `crates/df-web/src/egress/log.rs:~245` | `message(.., replace: bool)` flag changes three behaviours | fix-now | fixed (e595f37) |
| S33 | Standards | P3 | relay.rs:118, egress/tests.rs, relay/tests.rs | `backlog_end` marker built in four places | — | cosmetic |
| S34 | Standards | P3 | `crates/df-web/src/egress/log.rs:~289-311` | Nested `stream::once(..).flatten()`, duplicated `Event::default().data` | — | cosmetic |
| S35 | Standards | P3 | relay.rs:97, proxy.rs:411,463 | Lines over 100 columns in doc comment and macros | — | cosmetic |

User decision: Fix blocking + P1. C15: close the row (`flow_id`, one row per request); spec/design updated in 12a7e34.

## Iteration 2, Round 2 (2026-10-24)

- Implementer: e595f37, tasks 17/18, checks fmt/clippy/machete/buf ok, tests df-proto 3, df-gateway 66, df-web 239, dark-factory 11, container tests ran
- Reviewed range: 12a7e34...e595f37

Ledger check: C15, C17, C16+S31, S30, S32 verified-fixed (C15 with gap C18).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C18 | Spec | Blocking | `crates/df-web/src/egress/log.rs:183` | Closed row shows no duration (scenario "Long-lived tunnel visible while open") | fix-now | fixed (4483789) |
| S37 | Standards | P1 | `crates/df-gateway/src/proxy.rs:125,244`, `crates/df-web/src/egress/log.rs:256,341` | Flow counter restarts at 1 after a container restart (pod name unchanged): duplicate row ids, stale open rows; viewer open set never pruned | fix-now | fixed (4483789) |
| C20 | Spec | P1 | `crates/df-web/src/egress/log.rs` `live_message` | Opened + closed of one flow in the same live batch rely on htmx applying partials in document order (unverified) | fix-now | fixed (4483789) |
| C19+S39 | Spec+Standards | P1 | `crates/df-web/src/egress/tests.rs:~633` | Denied-only filter no longer asserted for a `gateway-shutdown` close | fix-now | fixed (4483789) |
| S36 | Standards | P1 | `crates/df-web/src/egress/log.rs:174` | Row bypasses `components::table::tr`, duplicates its classes | fix-now | fixed (4483789) |
| S38 | Standards | P3 | `crates/df-gateway/src/control/relay/tests.rs:156` | `next_host` duplicates `next_event` timeout | — | cosmetic |

User decision: Fix blocking + P1

## Iteration 2, Round 3 (2026-10-24)

- Implementer: 4483789, tasks 17/18, checks fmt/clippy/machete ok, tests df-gateway 66, df-web 243, container tests ran
- Reviewed range: e595f37...4483789

Ledger check: C18, S37, C20, C19+S39, S36 verified-fixed (C18 test gap C21, S37 design gap C22).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S40 | Standards | Blocking | `crates/df-web/src/egress/log.rs:201` | `seconds * 1000` from gRPC overflows (debug panic, release wrap); negative renders `-5 ms` | fix-now | fixed (400e8f1) |
| C21 | Spec | P1 | `crates/df-web/src/egress/tests.rs:576` | Opened event in test has no duration, so "open rows show none" isn't really tested | fix-now | fixed (400e8f1) |
| C22 | Spec | P1 | `design.md` D6 | Open-row cap (10,000, cleared on overflow) not in design | fix-now | fixed (design, by orchestrator) |
| S41 | Standards | P1 | `crates/df-web/src/egress/log.rs:363-364` | `contains_key` then `[]` indexing twice | fix-now | fixed (400e8f1) |
| S42 | Standards | P1 | `crates/df-web/src/egress/log.rs:279-282` | `MAX_OPEN_ROWS` between `Viewer` and its `impl` | fix-now | fixed (400e8f1) |
| S43 | Standards | P3 | `crates/df-web/src/egress/log.rs:158` | Hard-coded `colspan="6"` | — | cosmetic |

User decision: Fix blocking + P1

## Iteration 2, Round 4 (2026-10-24)

- Implementer: 400e8f1, checks fmt/clippy ok, df-web egress tests 32 ok (full df-web suite rerun by orchestrator)
- Reviewed range: 5043fe1...400e8f1

Ledger check: S40, C21, S41, S42 verified-fixed; C22 verified (D6).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| C23 | Spec | P1 | `design.md` D6, `crates/df-web/src/egress/log.rs:345` | Open-row cap is only enforced on live `opened` events; the backlog of several pods can briefly exceed it | reword D6 | fixed (design, by orchestrator) |
| S44 | Standards | P3 | `crates/df-web/src/egress/tests.rs:768` | Duration test closure duplicated | — | cosmetic |

Final checks (orchestrator, 400e8f1): `cargo build --workspace` ok, `cargo machete` ok. Tests: df-proto 3, df-gateway 66, df-web 244, dark-factory 11 (2 ignored) pass. In the full-suite run, 4 df-gateway control tests and 1 df-web auth test failed at the shared PostgreSQL container start (`df-test-postgres/src/lib.rs:83`); all 5 pass when rerun alone. Cause: environment (shared disk filled during earlier runs; `target/` and `~/.cache/bazel` deleted on user request; Podman holds a stale container entry 5c338dcdaab7).

## Result, iteration 2

- Status: done. No blocking finding open. Fix rounds: 3.
- Tasks 17/18; open: 5.1 (manual dev-cluster check) and the manual `git clone` check of 7.2.
- Tech debt: TD-48 resolved and removed; TD-51 added (configurable IP family, address fallback).
