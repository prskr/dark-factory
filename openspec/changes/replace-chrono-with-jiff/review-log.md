# Review log

## Round 1 — plan review (DF-24, Grace Hopper), verdict FAIL

| # | Severity | Finding | Resolution |
|---|---|---|---|
| 1 | major | `sqlx.toml` needs feature `sqlx-toml`; override key is case-sensitive (`TIMESTAMPTZ`) | D2, tasks 2.1, 2.2, 4.1; spot-checked in `sqlx-core` 0.9.0 `config/macros.rs` |
| 2 | major | Spike in `df-db` breaks the crate (override is crate-wide) | Spike in a throwaway crate (2.2); override and full `df-db` swap in one task (4.1) |
| 3 | major | Pin tests cannot run on `chrono` after `df-core` moved | New group 1 before any migration; D6 reworded |
| 4 | major | `to_rfc3339()` prints `+00:00`, `jiff` prints `Z` | Accepted: D4 lists the changed texts (`datetime` attribute, artifact tools line); proposal states it |
| 5 | major | Routines-first path breaks the build | Decided: `iter3-routines` lands after this change; its artifacts updated |
| 6 | minor | Test code outside `df-db` needs `jiff-sqlx` | D2 and tasks 5.1–5.4 |
| 7 | minor | Fallback incomplete (binds, `.sqlx/`) | D2 fallback, task 4.1 |
| 8 | minor | Missed uses (`factory-agent/src/transcript.rs`, `tools/artifacts.rs`, views, `tools/xtask`) | Context table, tasks 1.1, 5.2–5.5, 6.1 |
| 9 | minor | `to_proto` must normalize negative nanos | D3, task 3.1 |
| 10 | minor | Out-of-scope item without target or "rejected" | "Rejected for now" |
| 11 | nit | `Timestamp - Timestamp` is `SignedDuration` in `jiff` 1.0 | D1 table, extra mappings |
| 12 | nit | `query_as!` already calls `.into()` | D2, task 4.1 |
| 13 | nit | `jiff` 1.0 not compiled today; two `jiff` majors after | Context, proposal Impact, Risks, Out of Scope |
| 14 | nit | Wrong commit hash, "four dependencies" wording | Proposal Why |
| 15 | nit | Contributor docs (skill `rust`) | Task 6.4 |
| 16 | nit | `iter3-routines` names `df-agent-channel` for the scheduler | Fixed to `df-orchestrator` |
