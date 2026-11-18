# Review log: gateway-control-crate

- BASE: 2a5291ccad8d9073916caedbf2260eb161d3885a
- Store: none
- Status: done
- Implementer agent: aaa483a6683580dc2

## Round 1 (2026-11-18)

- Implementer: cbab6d7e, tasks 6/6, checks fmt/clippy/machete/check-crates/check-file-length/docs-build ok, nextest 448/448, control tests 54 before / 54 after, bazel build + test ok, container tests ran (docker.sock)
- Reviewed range: BASE...cbab6d7e

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1 | Standards | P1 | `docs/architecture.md:53`, `crates/df-gateway-control/Cargo.toml:32` | dev-dep `df-tls` (`test-util`) not allowed by rule 1 / not in the `df-gateway-control` row | fix-now | fixed (2533f2fe) |
| SP2 | Spec | P1 | `crates/df-gateway/src/convert.rs` | task 1.2: the three `pub` fns lack a doc comment naming the link as user (only module `//!` does) | fix-now | fixed (2533f2fe) |
| S2 | Standards | P1 | `AGENTS.md:55`, `docs/architecture.md:52` | new `df-gateway` feature `test-util` (`PlainConnector`) not documented in overview / row | fix-now | fixed (2533f2fe) |
| SP1 | Spec | P3 | `crates/df-gateway-control/BUILD.bazel:30-46` | extra Bazel target `df_gateway_control_test_util` not in design (commented in BUILD) | ledger only | cosmetic |

Security: 0 findings.

User decision: Fix blocking + P1

## Round 2 (2026-11-18)

- Implementer: 2533f2fe, fixed S1, SP2, S2; checks fmt, clippy -p df-gateway, check-crates, docs-build ok
- Reviewed range: cbab6d7e...2533f2fe (10 lines, docs and doc comments only; reviewed by the orchestrator, no reviewer agents)
- S1 fixed by widening rule 1 (`df-tls` with `test-util` allowed as dev-dependency in any crate, like `df-telemetry`); consistent with rule 4 (infrastructure crate) and with no other row listing dev-only deps.
- New findings: none.

## Finish

- Container tests: ran in round 1 (docker.sock), none skipped.
- Bazel: `build --lockfile_mode=error //crates/... //plugins/...` ok at cbab6d7e; round 2 touched no Cargo files.
- Docs: `just docs-build` ok; no published page changed (none affected). Contributor docs: `docs/architecture.md`, `AGENTS.md`, `crates/df-gateway/README.md`, `docs/tech_debt.md` (paths).
- Tech debt: none. Deferred work: none new (`orchestrator-crate` is its own change).
