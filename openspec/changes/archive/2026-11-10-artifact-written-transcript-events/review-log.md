# Review log: artifact-written-transcript-events

- BASE: 837038b154aa722d5ce42b1fef8464212ec4aa80
- Store: none
- Status: untested
- Implementer agent: aa211a67a059fbd7b

## Round 1 (2026-11-10)

- Implementer: 68b5bd1, tasks 11/13 (open: 6.1 bazel/container tests, 6.2 manual), checks fmt/clippy/openspec/docs-build ok, factory-agent + df-web transcript tests pass; machete not installed; bazel build fails in sandbox (`hooks_empty`, cache ALPN), container tests skipped: Docker socket answers but `docker run` fails (overlay mount "invalid argument") — df-db artifacts/transcript tests, df-agent-channel dispatcher/agent_port/summary tests, df-web artifacts + SSE tests, `just e2e-local`.
- Notes: ADR is 0057 (0056 taken); tasks.md/roadmap follow-up still say 0056.
- Reviewed range: 837038b...68b5bd1

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-web/src/transcript/tests.rs` | No test folds a tool call followed by `ArtifactWritten` and checks both rows, tool row first (task-ui "Agent writes findings", "Harness wraps the tool") | fix-now | fixed (308a679) |
| P2 | Spec | Blocking | `crates/df-web/src/sse/tests.rs:207` | SSE test does not check order against agent events (factory-activity "Board member watching") | fix-now | fixed (308a679) |
| P4 | Spec | P1 | `openspec/changes/.../tasks.md:6,36` | tasks.md says ADR-0056; ADR is 0057 | fix-now | fixed (308a679) |
| S1 | Standards | P1 | `crates/df-db/src/transcript.rs:20` | `SCHEMA_VERSION` doc names only `forge_event` | fix-now | fixed (308a679) |
| S2 | Standards | P1 | `crates/df-db/src/transcript.rs:29-40` | `factory_event` duplicates `forge_event`; share a private builder | fix-now | fixed (308a679) |
| S6 | Standards | P1 | `crates/df-db/src/transcript.rs:584` | over-long doc line | fix-now | fixed (308a679) |
| S7 | Standards | P1 | `crates/df-agent-channel/src/transcript.rs:227` | doc grammar after plural subject | fix-now | fixed (308a679) |
| S8 | Standards | P1 | `crates/df-agent-channel/src/tools/artifacts/tests.rs:202` | `source_key` selected but not asserted | fix-now | fixed (308a679) |
| S9 | Standards | P1 | `crates/df-web/src/artifacts/tests.rs:304,345` | leftover `tool_row` test duplicates the new harness test | fix-now | fixed (308a679) |
| P3 | Spec | P2 | browser / `scripts/artifactSplit.ts` | live tab appearance only tested piecewise; no browser test (manual 6.2). Matters when the script or OOB tab bar changes | tech-debt | tech-debt (TD-122) |
| S3 | Standards | P3 | 4 test files | hand-built `ArtifactWritten` fixtures | cosmetic | cosmetic |
| S4 | Standards | P3 | `crates/df-web/src/transcript/mod.rs:882`, `artifacts/view.rs:805` | data clump name/kind/mime | fix-now | fixed (308a679) |
| S5 | Standards | P3 | `transcript.proto:36` | `kind` string vs enum; consistent with `ArtifactMeta.kind` | fix-now | fixed (308a679) |
| S10 | Standards | P3 | process | python heredoc edits; diffs fine | cosmetic | cosmetic |

Security: no findings.

User decision: Fix blocking + P1, plus S4 and S5 (moved from P3 to fix-now). P3 to tech debt.

## Round 2 (2026-11-10)

- Implementer: 308a679, fixes P1 P2 P4 S1 S2 S4 S5 S6 S7 S8 S9; checks fmt/clippy/openspec/docs-build/buf ok, df-web transcript 44 + factory-agent 169 pass; container tests skipped (docker run fails): df-db artifacts/transcript, df-agent-channel dispatcher/agent_port/summary, df-web artifacts routes + SSE. Process: sed -i / python heredoc used again for test edits.
- Reviewed range: 68b5bd1...308a679

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11 | Standards | P1 | `crates/df-db/src/artifacts.rs:113` | kind mapped by string compare of `meta.kind`; use exhaustive `match` on `new.sniffed.kind` | fix-now | fixed (505201a) |
| S12 | Standards | P1 | `crates/df-web/src/transcript/tests.rs:735` | test name omits the folded-in failed-write assertion; split | fix-now | fixed (505201a) |
| S13 | Standards | P1 | `crates/df-web/src/sse/tests.rs:208` | test name omits id/order assertions | fix-now | fixed (505201a) |
| P5 | Spec | P1 | `crates/df-web/src/sse/tests.rs:215` | frame before the event is read but its id not asserted (`before < id`) | fix-now | fixed (505201a) |

Security: no findings. Round 1 rows P1 P2 P4 S1 S2 S4 S5 S6 S7 S8 S9 verified fixed.

User decision (round 2): Fix P1 items S11 S12 S13 P5.

## Round 3 (2026-11-10)

- Implementer: 505201a, fixes S11 S12 S13 P5; fmt/clippy ok, df-web transcript 43 pass (2 ignored: compiled assets), df-db/df-web test targets compile; container tests skipped (same list as round 2). Edits via Edit tool.
- Reviewed range: 308a679...505201a

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P6 | Spec | P1 | `crates/df-db/src/artifacts.rs:113`, `artifacts/tests.rs:~143` | no test asserts an image write emits `ARTIFACT_KIND_IMAGE` | fix-now | fixed (73d9ccb, diff checked by orchestrator) |

Security, Standards: no findings. S11 S12 S13 P5 verified fixed.

User decision (round 3): Fix P6, then finish.

## Finish (2026-11-10)

- Round 3 fix: 73d9ccb (P6). Fix rounds run: 3.
- Status `untested`: containers cannot start in this sandbox (overlay mount error); the user verifies container-backed tests in CI. Not run (compiled only): df-db `artifacts::tests`, `transcript::tests`; df-agent-channel `tools::artifacts` dispatcher test, `tests/agent_port.rs` factory-activity test, `summary` tests; df-web artifacts route tests and `sse::tests`; `just e2e-local`.
- `cargo machete`: run by the user, no unused dependencies.
- Not run: Bazel (no Cargo.toml/Cargo.lock change; sandbox build fails in `//crates/factory-agent:hooks_empty`).
- Tasks 11/13: 6.1 (Bazel/container tests) and 6.2 (manual OpenCode/Claude Code check) open.
- `just docs-build` passes. Docs changed: `docs/content/using/artifacts.md`, `docs/content/using/tasks.md`; ADR-0057 added.
- Tech debt: TD-122 (P3). Roadmap: backlog row for further factory activity and run-less rows.
