# Tasks

## 1. Dependencies and telemetry setup (after `agent-session-logging`)

- [x] 1.1 Add `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` (`grpc-tonic` with `tls-ring` and `tls-roots` for TLS to the collector; `http-proto` dropped: its rustls client pulls in `aws-lc-rs`, banned by `cargo deny`), `opentelemetry-prometheus`, `tracing-opentelemetry` and `axum` to `[workspace.dependencies]` with minimal features; verify `cargo deny check`, `cargo xtask outdated` and the Bazel lock files (design D2, D3, risks)
- [x] 1.2 Write failing tests for parsing `OTEL_METRICS_EXPORTER` / `OTEL_TRACES_EXPORTER` (defaults, lists, `none`, unknown value) as a pure function over the values
- [x] 1.3 Implement the telemetry setup (now `crates/df-telemetry/src/otel.rs`): providers, resource (`service.name` per subcommand, `service.version`, `service.instance.id`, `OTEL_RESOURCE_ATTRIBUTES`), global registration, shutdown guard bounded by `SHUTDOWN_GRACE` (design D1, D2)
- [x] 1.4 Serve `GET /metrics` with `axum` on `OTEL_EXPORTER_PROMETHEUS_HOST:PORT`; test that a recorded counter appears in the response (design D3)
- [x] 1.5 Add the `tracing_opentelemetry` layer to `init_tracing` (now `df_telemetry::logging`) when traces are enabled; test with an in-memory span exporter that a span is exported and that nothing is installed by default (design D6)
- [x] 1.6 Wire setup and shutdown into `serve` and `gateway`; `migrate` installs no telemetry

## 2. Agent channel: RuntimeTiming

- [x] 2.1 Add `RuntimeTiming` / `RuntimeOperation` to `agent.proto` (body field 17); run `buf format` and regenerate (design D4)
- [x] 2.2 Control plane test-first: a batch containing `RuntimeTiming` advances `last_persisted_seq` without a database row (container-backed test)
- [x] 2.3 Runtime test-first: with the fake ACP agent, one run sends `RuntimeTiming` for workspace prepare, harness start, each turn, each push and finalize, in sequence with the other messages
- [x] 2.4 Implement the timing measurement in `factory-agent` (`runner.rs`, `workspace.rs`)

## 3. Run metrics in the control plane

- [x] 3.1 Write failing tests with an in-memory metric reader: each instrument of the "Run and turn instruments", "Tool, push and runtime instruments" and "Connection instruments" requirements is recorded once per persisted message; replayed messages and rolled-back transactions record nothing (container-backed)
- [x] 3.2 Implement metric effects in `persist_batch`, recorded after commit; connection and reconnect counters in the session handler (design D4)
- [x] 3.3 Write failing tests for `BoundedLabel` (truncation, 32-value cap, `other`) and duration rejection; implement (design D5)
- [x] 3.4 Add the `agent.session` and `agent.persist_batch` spans and the `http.request` span for the HTTP listener

## 4. Gateway metrics (after `iter1-egress-gateway`)

- [x] 4.1 Write failing tests for `df.gateway.connections` (allowed, denied with reason), `df.gateway.active_connections`, `df.gateway.bytes`, `df.gateway.policy_version`; implement in the proxy; add the `gateway.connect` span

## 5. Helm chart

- [x] 5.1 Add failing helm-unittest cases for every scenario in `specs/helm-deployment/spec.md`
- [x] 5.2 Add `telemetry.*` and `networkPolicy.metricsScraper` values and schema (incl. the endpoint-required rule); render `OTEL_*` env, `OTEL_SERVICE_NAME` per component, `metrics` container and Service ports, optional `ServiceMonitor`, scraper ingress rule (design D7)
- [x] 5.3 Document the values and a "Scrape with kube-prometheus-stack" and "Push to an OTel Collector" scenario in the chart README

## 6. Documentation and verification

- [x] 6.1 Add a telemetry reference under `docs/` (variables, defaults, every instrument with type, unit and attributes) and a short how-to for enabling tracing
- [x] 6.2 `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all`, `cargo-machete`, `cargo deny check`; `cargo test` for `dark-factory`, `df-agent-channel`, `factory-agent`; report container-backed tests not run
- [x] 6.3 `cargo llvm-cov` reports ≥ 70 % line coverage for `df-telemetry/src/otel.rs` and the new channel metric code
- [x] 6.4 Extract `crates/df-telemetry` (logging shared by both binaries, HTTP span middleware, OTel setup; Bazel slim targets for `factory-agent`) and type the sandbox log format in `df-k8s`; closes TD-53 and TD-54 (review decision, design D9)
