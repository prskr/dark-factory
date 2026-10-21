## Purpose

Defines the OpenTelemetry metrics and optional traces that the control plane and gateway produce, how operators select exporters (Prometheus scrape, OTLP push, none), and how run metrics from untrusted sandboxes are recorded without loss and without unbounded cardinality.

## ADDED Requirements

### Requirement: Metrics exporter selection
The control plane and gateway SHALL read `OTEL_METRICS_EXPORTER` as a comma-separated list of `prometheus`, `otlp` and `none`. The default is `prometheus`. With `prometheus` the component SHALL serve the Prometheus text format at `GET /metrics` on `OTEL_EXPORTER_PROMETHEUS_HOST` (default `0.0.0.0`) and `OTEL_EXPORTER_PROMETHEUS_PORT` (default `9464`), a port separate from all other listeners. With `otlp` the component SHALL push metrics periodically over OTLP configured through the standard `OTEL_EXPORTER_OTLP_*` variables (endpoint, protocol, headers, insecure, timeout) and `OTEL_METRIC_EXPORT_INTERVAL` (default 60 s). `none` alone SHALL disable metrics. An unknown value SHALL stop the process at startup with an error naming the variable.

#### Scenario: Default scrape endpoint
- **WHEN** the control plane starts without `OTEL_METRICS_EXPORTER`
- **THEN** `GET /metrics` on port 9464 returns metrics in the Prometheus text format and no OTLP connection is attempted

#### Scenario: Pull and push together
- **WHEN** the gateway starts with `OTEL_METRICS_EXPORTER=prometheus,otlp`
- **THEN** it serves `/metrics` and pushes the same instruments over OTLP

#### Scenario: Metrics disabled
- **WHEN** a component starts with `OTEL_METRICS_EXPORTER=none`
- **THEN** it opens no metrics port and sends no metrics

#### Scenario: Unknown exporter
- **WHEN** a component starts with `OTEL_METRICS_EXPORTER=statsd`
- **THEN** it exits with a non-zero status and reports the invalid `OTEL_METRICS_EXPORTER` value

#### Scenario: Collector unreachable
- **WHEN** `otlp` is selected and the OTLP endpoint does not accept connections
- **THEN** the component keeps serving its other functions and logs the export failure at WARN at most once per export interval

### Requirement: Optional tracing
Tracing SHALL be disabled unless `OTEL_TRACES_EXPORTER=otlp` is set; the default and `none` SHALL install no trace exporter and open no connection. When enabled, the control plane and gateway SHALL export their spans over OTLP using the standard `OTEL_EXPORTER_OTLP_*` variables and SHALL honour `OTEL_TRACES_SAMPLER` and `OTEL_TRACES_SAMPLER_ARG` (default `parentbased_always_on`). Spans SHALL exist at least for each HTTP request, each agent channel session, each persisted agent message batch and each gateway proxy connection. Span attributes SHALL NOT contain credentials, prompt content or tool input/output.

#### Scenario: Tracing off by default
- **WHEN** the control plane starts without `OTEL_TRACES_EXPORTER`
- **THEN** no trace data is exported and no OTLP connection is opened for traces

#### Scenario: Tracing enabled with sampling
- **WHEN** the control plane runs with `OTEL_TRACES_EXPORTER=otlp`, `OTEL_TRACES_SAMPLER=traceidratio` and `OTEL_TRACES_SAMPLER_ARG=0.1`
- **THEN** about 10 % of agent channel batch spans are exported

### Requirement: Resource attributes
Every exported metric and span SHALL carry `service.name` (`dark-factory-control-plane` or `dark-factory-gateway`), `service.version` (the binary version) and `service.instance.id` (pod name when available, otherwise host name). Attributes from `OTEL_RESOURCE_ATTRIBUTES` SHALL be added, and `OTEL_SERVICE_NAME` SHALL override `service.name`.

#### Scenario: Two replicas
- **WHEN** two control plane replicas export metrics
- **THEN** their series differ by `service.instance.id`

### Requirement: Run metrics from the agent channel
The control plane SHALL record the following instruments from agent messages. Each message SHALL be recorded exactly once per control plane process: only after the transaction that persists it commits, and never for a message dropped as already persisted.

| Instrument | Type | Attributes | Source |
|------------|------|------------|--------|
| `df.runs.ended` | counter | `outcome` | `RunReport` |
| `df.run.duration` | histogram (s) | `outcome` | run start in the database to `RunReport` |
| `df.agent.turns` | counter | `stop_reason` | transcript `TurnEnded` |
| `df.agent.tool_calls` | counter | `kind`, `origin`, `status` | first `completed` or `failed` status of a tool call |
| `df.agent.tokens` | counter | `model`, `token_type` (`input`, `output`, `cache_read`, `cache_write`) | `TurnEnded.usage` |
| `df.agent.pushes` | counter | `target`, `status` | `PushReport` |
| `df.agent.runtime.duration` | histogram (s) | `operation`, `result` | `RuntimeTiming` |
| `df.agent.connections` | up-down counter | none | open agent channel streams |
| `df.agent.reconnects` | counter | none | `Hello` for a run that already had a stream |

All instruments SHALL also carry `project` (the project key from the database).

#### Scenario: Replayed batch
- **WHEN** a runtime reconnects and replays a `TurnEnded` event that was already persisted
- **THEN** `df.agent.turns` is not incremented again

#### Scenario: Failed transaction
- **WHEN** persisting a batch containing a `RunReport` fails and the transaction rolls back
- **THEN** `df.runs.ended` is unchanged until the batch is persisted on a later attempt

#### Scenario: Token usage
- **WHEN** a `TurnEnded` with usage `{model_id: "claude-sonnet-5-5", input_tokens: 100, output_tokens: 20}` is persisted
- **THEN** `df.agent.tokens` increases by 100 for `token_type=input` and by 20 for `token_type=output` with `model=claude-sonnet-5-5`

### Requirement: Bounded attributes from untrusted input
Attribute values that originate from a sandbox SHALL be limited to the protobuf enum names of the field (unknown numbers recorded as `unspecified`), except `model`. `model` SHALL be truncated to 64 bytes, and after 32 distinct values per process further values SHALL be recorded as `other`. Durations from `RuntimeTiming` that are zero, or larger than 7 days, SHALL be dropped and counted in `df.agent.runtime.rejected`. Run ids, task keys, tool titles, repository URLs and host names SHALL NOT be metric attributes.

#### Scenario: Model id flood
- **WHEN** runs report 100 distinct model ids
- **THEN** `df.agent.tokens` has at most 33 distinct `model` values

#### Scenario: Absurd duration
- **WHEN** a `RuntimeTiming` reports 30 days
- **THEN** it is not recorded in `df.agent.runtime.duration` and `df.agent.runtime.rejected` increases by one

### Requirement: Gateway metrics
The gateway SHALL record `df.gateway.connections` (counter; `decision` = `allowed` or `denied`, `reason` = the deny reason enum or `none`), `df.gateway.active_connections` (up-down counter), `df.gateway.bytes` (counter; `direction` = `upstream` or `downstream`) and `df.gateway.policy_version` (gauge; the last applied allowlist version). Destination hosts SHALL NOT be attributes.

#### Scenario: Denied CONNECT
- **WHEN** a sandbox issues a CONNECT to a host that is not allowlisted
- **THEN** `df.gateway.connections{decision="denied"}` increases by one

### Requirement: Telemetry shutdown
On a shutdown signal the control plane and gateway SHALL flush pending OTLP metrics and spans within the existing shutdown grace period and SHALL NOT delay shutdown beyond it when the collector is unreachable.

#### Scenario: Flush on shutdown
- **WHEN** the control plane receives SIGTERM with OTLP metrics enabled and a reachable collector
- **THEN** the collector receives a final export before the process exits
