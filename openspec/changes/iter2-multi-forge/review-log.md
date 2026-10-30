# Review log: iter2-multi-forge

- BASE: 4a43d97 (origin/main)
- Store: none
- Status: done
- Branch: `iter2/multi-forge-impl` (the remote `iter2/multi-forge` is the merged spec PR #42)
- Implementer agent: fresh agent per chunk (subagents end after about 10 minutes)
- DOCKER_HOST: unix:///var/run/docker.sock

## Implementation (2026-10-30)

| Chunk | Tasks | Commit | Checks |
|---|---|---|---|
| M1 `hosts`, `authScheme` in `PluginConfig` | 1.1 | 9eb9059 | df-core tests, clippy, hooks |
| M2 routing table | 1.2 | 34b0571 | df-core tests |
| M3 auth scheme in `HttpPolicy` | 2.1 (part) | 361914b | df-plugins tests |
| M4a `ForgeRouter`, one `call` helper (TD-38) | 3.1 (part), 2.2 | 43b89cb | df-plugins tests, `cargo check -p dark-factory` |
| M4b routing tests, plain-HTTP test knob | 2.1, 2.2, 3.1 | 8dd9f3d | `tests/routing.rs`, Bazel target |
| M5 unavailable plugins keep their hosts | 3.2 | 673c76f | `tests/routing.rs` |
| M6 load every plugin at startup | 4.1 | d54cbc5 | dark-factory `forge::` tests |
| M7 configuration docs | 4.2 | e496ebb | docs |
| M8a `ForgeRoutes` lookup | 5.1 (part) | ec92d79 | df-core, df-plugins tests |
| M8b forge per project in the UI | 5.1 | 4e1f8ea | df-web tests (385, with PostgreSQL) |
| M9 chart | 6.1 | e76aab8 | helm lint, helm-unittest 66/66 |

## Round 1 (2026-10-30)

- Reviewed range: 4a43d97...e76aab8, four reviewers (Standards: core and host / app, UI, chart; Spec: routing and host / UI, chart, docs)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S3, S4 | Standards | Blocking | `crates/df-plugins/src/router.rs:68`, `:81` | userinfo in a repository URL reaches error messages and the plugin's `base-url` | fix-now | open |
| S1, S2, S6, S9, S11, P7 | Both | P1 | `crates/df-core/src/routing.rs`, `config.rs:552`, `router.rs:61` | hand-parsed URLs; router and UI resolve differently; default port always 443; `impl` order; `{host, apiUrl}` accepts unknown keys | fix-now | open |
| S14, P6, P2, P1 | Both | P1 | `crates/df-plugins/src/forge.rs:38`, `tests/routing.rs:218`, `config.rs:805` | auth scheme stored twice, wiring from config untested; repository host not shown denied; `authScheme` error naming the plugin untested | fix-now | open |
| P3, P4, S101 | Both | P1 | `crates/dark-factory/src/forge.rs` | load-error and routing-table events untested; module doc outdated | fix-now | open |
| S102, S103, P102, P103 | Both | P1 | `crates/df-web/src/config_pages.rs:563`, egress icon tests | non-exhaustive match; not-loaded hint wording; egress icons with two forges untested | fix-now | open |
| P101, P104, P105, P106 | Spec | P1 | chart README, schema tests, `configuration.md` | fail-fast troubleshooting text; unknown host-item keys untested; example without `hosts` on `forgejo`; form hint not in the troubleshooting guide | fix-now | open |
| S5 | Standards | P2 | `routing.rs:61`, `router.rs:51` | plain-HTTP test knob in public APIs; matters when someone wires it into configuration | tech-debt | open |
| P5 | Spec | P2 | `crates/dark-factory/src/forge.rs:121` | skipping a plugin without `pull-requests` untested (no such component exists); matters with the first non-forge plugin | tech-debt | open |
| S7, S8, S10, S12, S13, S104–S114, P8, P9, P10 | Both | P3 | various | naming, small duplications, test-helper layering, pre-MVP config break (P9) | cosmetic | cosmetic |

User decision: Fix blocking + P1

Fix round 1 in four parts: 817d7ce (URL resolution in df-core with `url`), 5d6c69c (router resolves through df-core), 6b4d2b5 (startup log tests, form hints), a310d2b (chart and docs).

## Round 2 (2026-10-30)

- Reviewed range: e76aab8...a310d2b plus the ledger check: blocking S3/S4 and all fix-now rows fixed except P1 (the `authScheme` error named the plugin only through the YAML snippet)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | P1 | `crates/df-core/src/config.rs` | unknown `authScheme` error does not reliably name the plugin | fix-now | fixed (6f39ac1, validated in the routing table) |
| S201, P201 | Both | P1 | `crates/df-plugins/docs/configuration.md` | outdated error texts | fix-now | fixed (6f39ac1) |
| P202 | Spec | P1 | `crates/df-core/src/routing.rs` | `http://` repository URL error without host and reason | fix-now | fixed (6f39ac1) |
| S202 | Standards | P1 | `crates/df-core/src/routing.rs` | invalid-host error echoed the raw value | fix-now | fixed (6f39ac1) |
| S203 | Standards | P1 | `crates/df-core/src/routing.rs` | `route_for` used only by tests | fix-now | fixed (6f39ac1, removed) |
| S204, P203 | Both | P1 | `crates/df-plugins/src/router.rs` | two plain-HTTP switches | fix-now | fixed (6f39ac1) |
| S205 | Standards | P1 | `crates/df-core/src/routing.rs` | wrong doc on `RepoUrl::parse` | fix-now | fixed (6f39ac1) |

User decision: Fix blocking + P1

## Round 3 (2026-10-30)

- Reviewed range: a310d2b...6f39ac1; all round-2 rows fixed, no regressions

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S301 | Standards | Rejected | `crates/df-core/src/config.rs:580` | keep `AuthScheme` typed at parse time | rejected (the spec requires the error to name the plugin; serde cannot, see P1) | rejected |
| P301 | Spec | P3 | `crates/df-core/src/config.rs:584` | non-string `authScheme` (`true`) gets serde's generic error | cosmetic | cosmetic |
| P302 | Spec | P1 | `tasks.md` 1.1 | wording of the `authScheme` check | fixed by the orchestrator | fixed |

No blocking finding open.

## Finish (2026-10-30)

- Container-backed tests ran: `cargo test -p df-web` (387, PostgreSQL through `DOCKER_HOST=unix:///var/run/docker.sock`) and the Bazel suite.
- Bazel: `test --lockfile_mode=error //crates/df-plugins/... //crates/df-core/... //crates/dark-factory/... //crates/df-web/...` 11/11; `build --lockfile_mode=error //crates/... //plugins/...` ok.
- Coverage (task 7.1): `df-plugins` 93.8 % lines; `df-core` `routing.rs` 99.8 %, `config.rs` 95.8 %.
- Tech debt: TD-78 (S5), TD-79 (P5); TD-38 closed by M4a. TD-77 is used by PR #44.
- Fix rounds: 2.
