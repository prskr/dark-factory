# Review log: otel-metrics-and-tracing

- BASE: bb6cacdc170b7d050645bea27155867f152f5935 (rebased from 925e5bfd0565fd6114fc4ccfe3c48216bc5bb033 after the session-logging cli_test fix)
- Store: none
- Status: done
- Implementer agent: ae064e9e277c5fc94

## Round 1 (2026-10-24)

- Implementer: 88a6e32385b784da7711807d777490ff09b92148 (06fb1847e28e436e826f8558dd9c45ac210b4d38 before rebase), tasks 21/21, checks fmt/clippy/machete/deny/outdated ok, helm-unittest 42/42, tests pass (dark-factory, df-agent-channel, df-gateway, factory-agent), container tests ran (podman)
- Reviewed range: BASE...88a6e32385b784da7711807d777490ff09b92148
- Deviation: OTLP `http/protobuf` dropped (reqwest-rustls pulls banned aws-lc-rs); grpc only, design D2 fallback

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P1 | Spec | Blocking | `crates/dark-factory/src/main.rs:142,540` | flush gets a second 30 s after the 30 s listener drain; k8s default terminationGracePeriod 30 s (verified) | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S2 | Standards | Blocking | `crates/dark-factory/src/telemetry.rs:458,474` | scoped-subscriber span tests in shared unit binary: callsite-interest race (verified) | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| P2 | Spec | Blocking | `crates/dark-factory/src/telemetry.rs:511` | "Flush on shutdown" untested (reachable and hanging exporter) | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| P3 | Spec | Blocking | `telemetry.rs:190`, `docs/telemetry.md:36` | SDK logs export failures at ERROR, spec/docs say WARN; untested | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| P4 | Spec | Blocking | `telemetry.rs:192,203` | `OTEL_EXPORTER_OTLP_*PROTOCOL` silently ignored; reject non-grpc | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| P5 | Spec | Blocking | `main.rs:818`, gateway health, `/metrics` | HTTP request spans only on UI router | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| P6 | Spec | Blocking | `deploy/helm/.../_helpers.tpl` | scraper selector can match sandbox namespace; not enforced/tested | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| P7 | Spec | Blocking | `crates/factory-agent/src/tests.rs:5054` | two-turn timing and push timing untested | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| P8 | Spec | Blocking | telemetry/agent-channel scenarios | pull+push together, sampling, unknown exporter exit code, timing across reconnect untested | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| P9 | Spec | Blocking | `telemetry.rs:181` | `service.instance.id` per series via Prometheus unverified (maybe only `target_info`) | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S10 | Standards | P1 | `telemetry.rs:187`, `main.rs:94` | "serving Prometheus metrics" logged before subscriber init: dropped (verified) | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S1 | Standards | P1 | `telemetry.rs:105-272` | file structure: impl not next to type, private helpers between public items | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S3 | Standards | P1 | `telemetry.rs:198` | `start_with` sets the global meter provider from tests | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S4 | Standards | P1 | `crates/df-gateway/tests/spans.rs:78` | sleep-polling loop | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S5 | Standards | P1 | `df-gateway/src/proxy.rs:657`, `metrics.rs:66` | verdict mapped twice, span vs metric can disagree | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| P10 | Spec | P1 | `deploy/helm/dark-factory/values.schema.json` | whole file reformatted, hides the schema change | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S6 | Standards | P2 | `crates/factory-agent/src/runner.rs:664…` | timing boilerplate repeated 5× | tech-debt | tech-debt (TD-55) |
| S7 | Standards | P1 | gateway/channel metrics plumbing | `Arc<Metrics>` threaded through many constructors | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S8 | Standards | P1 | `df-agent-channel/src/session.rs:1278` | `use` mid test module | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| S9 | Standards | P1 | `df-agent-channel/src/metrics.rs:89` | `duration_ms: u64` next to `Duration` | fix-now | fixed (c23ebc5b858fdb99f8ade4dd59a99a756a567efc) |
| A1 | Spec | follow-up | helm-deployment spec:4, design.md:35,93, tasks 1.1 | artifacts still mention `http/protobuf`; D4 double-count note | opsx:update | fixed (spec update) |

User decision: Fix blocking + P1 (P1–P9, S2, S10, S1, S3, S4, S5, P10; P3 by changing spec/docs to ERROR; S6 to tech debt; A1 + P3 spec text via /opsx:update before archive)
User adjustment (mid-round): S7, S8, S9 moved to P1 fix-now

## Round 2 (2026-10-24)

- Implementer: c23ebc5b858fdb99f8ade4dd59a99a756a567efc, tasks 21/21, checks fmt/clippy/machete/deny ok, helm 43/43, cargo tests pass, Bazel 9/9 incl. spans_test/cli_test, container tests ran (podman)
- Reviewed range: 88a6e32385b784da7711807d777490ff09b92148...c23ebc5b858fdb99f8ade4dd59a99a756a567efc
- Notes: P3 test asserts force_flush Err + process keeps serving, not the ERROR log; dark-factory tests/spans.rs compiles src via #[path]; S6 deferred

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S11 (=S2) | Standards | Blocking | `crates/dark-factory/tests/spans.rs:8` | `#[path]` inclusion re-runs logging/telemetry unit tests (scoped subscribers) next to the global one; race remains (verified) | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| P12 (=P2) | Spec | Blocking | `crates/dark-factory/src/telemetry.rs:656` | metrics flush on shutdown untested | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| P11 (=P5) | Spec | Blocking | `crates/dark-factory/src/telemetry.rs:403` | `/metrics` router has no request span (verified) | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| P13 | Spec | P1 | `deploy/helm/.../_helpers.tpl:247` | only `kubernetes.io/metadata.name` checked; other sandbox-ns labels can match | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S1 (r2) | Standards | P1 | `telemetry.rs:77` | private `SamplingKind` between `Sampling` and its impls | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S12 | Standards | P1 | `crates/dark-factory/src/main.rs:43` | `flush_budget` between constants | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S13 | Standards | P1 | `main.rs:39,845` | `SIGNALLED` process global links signal and main | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S16 | Standards | P1 | `telemetry.rs:663` | OTLP `force_flush` test wait without timeout | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S17 | Standards | P1 | `crates/df-agent-channel/tests/agent_port.rs:1033` | new sleep-poll `eventually` wait | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S14 | Standards | P1 | `crates/df-gateway/src/metrics.rs:9` | `decision` above `pub struct Metrics` | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S15 | Standards | P1 | `df-gateway/src/metrics.rs:74` | double branch on verdict | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S19 | Standards | P1 | `telemetry.rs:577`, `factory-agent/src/tests.rs:5483` | `let _ =` without comment | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| S18 | Standards | P1 | `main.rs:837` | generic HTTP span middleware lives in df-gateway | fix-now | fixed (01f222d99e63cd66a834ccc84bc2c57f41de7a05) |
| A2 | Spec | follow-up | spec/design/tasks (10 items from spec review r2) | artifact text for ERROR level, grpc only, sampler parsing, shared grace, scraper selector rule, resource attrs | opsx:update | fixed (spec update) |

User decision: Blocking + P1 + S18 (S11, P12, P11 via spec exclusion of /metrics in /opsx:update, P13 documented, S1 r2, S12–S17, S19, S18 fix-now; A2 via /opsx:update)
User adjustment (round 2): S11 and S18 resolved by a new df-telemetry crate (logging default, clap, http, otel features); also resolves TD-53 and TD-54 (entries to be deleted). Design line 'SDK and exporters set up in the dark-factory binary' goes into the /opsx:update (A2).

## Round 3 (2026-10-24)

- Implementer: 01f222d99e63cd66a834ccc84bc2c57f41de7a05, tasks 21/21, checks fmt/clippy/machete/deny/outdated ok, cargo tests pass, Bazel 14/14, container tests ran (podman)
- Reviewed range: c23ebc5b858fdb99f8ade4dd59a99a756a567efc...01f222d99e63cd66a834ccc84bc2c57f41de7a05 (new crate df-telemetry reviewed in full)
- Notes: TD-53/TD-54 deleted from docs/tech_debt.md; Bazel has one df_telemetry target with all features (factory-agent links OTel under Bazel only)

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| P14+S20 | Spec+Standards | P1 | `crates/df-telemetry/BUILD.bazel:10` | one all-features Bazel target: `test-util` in all prod binaries, OTel/axum in factory-agent (verified) | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S21+P15 | Standards+Spec | P1 | `crates/df-telemetry/src/lib.rs:9`, `AGENTS.md:50` | docs claim no OTel/axum in sandbox; false under Bazel; AGENTS line misses clap/test-util | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S22 | Standards | P1 | `crates/df-telemetry/src/otel.rs:11` | `anyhow` in a library crate's public API; AGENTS wants thiserror | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S23 | Standards | P1 | `crates/dark-factory/src/main.rs:38` | `type Signalled` between constants | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S24 | Standards | P1 | `crates/df-telemetry/src/testing.rs:20` | private `Sink` between public items | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S25 | Standards | P1 | `crates/df-telemetry/src/logging.rs:49` | `build` pub without external caller | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S26 | Standards | P1 | `docs/tech_debt.md:19` | Origin line still says TD-52 to TD-54 | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S27 | Standards | P1 | `crates/df-gateway/src/metrics.rs:67` | compares `decision()` result to string literal | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| S28 | Standards | P1 | `crates/df-gateway/BUILD.bazel:71` | unused df_telemetry dep on spans_test | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |
| P16 | Spec | P3 | `crates/dark-factory/src/main.rs:155` | main's signal→flush wiring not tested end to end (parts tested) | cosmetic | cosmetic |
| A3 | Spec | follow-up | proposal/design/tasks | 22 artifact text items (spec review r3) | opsx:update | fixed (spec update) |
| U1 | User | P1 | `deploy/helm/dark-factory/templates` | fail render when ServiceMonitor enabled but `networkPolicy.metricsScraper.namespaceSelector` empty (user request); spec text via /opsx:update | fix-now | fixed (656d7bb419e849ce9c9974c738e4ecc12a8ec4e6) |

User decision: Fix P1 round, then finish (P14+S20, S21+P15, S22, S23–S28; P16 cosmetic; A3 via /opsx:update)

## Round 4 (2026-10-24)

- Implementer: 656d7bb419e849ce9c9974c738e4ecc12a8ec4e6, tasks 21/21, checks fmt/clippy/machete/deny ok, helm lint + unittest 45/45, cargo tests pass, Bazel 11/11, factory-agent Bazel deps contain no opentelemetry (cquery), container tests ran (podman)
- Reviewed range: 01f222d99e63cd66a834ccc84bc2c57f41de7a05...656d7bb419e849ce9c9974c738e4ecc12a8ec4e6

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| S29 | Standards | P1 | `crates/df-telemetry/src/otel.rs:56-64` | `{source}` in message + `source` field: cause printed twice with `{err:#}` (verified) | fix-now | fixed (79a08ba6151d761292ad38983f71b509e83a9ea6) |
| S30 | Standards | P1 | `crates/df-telemetry/src/otel.rs:48-54` | empty-string sentinel for `reason`; use `Option` | fix-now | fixed (79a08ba6151d761292ad38983f71b509e83a9ea6) |
| S31 | Standards | P1 | `crates/df-telemetry/BUILD.bazel:32` | hand-written slim deps lack keep-in-sync note | fix-now | fixed (79a08ba6151d761292ad38983f71b509e83a9ea6) |
| P17 | Spec | follow-up | `specs/helm-deployment/spec.md:25` | ServiceMonitor scenario fails on defaults now; add selector rule + 2 scenarios | opsx:update | fixed (spec update) |

User decision: Fix S29–S31, then finish (P17 via /opsx:update)

## Round 5 (2026-10-24)

- Implementer: 79a08ba6151d761292ad38983f71b509e83a9ea6, checks fmt/clippy/machete ok (hooks re-run by orchestrator after regenerating .bazel_env: all pass), df-telemetry tests pass, Bazel 5/5; container tests skipped: Podman socket down (dark-factory df_test_postgres tests, Bazel dark_factory_test)
- Reviewed range: 656d7bb419e849ce9c9974c738e4ecc12a8ec4e6...79a08ba6151d761292ad38983f71b509e83a9ea6

| ID | Axis | Class | Where | Finding | Decision | Status |
|----|------|-------|-------|---------|----------|--------|
| — | — | — | — | round 5 reviewed inline by the orchestrator (56 lines): S29–S31 verified, no new findings | — | — |

## Result

- Status: done (no blocking finding open)
- Fixed in review: P1–P17, S1–S5, S7–S31 (except cosmetic P16), U1 (user request: ServiceMonitor requires scraper selector)
- New crate `df-telemetry` (user decision); resolved TD-53, TD-54
- Tech debt: TD-55 (S6)
- Spec artifacts updated via /opsx:update (P3, P11, P17, A1–A3)
- Container tests not run in round 5 (Podman socket down): dark-factory df_test_postgres tests, Bazel dark_factory_test; they passed in rounds 1–4
