# Review log: native-harness-spike

- BASE: 6309c00e2c5c774bef2fde0dec97ffb61f9a934d
- Store: none
- Status: done
- Implementer agent: a2ac2c48b5402835c

## Round 1 (2026-10-27)

- Implementer: 1256ae1 (HEAD_1), tasks 11/11, checks fmt/clippy/machete/deny ok, df-harness 6 pass, factory-agent 101 pass; container tests: none needed (Podman used for measurements)
- Reviewed range: BASE...1256ae1

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | Blocking | `crates/df-harness/src/mcp.rs:49-50` | MCP handshake and tool listing have no timeout (AGENTS.md MUST) | fix | fixed (5c1c346) |
| S2 | Standards | Blocking | `agent.rs:49`, `mcp.rs:94` | `reqwest::Client::new()` without timeouts; `call_tool` unbounded | fix | fixed (5c1c346) |
| X1 (Spec P1) | Spec | P1 | branch scope | Code and docs mixed in WIP commits; proposal allows only docs on main | handle at finish (squash split) | open: user to split code from docs when squashing |
| X2 (Spec P4+S6) | Spec | P1 | `tests.rs` df-harness test, `smoke.rs:277` | Skipped tests are no evidence; record one real run of each in Findings | fix | fixed (5c1c346) |
| X3 (Spec P2) | Spec | P1 | `design.md:152-156` | No wall-clock column; tokens column is context occupancy; PR review n/a | fix | fixed (5c1c346) |
| X4 (Spec P3) | Spec | P1 | `design.md:149` | cgroup peaks are warm-cache figures; decision rests on tree RSS | fix | fixed (5c1c346) |
| X5 (Spec P5) | Spec | P1 | `full-harness-mode/{design,tasks}.md` | Custom tools estimate and tasks for tuning and bash hardening missing | fix | fixed (5c1c346) |
| X6 (Spec P6) | Spec | P1 | `product/context.md:52` | `df-native` name settles an open question | fix | fixed (5c1c346) |
| X7 (Spec P7) | Spec | P1 | `design.md:112`, `proposal.md` | Stale usage row and version 0.9.x | fix | fixed (5c1c346) |
| S3 | Standards | P2 | `main.rs` | `DF_HARNESS_*` variables have no reference entry | TD | tech-debt (TD-63) |
| S7 | Standards | P3 | tests | duplicated fake SSE endpoint | cosmetic | cosmetic |
| S4,S5,S8-S13, Spec P8 | both | P3 | various | ordering, `let _`, naming, hardcoded host in run.sh, missing file refs | cosmetic | cosmetic |

User decision (round 1): Fix blocking + P1.

## Round 2 (2026-10-27)

- Implementer: 5c1c346 (HEAD_2), fixes S1, S2, X2-X7; checks fmt/clippy/machete/deny/openspec validate ok; df-harness 2 unit + 6 smoke pass; container tests: none needed
- Reviewed range: 1256ae1...5c1c346 plus ledger check

Round 2 findings: no blocking. S1, S2, X2-X7 verified fixed. New: R2-1..R2-3 (design.md nits, fixed 90f393b), S14 (tech-debt TD-64), S15, S16 (cosmetic). S6 rejected (skip-when-binaries-missing is an established pattern in factory-agent tests; evidence covered by X2).
User decision (round 2): fix P1, then finish. Tech debt written: TD-63 (S3), TD-64 (S14).
