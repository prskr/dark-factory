## Context

- No metrics or traces exist. `design/tech-stack.md` / ADR-0003 settle on `tracing` + OpenTelemetry.
- `dark-factory` is one binary for control plane (`serve`), gateway (`gateway`) and `migrate`. After `agent-session-logging` its subscriber is set up in `init_tracing(format)` after `Cli::parse()`.
- The control plane HTTP endpoint is a hand-rolled placeholder (`serve_http_health`); `axum` is in `Cargo.lock` through `tonic`'s `router` feature.
- The agent channel (`df-agent-channel/src/session.rs`, `persist_batch`) drops already persisted sequence numbers and applies each new message inside one transaction. This gives an at-most-once hook for metrics.
- Sandboxes are untrusted and network-restricted (`run-network-policy`); their only path to the control plane is the mTLS agent channel with the durable outbox.
- Instance NetworkPolicies (`templates/netpol.yaml`) restrict ingress only.

## Goals / Non-Goals

**Goals:**
- Prometheus scraping works with default values; OTLP is opt-in; tracing is opt-in.
- Standard `OTEL_*` variables, no custom configuration surface besides Helm values that render them.
- No OTel dependency and no new egress for the sandbox runtime; its messages reach the control plane reliably through the outbox.
- Metric recording on the control plane is best-effort (at most once per message); the database, not the metrics, is the record of what happened.

**Non-Goals:**
- Logs over OTLP (stdout logs from `agent-session-logging` stay the log path).
- Spans in the sandbox runtime and trace context propagation over the agent channel or between gateway and control plane.
- Dashboards and alert rules (can follow as a separate chart artifact).
- A gauge of runs per state (each replica would report the same database count; needs leader-only reporting — later).
- Metrics for `migrate`.

## Decisions

### D1: Libraries use the OTel API, `df-telemetry` owns the SDK

`df-agent-channel` and the gateway take their instruments from `opentelemetry::global::meter("<crate>")`, bundled in a small `Metrics` struct per crate that the constructors receive as `Arc<Metrics>`; the default uses the global meter, tests inject a meter with an in-memory reader. `df_telemetry::otel` (feature `otel`, enabled only by `dark-factory`, see D9) builds the `SdkMeterProvider` / `SdkTracerProvider`, registers them globally and returns a guard that flushes on shutdown. Without a provider (tests, `none`) the global meter is a no-op.

Alternative: only the global meter, no injected struct. Rejected: parallel tests share the process-global provider and cannot assert their own instruments.

### D2: Exporters from standard env vars

`OTEL_METRICS_EXPORTER` and `OTEL_TRACES_EXPORTER` are parsed by `df_telemetry::otel` (the Rust SDK does not auto-configure exporters from them). Defaults deviate from the OTel spec on purpose: metrics `prometheus` (spec: `otlp`) and traces `none` (spec: `otlp`), because a homelab without a collector must not log export errors forever. The OTLP builders resolve `OTEL_EXPORTER_OTLP_*` themselves (0.33 behavior: schemeless endpoints default to `https`, `OTEL_EXPORTER_OTLP_INSECURE` for plaintext). Only `grpc` is offered (tonic, already a dependency; TLS through `tls-ring` with the native root store via `tls-roots`, which the distroless static base image provides and which honours a mounted private CA). `http/protobuf` would need `reqwest` with rustls, which pulls in `aws-lc-rs`, banned by `cargo deny`. Any `OTEL_EXPORTER_OTLP_PROTOCOL`, `_METRICS_PROTOCOL` or `_TRACES_PROTOCOL` value other than `grpc` stops the process at startup with an error naming the variable. The SDK's `PeriodicReader` logs failed exports at ERROR, once per export interval.

Alternative: a `telemetry:` section in the instance config. Rejected: telemetry must be installed together with the subscriber before the config file is read, and the standard variables are what collector documentation tells operators to set.

### D3: Prometheus via `opentelemetry-prometheus`, served by `axum`

`opentelemetry-prometheus` (released in lockstep with the SDK, 0.33.1) provides a reader with a `prometheus::Registry`; a small `axum` router serves `GET /metrics` on its own listener and port. Instrument names follow OTel conventions (`df.agent.turns`); the exporter renders them as `df_agent_turns_total`.

Alternatives: OTLP-only and let Prometheus ingest OTLP (`--web.enable-otlp-receiver`) — rejected as the default because many homelab Prometheus installs (kube-prometheus-stack) don't enable it. Hand-written text exposition — rejected, more code to maintain than the exporter crate.

### D4: Agent metrics are derived and relayed, not exported by the runtime

`persist_batch` collects "metric effects" alongside its existing `Effects` while it applies messages, and records them after `tx.commit()`. Replayed and rolled-back messages therefore never count. A batch that fails is retried by the runtime, and the metrics are recorded on the attempt that commits. Turn duration, prepare, push, harness start and finalize durations cannot be derived reliably (no timestamps on `PhaseChanged`, replay skews arrival times), so the runtime measures them and sends `RuntimeTiming` (new `AgentMessage.body` field 17):

```proto
message RuntimeTiming {
  RuntimeOperation operation = 1;
  uint64 duration_ms = 2;
  bool ok = 3;
}
enum RuntimeOperation {
  RUNTIME_OPERATION_UNSPECIFIED = 0;
  RUNTIME_OPERATION_WORKSPACE_PREPARE = 1;
  RUNTIME_OPERATION_HARNESS_START = 2;
  RUNTIME_OPERATION_TURN = 3;
  RUNTIME_OPERATION_PUSH = 4;
  RUNTIME_OPERATION_FINALIZE = 5;
}
```

The control plane does not store `RuntimeTiming` in the database; it only advances `last_persisted_seq` and records the histogram after commit. `RuntimeTiming` metrics are best-effort: if the control plane process stops between commit and recording, the sequence is already acknowledged, the runtime does not resend the message and the observation is lost. Persisting timings or a durable export path was rejected; a lost histogram sample on a control plane crash does not justify a table or a second delivery path.

Alternative: OTLP from the sandbox to a collector — rejected (decided with the user): needs an egress exception reachable by agent code, and in-memory buffers lose data when the pod ends.

Tool calls are counted on every `ToolCallUpdate` (or `ToolCall`) with status `completed`/`failed`; ACP sends the terminal status once. `kind` and `origin` come from the stored `tool_call` event of that `tool_call_id` (one extra query per terminal update, inside the same transaction). A `ToolCall` that already carries a terminal status followed by a terminal `ToolCallUpdate` counts twice.

### D5: Cardinality guard

A small `BoundedLabel` (mutex-protected `HashSet<String>`, capacity 32, values truncated to 64 bytes) maps model ids; everything else uses `as_str_name()` of the protobuf enum. `project` comes from the database, not from the sandbox.

### D6: Tracing layer

With `OTEL_TRACES_EXPORTER=otlp`, `df_telemetry::logging::init_tracing(format, extra)` adds a `tracing_opentelemetry` layer (filtered by the same `RUST_LOG`) to the existing fmt layer. New spans: `http.request` (method, route, status; UI and gateway health routers through `df_telemetry::http::request_span`, no span for the `/metrics` scrape endpoint), `agent.session` (run id as span attribute — allowed on spans, not on metrics), `agent.persist_batch` (message count), `gateway.connect` (decision; no host for denied connections). `df_telemetry::otel` parses `OTEL_TRACES_SAMPLER` (`always_on`, `always_off`, `traceidratio`, `parentbased_*`) and `OTEL_TRACES_SAMPLER_ARG` (ratio from 0 to 1) and builds the sampler; invalid values stop startup with an error naming the variable.

### D7: Helm

```yaml
telemetry:
  metrics:
    exporters: [prometheus]      # prometheus, otlp; [] = off
    port: 9464
    serviceMonitor: { enabled: false, labels: {}, interval: "" }
  traces:
    enabled: false
    sampler: parentbased_always_on
    samplerArg: ""
  otlp:
    endpoint: ""
    protocol: grpc               # grpc only (D2)
    insecure: false
    headersSecret: ""            # Secret with key `headers` -> OTEL_EXPORTER_OTLP_HEADERS (HTTP headers or gRPC metadata)
networkPolicy:
  metricsScraper:
    namespaceSelector: {}        # empty = no scraper ingress rule; required when the ServiceMonitor renders with network policies on;
                                 # selecting the sandbox namespace by kubernetes.io/metadata.name fails the render
    podSelector: {}
```

Settings are shared by control plane and gateway; `OTEL_SERVICE_NAME` is set per component by the templates. Per-component overrides can be added when someone needs them.

### D8: Shutdown budget

The shutdown grace period (30 s, the default `terminationGracePeriodSeconds`) counts from the signal and is shared: the listeners drain first, the telemetry flush gets what is left, possibly nothing. The signal instant is passed from `main` to the `serve` and `gateway` commands; the budget is a pure function of signal time, now and grace.

### D9: `df-telemetry` crate

Logging setup (shared by `dark-factory` and `factory-agent`), the HTTP request span middleware and the OTel setup live in `crates/df-telemetry`, with features: none (`LogFormat`, `RUST_LOG` filter, `init_tracing`), `clap` (`ValueEnum` for `LogFormat`), `http` (`request_span`, axum), `otel` (exporters, `trace_layer`, `/metrics`, shutdown) and `test-util` (log capture helpers). `dark-factory` enables `clap`, `http`, `otel`; `df-gateway` `http`; `df-k8s` the default (typed `LogFormat` for sandboxes); `factory-agent` only `clap`, so the sandbox runtime links neither OpenTelemetry nor axum. Bazel builds the full variant (`clap`, `http`, `otel`) shared by control plane, gateway and df-k8s, so their crate graph has one `LogFormat`, and slim variants for `factory-agent` (`df_telemetry_slim`, `df_telemetry_slim_test_util`). This replaces the duplicated logging setup and the string-typed sandbox log format (TD-53, TD-54).

## Risks / Trade-offs

- [`opentelemetry-prometheus` depends on the `prometheus` crate (and `protobuf`)] → Check `cargo deny` and `cargo xtask outdated` first (task 1.1). Fallback: OTLP-only plus the documented Prometheus OTLP receiver.
- [OTel Rust API churn between 0.x releases] → All OTel crates are pinned to one release line in `[workspace.dependencies]`; usage is concentrated in `df-telemetry/src/otel.rs`.
- [Metrics recorded after commit are lost if the process crashes between commit and record] → Accepted as best-effort: counters reset on restart anyway. The database stays the source of truth for runs, turns, tool calls, tokens and pushes; `RuntimeTiming` is not stored, so those samples are lost in that window without a way to recover them.
- [Unauthenticated `/metrics`] → Separate port, never exposed through the ingress, NetworkPolicy allows only the configured scraper; metrics carry no run ids or hosts.
- [Gateway instruments depend on `iter1-egress-gateway`] → Gateway exporter setup lands independently; instruments are added in the proxy once that change is merged.

## Migration Plan

Additive. Older runtimes don't send `RuntimeTiming`; older control planes ignore it (sequence preserved, see `agent-channel` spec). Rollback: revert images; the extra env vars and the `metrics` port are unused by older binaries.

## Open Questions

- Exact histogram bucket boundaries for `df.run.duration` and `df.agent.runtime.duration` (minutes to hours vs. milliseconds to seconds). Decided during implementation with the SDK's view API; does not affect the specs.
