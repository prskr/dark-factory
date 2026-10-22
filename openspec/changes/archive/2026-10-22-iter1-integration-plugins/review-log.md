# Review log: iter1-integration-plugins

- BASE: fb03b1c
- Status: done
- Tech debt: TD-38 (S7), TD-39 (R2-P1), TD-40 (R2-S2)
- Implementer agent: a7d463b9ff3752bf2

Resumed from an earlier session: tasks 1.1–5.2 were implemented and reviewed once outside this ledger (uncommitted). Task group 6 (delivery readiness, design D6) is new and open.

## Round 1 (2026-10-22)

- Implementer: uncommitted work in progress, tasks 12/15, checks from the earlier session green (clippy, cargo tests incl. Podman, Bazel tests, fmt, machete)
- Reviewed range: BASE...working tree (uncommitted + untracked; excludes `.claude/`, `openspec/changes/iter2-plugin-distribution/`, `product/`, `AGENTS.md`, TD-37)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/df-plugins/src/host.rs:29`, `contract/wit/integrations/integrations.wit:179` | Import allow-list matches namespace prefixes (`wasi:cli/`, `wasi:http/`, `wasi:random/`), wider than world `plugin-base`; the forgejo component imports `wasi:cli/{environment,stdin,stdout,exit,terminal-*}` (std on wasip2), so task 2.1 is not met | fix-now | fixed (61b6505) |
| P2 | Spec | Blocking | `crates/df-plugins/tests/sandbox.rs:163` | "no connection is opened" is asserted against a server the request never targeted | fix-now | fixed (61b6505) |
| S1 | Standards | P1 | `crates/dark-factory/src/forge.rs:89` | Runtime `sqlx::query_as(...)` in the binary; storage belongs in `df-db` with compile-time checked queries | fix-now | fixed (61b6505) |
| P3 | Spec | P1 | `crates/dark-factory/src/main.rs:87` | `error = %err` logs only the outermost context; the cause (ENOENT, forbidden import) is lost | fix-now | fixed (61b6505) |
| P4 | Spec | P1 | `crates/dark-factory/src/main.rs:386` | Plugin loads after TLS, DB pool, notifier and backend start; a missing component is reported only once the DB is reachable | fix-now | fixed (61b6505) |
| S6 | Standards | P1 | `crates/df-plugins/tests/forgejo.rs:212` | Real 1 s wait asserted with `Instant` (timing-based test) | fix-now | fixed (61b6505) |
| S9 | Standards | P1 | `crates/df-plugins/src/lib.rs:15` | `InstanceSpec`, `HttpPolicy` exported without external user | fix-now | fixed (61b6505) |
| S3 | Standards | P1 | `crates/df-plugins/src/forge.rs:97` | Private `parse_repo_url` between the two impls of `PluginForgeClient` | fix-now | fixed (61b6505) |
| S4 | Standards | P1 | `crates/df-plugins/src/state.rs:141,149` | Constants mid-file | fix-now | fixed (61b6505) |
| S5 | Standards | P1 | `plugins/forgejo/src/lib.rs:28` | Private helpers between `ForgejoPlugin` and its impls | fix-now | fixed (61b6505) |
| S10 | Standards | P1 | `crates/df-plugins/src/convert.rs:3` | `use df_core as core;` shadows the `core` crate | fix-now | fixed (61b6505) |
| P5 | Spec | P1 | `plugins/forgejo/src/lib.rs:30` | Missing `base-url` → `config` error is untested | fix-now | fixed (61b6505) |
| P6 | Spec | P1 | `plugins/forgejo/src/wasi_transport.rs:17` | Sub-path prefix in `base-url` is unreachable (host passes scheme + authority only) | fix-now | fixed (61b6505) |
| P7 | Spec | P1 | `design.md` D1, `crates/df-plugins/README.md:39` | "`Engine` shared" contradicts D6 and the code (one `Engine` per plugin) | fix-now | fixed (61b6505) |
| S7 | Standards | P2 | `crates/df-plugins/src/forge.rs:118-230` | Five `ForgeClient` methods repeat resolve → retry → run → map error | tech-debt | tech-debt (TD-38) |
| S2 | Standards | P3 | `crates/df-plugins/src/policy.rs:16` | `for_base_url` returns `Result<_, String>`; moot once `HttpPolicy` is `pub(crate)` (S9) | - | cosmetic |
| S8 | Standards | P3 | `crates/df-plugins/src/host.rs:90` | `(name, call_timeout, memory_limit)` travel together | - | cosmetic |

User decision: Fix blocking + P1 (S7 → tech debt; S2, S8 cosmetic). Fix round 1 also implements task group 6 (6.1–6.3).

## Round 2 (2026-10-22)

- Implementer: 61b6505, tasks 15/15, checks fmt/clippy/machete/deny ok, cargo tests ok, Bazel tests 8/8 ok, lefthook ok, container tests ran (Podman)
- Reviewed range: fb03b1c...61b6505 (round 1 and the fix round share one commit; focus on the fix items and task group 6)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| R2-P2 | Spec | P1 | `crates/df-plugins/src/host.rs:118`, `crates/df-plugins/docs/configuration.md` | `compile_ms` also covers `Engine::new`, linker, `instantiate_pre`, ticker spawn; doc says "validation and compilation" | fix-now | fixed (a8812b4) |
| R2-P1 | Spec | P2 | `deploy/test_images.sh:82` | `wasm-tools validate` of the shipped component was a manual check; the image test checks size and magic only (`wasm-tools` not in Bazel multitool) | tech-debt: add `wasm-tools` to multitool and validate in the image test; matters when the component build (toolchain, opt transition) changes | tech-debt (TD-39) |
| R2-S2 | Standards | P2 | `crates/df-plugins/src/host.rs:30`, `integrations.wit:182` | `ALLOWED_IMPORTS` repeats world `plugin-base` by hand | tech-debt: test that every `plugin-base` import is allowed; matters when `plugin-base` gains an import | tech-debt (TD-40) |
| R2-S1 | Standards | P3 | `crates/dark-factory/src/forge.rs:56`, `crates/df-db/src/projects.rs:9` | `df_db::ForgeTarget` copied field by field into `df_plugins::ForgeTarget` (keeps df-plugins independent of df-db) | - | cosmetic |
| R2-S3 | Standards | P3 | `crates/dark-factory/src/main.rs:369` | `"forgejo"` and `/plugins/forgejo.wasm` are literals | - | cosmetic |

User decision: Fix blocking + P1 (R2-P2 fix-now; R2-P1, R2-S2 → tech debt; R2-S1, R2-S3 cosmetic).

## Round 3 (2026-10-22)

- Implementer: a8812b4 (fix round 2), doc-only change (R2-P2); fmt and clippy ok
- Reviewed by the orchestrator (two-line doc wording, no code change); no reviewer round spawned
- No open blocking findings. Finished.

## Round 4: external review (2026-10-22)

Comments from an external review, all applied (fix round 3, absorbed into the change commit):

| ID | Where | Finding | Status |
|----|-------|---------|--------|
| E1 | `.claude/skills/implement-change/SKILL.md` | OpenSpec store not resolved in preflight nor passed on | fixed |
| E2 | `.claude/skills/implement-change/SKILL.md` | "Stop here" with open blocking findings could record `done` | fixed (`stopped` status) |
| E3 | `crates/dark-factory/src/forge.rs` | `memory_mib * MIB` could overflow | fixed (checked, with test) |
| E4 | `crates/df-plugins/src/host.rs` | Epoch deadline `+ 1` could overflow | fixed (`saturating_add`, with test) |
| E5 | `design/adr/0018-…md` | Init-container fallback contradicts iteration 2 plan | fixed (image volume or custom control plane image) |
| E6 | this ledger | R2-P1/R2-S2 rows had 8 cells | fixed |
| E7 | `specs/plugin-host/spec.md`, `design.md` D6, `tasks.md` 6.1 | "compile duration" vs post-read load duration | fixed |
| E8 | `openspec/specs/run-network-policy/spec.md` | `hostAliases` for headless `allowDns` Services | fixed (outside this change) |
| E9 | `openspec/specs/run-network-policy/spec.md` | Egress for selector-less Services | fixed (outside this change) |
