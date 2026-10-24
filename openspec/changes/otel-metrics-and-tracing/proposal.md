## Why

Dark Factory has no metrics and no traces. No component exposes or exports telemetry, although ADR-0003 settled on `tracing` + OpenTelemetry. Operators cannot see run throughput, failure rates, token spend or egress decisions without querying the database or the web UI. The target audience is homelabs, so a Prometheus scrape endpoint has to work out of the box, OTLP has to be optional, and tracing has to be possible to skip entirely.

## What Changes

- **Control plane and gateway** export OpenTelemetry metrics. They can be scraped from a Prometheus endpoint on a dedicated port (enabled by default), pushed over OTLP, or both. Exporters are selected with the standard `OTEL_METRICS_EXPORTER` variable (`prometheus`, `otlp`, `none`), and the OTLP exporter uses the standard `OTEL_EXPORTER_OTLP_*` variables.
- **Tracing** for the control plane and gateway is optional and **off by default**. When enabled with `OTEL_TRACES_EXPORTER=otlp`, existing `tracing` spans plus new request spans are exported over OTLP.
- **The agent runtime is push-based through the existing agent channel. It gets no OTel SDK and no new egress.** The control plane derives run, turn, tool call, token and push metrics from the messages it already persists, once per message, after the commit. The runtime adds one new typed message, `RuntimeTiming`, which carries measurements only the runtime can take (workspace prepare, harness start, turn, push, finalize durations). Delivery to the control plane goes through the durable outbox and survives pod exit and reconnects. Recording it as a metric on the control plane is best-effort, like every in-memory metric: observations not yet recorded when a control plane process stops are lost.
- Labels that come from the agent are limited to enums and a bounded model set, so a sandbox cannot inflate metric cardinality.
- Helm: `telemetry` values for exporters, the metrics port, OTLP endpoint and headers Secret, the tracing switch and sampler. An optional `ServiceMonitor` and a NetworkPolicy ingress rule for the scraper.

## Capabilities

### New Capabilities
- `telemetry`: exporter selection, the metric instruments of control plane and gateway (including the run metrics derived from the agent channel), tracing, resource attributes and cardinality limits.

### Modified Capabilities
- `agent-channel`: new `RuntimeTiming` agent message, delivered like every other agent message.
- `helm-deployment`: telemetry values, metrics Service ports, optional `ServiceMonitor`, scraper NetworkPolicy ingress.

## Impact

- New dependencies (workspace): `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp`, `opentelemetry-prometheus`, `tracing-opentelemetry` (all on the 0.33 / 0.34 line, released together), plus direct use of `axum` (already in the dependency graph through `tonic`) for `/metrics`. Library crates depend on the `opentelemetry` API only; the SDK and exporters are set up by the new `df-telemetry` crate (`otel` feature), which only the `dark-factory` binary enables.
- `factory-agent` gets no OpenTelemetry or axum dependency (cargo and Bazel); it uses `df-telemetry` for its logging setup and only sends `RuntimeTiming` messages.
- Code: `crates/df-telemetry` (logging setup shared by both binaries, HTTP request span middleware, OTel setup and `/metrics` server), `crates/dark-factory` (telemetry wiring, shutdown flush), `crates/df-k8s` (typed `LogFormat`), `crates/df-agent-channel` (instruments recorded after `persist_batch` commits), `contract/proto/darkfactory/agent/v1/agent.proto` (`RuntimeTiming`), `crates/factory-agent` (timings), gateway instruments.
- Chart: values, schema, Services, NetworkPolicy, optional `ServiceMonitor`, tests, README.
- **Depends on `agent-session-logging`** (subscriber setup that the OTel layer plugs into) and **`iter1-egress-gateway`** (gateway instruments are added to the proxy it implements). Neither change is amended.
