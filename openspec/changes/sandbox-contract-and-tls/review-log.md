# Review log: sandbox-contract-and-tls

- BASE: 854d41d2028ed4c5d926b53b7bb9011414cd8f85
- Store: none
- Status: done
- Implementer agent: a04b59735b0b7fbd6

## Round 1 (2026-11-18)

- Implementer: 396525d4, tasks 13/13, checks fmt/clippy/machete/check-file-length/check-crates/bazel build/docs-build ok; nextest 2726/2740 + bazel test 54/55 — only failures `factory-agent` sandbox tests (need Landlock ABI 6 kernel, environmental), container tests ran (DOCKER_HOST=/var/run/docker.sock)
- Reviewed range: 854d41d2...396525d4

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S1, S2 | Standards | P1 | `docs/architecture.md:69`, `crates/df-web/Cargo.toml:44`, `crates/df-agent-channel/BUILD.bazel:99` | df-web / df-agent-channel dev-depend on `df-telemetry` test-util, which their rows don't list (rule 1) | fix-now: widen rule 1 for `df-telemetry` test-util | fixed (e9f30d36) |
| P2 | Spec | P1 | `docs/tech_debt.md:207` | TD-10 location still names `accept_loop` in df-agent-channel server.rs | fix-now | fixed (e9f30d36) |
| S3 | Standards | P1 | `crates/df-tls/src/testing.rs:29` | `mod anyhow_free` between `TestPki` and its `impl` | fix-now | fixed (e9f30d36) |
| S4 | Standards | P3→fix | `crates/df-tls/src/testing.rs:30` | `anyhow_free` is a mysterious name | fix-now (user) | fixed (e9f30d36) |
| S6 | Standards | P1 | `crates/dark-factory/src/main.rs:325` | `tls_paths(tls_config)` computed twice | fix-now | fixed (e9f30d36) |
| P1 | Spec | P3 | `crates/df-tls/src/reload.rs:104-113` | Reload log wording changed ("agent port TLS" → "mTLS"); more accurate, no docs quote it | keep | cosmetic |
| S5 | Standards | Rejected | `crates/dark-factory/src/main.rs:744` | `TlsPaths` mirrors `AgentTlsConfig` | — | rejected (required by rule 1 boundary) |
| P3 | Spec | Rejected | `crates/df-tls/src/testing.rs:93` | `write_agent_tls` → `write_server_tls` rename | — | rejected (justified: df-tls has no agent concept, all callers changed anyway) |

Security: 0 findings.

Environment: 14 `factory-agent` sandbox tests (`tests/sandbox.rs`, `//crates/factory-agent:sandbox_test`) fail locally, need a Landlock ABI 6 kernel. User: fix the kernel after this change; CI runs them.

User decision: Fix blocking + P1 + S4.

## Round 2 (2026-11-18)

- Implementer: e9f30d36 (fix round 1), checks fmt/clippy/nextest df-tls+dark-factory (container tests ran)/docs-build ok. S6 implemented as helper `load_tls_pair` (a local pushed `serve` over `too_many_lines`).
- Reviewed range: 396525d4...e9f30d36

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P4 | Spec | P1 | `docs/tech_debt.md:275-279` | TD-26 still lists `anyhow_free` (removed by this change) | fix-now | fixed (7023abb9) |

Security: 0 findings. Standards: 0 new; S1+S2, S3+S4, S6 confirmed fixed. Spec: P2 confirmed fixed.

User decision: Fix blocking + P1.

Fix round 2 (7023abb9): doc-only TD-26 edit, verified by the orchestrator against the diff; no further review round.

## Finish (2026-11-18)

- Container-backed tests ran (cargo and Bazel, DOCKER_HOST=/var/run/docker.sock). The `factory-agent` Landlock sandbox tests fail locally on the kernel (Landlock ABI 6 missing); user accepted, CI runs them.
- Bazel build `--lockfile_mode=error //crates/... //plugins/...` ok at 396525d4; later commits touch no `Cargo.toml`, `Cargo.lock` or `BUILD.bazel`.
- `just docs-build` ok (7023abb9). No published docs page changed (none names a moved item).
- Tech debt: no P2 entries; TD-109 resolved and removed by this change.
- Deferred: roadmap rows for architecture #4 and #5 are added at archive (tasks.md, "Workflow follow-up").
