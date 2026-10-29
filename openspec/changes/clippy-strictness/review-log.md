# Review log: clippy-strictness

- BASE: e3c682ae231cfc8d41962049a75a2929050c2837
- Store: none
- Status: done
- Implementer agent: a11f0f4d935a47a0c

## Round 1 (2026-10-29)

- Implementer: 988a34f (groups: 3facbbd, b99fdb4, 4019a4a, 6751278, d41103a, e2f7b3a, c9fb436, 988a34f), tasks 16/16, checks fmt/clippy/machete/xtask/bazel lint ok, unit tests ok, container tests skipped: Podman socket answered at preflight but refused connections during implementation (df-db, df-agent-channel, df-web testkit/auth/SSE, df-gateway control::tests, dark-factory serve, df-test-postgres)
- Reviewed range: BASE...988a34f

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | P1 | `crates/df-k8s/src/crd.rs:17` | kopium output hand-edited (allow removed, `pub(super) use`, raw string); regeneration reintroduces hits | keep kopium output pristine, expect lints at `mod crd` in lib.rs | fixed (a2b19d2) |
| S2+SP4 | Both | P1 | `factory-agent/src/mcp.rs:168`, `fake-acp-agent/src/mock_mcp.rs:42`, `df-gateway/src/proxy.rs:174`, `df-agent-channel/src/server.rs:413`, `df-agent-channel/src/testing.rs:177-234`, `df-core/src/testing.rs:108` | `.ok()` drops without the comment AGENTS.md requires; `waiting().await.ok()` hides JoinError in fns returning Result | propagate with `?` where fn returns Result, else one-line reason | fixed (a2b19d2) |
| S3 | Standards | P1 | `factory-agent/src/connection.rs:630` | worker JoinError dropped silently | `warn!` on error | fixed (a2b19d2) |
| S4 | Standards | P1 | `factory-agent/src/main.rs:143` | flush timeout dropped silently | `warn!` on Elapsed | fixed (a2b19d2) |
| S5 | Standards | P1 | `factory-agent/src/test_harness.rs:421-428` | setup errors dropped via `.ok()` | propagate or comment | fixed (a2b19d2) |
| S16+SP3 | Both | P1 | `fake-acp-agent/src/agent.rs:58` | HOME-less fallback moved sessions from temp dir to cwd (behavior change) | keep `temp_dir` with `#[expect(clippy::disallowed_methods, reason)]` | fixed (a2b19d2) |
| SP6 | Spec | P1 | `df-gateway/src/proxy.rs:497-516` | rewritten `resolve` has no test for `resolve-failed` | add test: empty/err resolver → 502 resolve-failed | fixed (a2b19d2) |
| S10 | Standards | P1 | `df-core/src/config.rs:703` and 9 more | `assert_eq!(x, [] as [T;0])` contradicts the `assert_is_empty` allow reason | revert to `assert!(x.is_empty())` | fixed (a2b19d2) |
| S7 | Standards | P1 | `df-db/src/notify.rs:248` | consts after impl | move to top | fixed (a2b19d2) |
| S8 | Standards | P1 | `factory-agent/src/runtime_socket.rs:106` | private fn between pub items | move below | fixed (a2b19d2) |
| S9 | Standards | P1 | `df-plugins/tests/common/mod.rs:1-6` | `#![expect]` before `//!` doc | doc first | fixed (a2b19d2) |
| S15 | Standards | P1 | `runner.rs:440,1110`, `session.rs:49` | `Duration::from_hours(8760)` x3 unnamed | named const | fixed (a2b19d2) |
| SP1 | Spec | P1 | `AGENTS.md:183-185` | test exemptions not named in "Tests" (D7) | reference them under Tests | fixed (a2b19d2) |
| SP2 | Spec | P1 | `clippy.toml:9-11` | nesting counts not split prod/test | record 4=51/55, 5=12/13, 6=5/1 in comment | fixed (a2b19d2) |
| SP7 | Spec | P2 | factory-agent outbox sites, `df-db/src/migrate.rs:59-68` | new `warn!` on dropped outbox/cleanup errors untested; matters when outbox failures need diagnosing | log-capture test with failing outbox | tech-debt (TD-76) |
| SP5 | Spec | P3 | `df-db/src/egress.rs:60` etc. | `|_elapsed|` renames silence `map_err_ignore`; sources (Elapsed, out-of-range) carry no info | accept convention | cosmetic |
| S14 | Standards | P3 | `df-k8s/src/provider.rs:225` | `|_error|` vs `|_out_of_range|` | rename | cosmetic |
| S6 | Standards | P3 | `dark-factory/src/sandbox.rs:290,300` | redundant trailing comment | — | cosmetic |
| S11 | Standards | P3 | `factory-agent/src/connection.rs:580` | 120+ char line in select! | — | cosmetic |
| S12 | Standards | P3 | `factory-agent/src/connection.rs:650` | 6-param `send_outbound`, duplicated comment | — | cosmetic |
| S13 | Standards | P3 | `df-gateway/src/proxy.rs:826,831` | two near-identical error helpers | — | cosmetic |
| S17 | Standards | P3 | `tasks.md:16` | task text says `#[expect]`, code uses `.ok()`+comment | — | cosmetic |
| SP8 | Spec | P3 | `dark-factory/src/demo_flows.rs:117` | fn-level `expect(indexing_slicing)`; safe by construction | accept | cosmetic |

User decision: Fix blocking + P1 (no blocking open; SP7 → tech debt; P3 cosmetic)

## Round 2 (2026-10-29)

- Implementer: a2b19d2, tasks 16/16, checks fmt/clippy/machete/xtask/bazel lint ok, unit tests ok, container tests skipped (no socket)
- Reviewed range: 988a34f...a2b19d2

Round 1 rows S1–SP2 verified fixed by both reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S19+SP9 | Both | Blocking | `factory-agent/src/test_harness.rs:424-434` | regression from S5 fix: handler error `?` skips `send_close_notify` (GnuTLS clients reject EOF without it); client-gone `complete_io` now warns | send close_notify regardless of handler result, then return handler error; `complete_io` best effort with comment | fixed (76aa8e7) |
| SP10 | Spec | P1 | `fake-acp-agent/src/agent.rs:56-59` | expect reason doesn't say why `temp_dir` is fine | reword: stable fallback dir, tempfile would be deleted | fixed (76aa8e7) |
| S18 | Standards | P3 | `factory-agent/src/test_harness.rs:282,285,321,384` | `.ok()` drops without comment (code untouched by round 2, capped at P2) | add reason comments | cosmetic |
| S20 | Standards | P3 | `runner.rs:49` / `session.rs:49` | same "never fires" duration named `NO_DEADLINE` and `MAX_TIMER` | — | cosmetic |

User decision: Fix blocking + P1 (S19+SP9, SP10); S18 → tech debt; S20 cosmetic

## Round 3 (2026-10-29)

- Implementer: 76aa8e7, checks clippy/fmt/xtask ok, factory-agent + fake-acp-agent tests ok, container tests skipped (no socket)
- Reviewed range: a2b19d2...76aa8e7

Round 3 findings: none (both axes). S19+SP9 and SP10 verified fixed.

## Finish (2026-10-29)

- Bazel `--lockfile_mode=error --config=lint //crates/... //plugins/...`: ok
- Tech debt: TD-76 (SP7). S18 moved to cosmetic by user decision.
- Container-backed tests not run in any round (Podman socket refused connections): df-db, df-agent-channel, df-web testkit/auth/SSE, df-gateway `control::tests`, dark-factory `serve`, df-test-postgres. CI must cover them.
- Status: done (no blocking finding open)
