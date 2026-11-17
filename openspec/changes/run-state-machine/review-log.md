# Review log: run-state-machine

- BASE: 45129a209f5fe4ff09d73b0a34062baf9f6d41e4
- Store: none
- Status: done
- Implementer agent: aecc2bb8b919a8237 (died during fix round 3, sandbox crash); replaced by ad0a5fd1e558e8bb3, which picks up the uncommitted partial edits

## Round 1 (2026-11-17)

- Implementer: 699e6a4b, b872131a, tasks 15/15, checks fmt/clippy/machete/tests/check-*/SQLX_OFFLINE build/docs-build/Bazel build+test/e2e-local ok, container tests ran (docker.sock)
- Reviewed range: BASE...b872131a
- Implementer deviations: RunSnapshot holds timestamps + run_id; Overdue carries RunsConfig; RunEnv param; session flag `command_sent: Option<RunCommand>`; test `a_budget_cancel_is_never_followed_by_a_release` rewritten; turn_outcome order Cancel > end-run > Release (matches old code, not design text); RunControl in run_control.rs; mounted df-agent-channel docs updated.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP2 | Spec | Blocking | `crates/factory-agent/src/runner.rs:1364` | Idle Release arm now uses `turn_outcome()`; with `await_input` + late end-run/cancel flag the outcome becomes EndTurn instead of Blocked (behavior drift, verified against base:1370) | | open |
| SP3 | Spec | Blocking | `crates/df-agent-channel/src/session.rs:790` | No test that PREPARING/FINALIZING phases leave the run unchanged | | open |
| S1 | Standards | P1 | `crates/df-db/src/task_events.rs:457` | Unchecked `query_scalar` on `runs` in task_events (pre-existing as UPDATE, rewritten) → checked fn in `runs.rs` | | open |
| S2 | Standards | P1 | `crates/df-db/src/tasks.rs:1195` | Same, `request_stop_tx` active-run lookup → checked `runs::active_run_of_task` | | open |
| S3 | Standards | P1 | `crates/df-db/src/run_events.rs:283` | `UPDATE tasks` in run applier → move to `tasks.rs` | | open |
| S4 | Standards | P1 | `crates/df-db/src/runs.rs:572` | `RunEnv` built by hand 3× → one constructor | | open |
| S5 | Standards | P1 | `crates/df-db/src/runs.rs:474` | `end_overdue`/`end_run_tx` take RunEnv parts separately → pass `RunEnv` | | open |
| S6 | Standards | P1 | `crates/df-core/src/run_transition.rs:335` | `after` public only for a channel test duplicating a df-core test | | open |
| S10 | Standards | P1 | `crates/df-db/src/runs.rs:10` | Compat re-export of `RunEnd`/`STOP_TIMEOUT_SECONDS` → import from df-core | | open |
| S12 | Standards | P1 | `crates/df-db/src/run_events/tests.rs:89` | Lock-waiter loop without timeout; share helper with session test | | open |
| SP1 | Spec | P1 | `run_transition.rs:446`, `session.rs:352` | PR merge now sends Cancel("run stopped…") not ("task marked done"); idle/budget-finalizing run no longer gets extra Cancel on merge. Follows design table; record as intended in design.md | | open |
| SP5 | Spec | P1 | proposal.md Impact, tasks.md header | Say the published `configuration.md` changes | | open |
| SP6 | Spec | P1 | `crates/df-agent-channel/docs/configuration.md` | `stop` row omits PR merge | | open |
| SP7 | Spec | P1 | design.md §5 | Precedence text wrong (code: cancel > end-run > release) | | open |
| SP4 | Spec | P3 | `run_transition.rs:389` | Reconnect no longer publishes RunChanged; note in design with SP1 | | cosmetic |
| S7 | Standards | P3 | `run_transition.rs:167` | `Overdue` carries whole `RunsConfig` | | cosmetic |
| S8 | Standards | P3 | `run_transition.rs:280-300` | Repeated state-set matches | | cosmetic |
| S9 | Standards | P3 | `run_events.rs:61` | `Facts` name | | cosmetic |
| S11 | Standards | P3 | `runs/tests.rs:895` | test helper takes bool | | cosmetic |

Security: 0 findings.

User decision: fix Blocking + P1 + S8 + S11 (S8, S11 moved P3 → P1). SP4 questioned by user: answered — design.md decision 1 and task 2.2 require no publish on an unchanged row; all RunChanged consumers (session reconcile, scheduler, gateway policy refresh, web SSE) only re-read unchanged state, and the session reconciles at stream start (session.rs:199). Kept P3, note added to design.md with SP1.

User addendum (fix round 1): record in design.md/proposal.md: (1) RunSnapshot.stop_requested_at is Option<DateTime> for the stop timeout; (2) session keeps the last command sent per stream, with a test that a board stop after Release still sends Cancel; (3) FinalizeReason::command() alone picks Release/Cancel, reconcile_finalizing skips only when the same command was already sent on the stream.

## Round 2 (2026-11-17)

- Implementer: f94c6f85, all round-1 items + addendum fixed; fmt/clippy/machete/check-*/SQLX_OFFLINE build/docs-build/tests ok (df-core, df-db, df-agent-channel, factory-agent, df-test-postgres); container tests ran; Bazel/e2e not rerun (no Cargo.toml/BUILD change)
- Reviewed range: b872131a...f94c6f85
- Addendum 2 conflict: board stop (`StopRun(ActiveOnly)`) leaves a finalizing run alone, in base too (`NOT_COMMANDABLE` includes `finalizing`, base tasks.rs:1200). Implementer pinned today's behavior with `a_board_stop_of_a_released_run_sends_nothing_after_the_release`, not "still sends Cancel" as the user wrote. User decision: keep today's behavior (no Cancel), keep the test.

Round 1 items: S1, S2, S3, S4, S5, S6, S8, S10, S11, S12, SP1–SP7: fixed (f94c6f85), confirmed by round-2 reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP8 | Spec | Blocking | `crates/df-agent-channel/src/session.rs:269-279,352-372` | Base budget timer sent Cancel unconditionally, also to a run already finalizing for idle (base :271-279), and base reconcile skipped Release after the budget deadline (base :374-383, verified). Now `Finalize(Budget)` leaves a finalizing run unchanged: an idle-released run gets no budget Cancel, and a reconnect after the deadline sends Release instead of Cancel. Unlisted behavior change, untested | | open |
| SP9 | Spec | P1 | `proposal.md:9,24`, design.md decision 1 | `RunLimits` → `RunsConfig`; "no change in observable behavior" contradicts Intended differences; `RunEnv::new`/`events_only` missing | | open |
| SP10 | Spec | P1 | `crates/df-agent-channel/docs/configuration.md:111-122` | Diagram lacks budget edges from preparing/awaiting_input; stop edges omit PR merge | | open |
| SP11 | Spec | P1 | `configuration.md:187-188` | Doc says stop rewrites only `idle` reason; code rewrites `budget` too and a second Cancel follows | | open |
| S13 | Standards | P1 | `crates/df-db/src/run_events.rs:58`, `run_events/tests.rs` | `RunEnv::none` and tests still build literals | | open |
| S14 | Standards | P1 | `crates/df-core/src/run_transition.rs:278-312` | `match` with only `_ if` guards → if/else chain | | open |
| S15 | Standards | P1 | `crates/df-db/src/run_events.rs:39` | `RunEnv::new` lacks doc comment | | open |

Security: 0 findings.

User decision: fix Blocking + P1 (SP8 restores base behavior: budget overrides idle reason; stop/done keep reason, dropped second budget Cancel listed as intended difference).

## Round 3 (2026-11-17)

- Implementer: d38307ce, all round-2 items fixed; fmt/clippy/machete/check-*/SQLX_OFFLINE build/docs-build/tests ok; container tests ran; Bazel/e2e not rerun
- Reviewed range: f94c6f85...d38307ce

Round 2 items: SP8, SP9, SP10, SP11, S13, S14, S15: fixed (d38307ce), confirmed by round-3 reviewers. Spec reviewer found no new drift from the stream-start `Finalize(Budget)` (checked case by case against base).

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| SP12 | Spec | Blocking | `crates/df-agent-channel/src/session/command_tests.rs` | Intended difference (c) has no session test: a stop/done-finalizing run past its budget sends exactly one Cancel | | open |
| S16 | Standards | Blocking | `crates/df-core/src/run_transition/tests.rs:251` | Test name `a_stop_a_done_task_or_an_idle_finalize_leaves_a_finalizing_run_alone` doesn't match what it checks | | open |
| S17 | Standards | Blocking | `crates/df-core/src/run_transition/tests.rs:238` | Test name omits the `budget` case it asserts | | open |
| S20+SP13 | Standards/Spec | P1 | `crates/df-core/docs/run-transitions.md:27` | Diagram lacks the `finalizing --> finalizing` budget-takeover edge | | open |
| S18 | Standards | P1 | `crates/df-core/src/run_transition.rs:396-404` | `allowed` bound twice; `released` misleading | | open |
| S19 | Standards | P1 | `crates/df-agent-channel/src/session.rs:297,303` | Budget deadline computed twice → `budget_deadline()` | | open |
| SP14 | Spec | P1 | design.md decision 1 | `start_finalizing` bool means "changed the run", not "changed the state" | | open |

Security: 0 findings.

User decision: fix Blocking + P1 (fix round 3).

## Round 4 (2026-11-17)

- Implementer: d378642c (replacement implementer; kept the dead implementer's partial edits unchanged), SP12, S16, S17, S18, S19, S20/SP13, SP14 fixed; fmt/clippy/machete/check-*/SQLX_OFFLINE build/docs-build ok; df-core, df-db, df-agent-channel tests ok with docker.sock; factory-agent unit + cli ok, `tests/sandbox.rs` 14/15 fail: kernel 6.12.115 lacks Landlock ABI 6 (sandbox restarted on a different kernel; same failures with the changes stashed; this round doesn't touch factory-agent)
- Reviewed range: d38307ce...d378642c

Round 3 items: SP12, S16, S17, S18, S19, S20+SP13, SP14: fixed (d378642c), confirmed by round-4 reviewers.

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S21 | Standards | P3 | `crates/df-agent-channel/src/session/command_tests.rs:144-160` | New test repeats the setup of the neighbouring budget test | | cosmetic |
| S22 | Standards | P3 | `crates/df-agent-channel/src/session/command_tests.rs:176-178` | `let … else { panic!() }` close to the forbidden guard pattern (arguable) | | cosmetic |

Spec: 0 findings. Security: 0 findings.

## Finish (2026-11-17)

- Container tests: ran in every round with docker.sock; orchestrator re-ran df-core, df-db, df-agent-channel at d378642c: 1008/1008 pass. `factory-agent/tests/sandbox.rs` 14/15 fail on the restarted sandbox's kernel 6.12.115 (no Landlock ABI 6); passed in rounds 1–2 on kernel 6.12.117; factory-agent unchanged since f94c6f85.
- Clippy (-D warnings) ok, `just docs-build` ok.
- Bazel build: not required (no Cargo.toml/Cargo.lock change); `crates/df-core/BUILD.bazel` unchanged since round 1, Bazel tests passed then.
- Published docs changed: `crates/df-agent-channel/docs/configuration.md` (Run transitions section, state diagram, finalize_reason table).
- Tech debt: none (no P2 findings).
- Roadmap backlog rows added: remove RUN_PHASE_PREPARING/FINALIZING from agent.proto; move old lock-wait test loops onto `wait_for_lock_wait` (user request).
- Totals: fixed 31 IDs (S1–S6, S8, S10–S20, SP1–SP3, SP5–SP14; S20 and SP13 one defect), cosmetic 5 (S7, S9, SP4, S21, S22), rejected 0, security 0.
