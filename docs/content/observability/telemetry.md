---
title: Telemetry
weight: 2
---

The control plane (`dark-factory serve`) and the gateway (`dark-factory gateway`) export OpenTelemetry
metrics and, optionally, traces. `migrate` and the sandbox runtime export nothing: the runtime sends its
measurements to the control plane over the agent channel (`RuntimeTiming`), and the control plane derives
the run metrics from the messages it persists.

```mermaid
flowchart LR
    runtime[Sandbox runtime] -- "agent messages incl. RuntimeTiming (mTLS, durable outbox)" --> cp[Control plane]
    cp -- "after commit" --> meters[OTel meter provider]
    gw[Gateway] --> meters
    meters -- "GET /metrics :9464" --> prom[Prometheus]
    meters -- "OTLP/gRPC push" --> coll[OTel Collector]
```

## Configuration

Standard `OTEL_*` variables; Helm renders them from `telemetry.*` (see `deploy/helm/dark-factory/README.md`).

| Variable | Required | Default | Effect |
|----------|----------|---------|--------|
| `OTEL_METRICS_EXPORTER` | no | `prometheus` | Comma-separated list of `prometheus`, `otlp`, `none`. `none` alone disables metrics. An unknown value stops the process at startup, the error names the variable. |
| `OTEL_EXPORTER_PROMETHEUS_HOST` | no | `0.0.0.0` | Address of the scrape endpoint. |
| `OTEL_EXPORTER_PROMETHEUS_PORT` | no | `9464` | Port of `GET /metrics`, separate from all other listeners. |
| `OTEL_TRACES_EXPORTER` | no | `none` | `otlp` exports spans; `none` installs no exporter and opens no connection. |
| `OTEL_TRACES_SAMPLER`, `OTEL_TRACES_SAMPLER_ARG` | no | `parentbased_always_on` | `always_on`, `always_off`, `traceidratio` (argument: ratio from 0 to 1, default 1), each also with the prefix `parentbased_`. Other values stop the process at startup. |
| `OTEL_EXPORTER_OTLP_ENDPOINT`, `_HEADERS`, `_INSECURE`, `_TIMEOUT` | with `otlp` | none | OTLP over gRPC. `OTEL_EXPORTER_OTLP_PROTOCOL` (also `_METRICS_PROTOCOL`, `_TRACES_PROTOCOL`) accepts only `grpc`; any other value stops the process at startup. An endpoint without scheme uses `https`; the native root store and a mounted private CA are honoured. |
| `OTEL_METRIC_EXPORT_INTERVAL` | no | `60000` (ms) | Push interval of the `otlp` metrics exporter. |
| `OTEL_SERVICE_NAME` | no | `dark-factory-control-plane` / `dark-factory-gateway` | `service.name`. |
| `OTEL_RESOURCE_ATTRIBUTES` | no | none | Extra resource attributes. |

Every metric and span carries the resource attributes `service.name`, `service.version` and
`service.instance.id` (pod name, else host name). `service.version` is the release version the binary was built
with, `dev` for unstamped builds. Build-time variable `DF_VERSION` (optional, default `dev`; an empty value also
gives `dev`): Bazel sets it from `--//build:version=<version>`, plain cargo with `DF_VERSION=<version> cargo build`.
`OTEL_RESOURCE_ATTRIBUTES=service.version=…` overrides it.

Export failures never affect the component; the SDK logs them at ERROR once per export interval. On shutdown
pending exports are flushed within the shutdown grace period (30 s) counted from the signal: what the
listeners did not use is left for the flush.

## Instruments

Recording is best-effort: the control plane records after the transaction that persists a message committed,
never for replayed messages, so a crash between commit and recording loses the sample. Run instruments carry
`project` (project key). Prometheus renders `df.agent.turns` as `df_agent_turns_total` and appends the unit
(`df_run_duration_seconds`).

### Control plane

| Instrument | Type | Unit | Attributes | Source |
|------------|------|------|------------|--------|
| `df.runs.ended` | counter | | `project`, `outcome` | `RunReport` |
| `df.run.duration` | histogram | s | `project`, `outcome` | run start to `RunReport` |
| `df.agent.turns` | counter | | `project`, `stop_reason` | `TurnEnded` |
| `df.agent.tokens` | counter | | `model`, `token_type` (`input`, `output`, `cache_read`, `cache_write`) | stored gateway usage records (a record that is stored twice counts once) |
| `df.agent.tool_calls` | counter | | `project`, `kind`, `origin`, `status` (`completed`, `failed`) | terminal status of a tool call |
| `df.agent.pushes` | counter | | `project`, `target`, `status` | `PushReport` |
| `df.agent.runtime.duration` | histogram | s | `project`, `operation`, `result` (`ok`, `error`) | `RuntimeTiming` |
| `df.agent.runtime.rejected` | counter | | `project`, `operation` | `RuntimeTiming` with a duration of zero or more than 7 days |
| `df.agent.connections` | up-down counter | | `project` | open agent channel streams |
| `df.agent.reconnects` | counter | | `project` | streams of runs that had one before |
| `df.outbox.jobs` | counter | | `kind` (for example `lifecycle_event.deliver`), `result` (`done`, `retry`, `gave_up`) | every outbox job attempt, on the leader |
| `df.lifecycle_event.deliveries` | counter | | `result` (`delivered`, `failed`, `refused`) | every lifecycle event delivery attempt; given-up deliveries are counted by `df.outbox.jobs{kind="lifecycle_event.deliver",result="gave_up"}` |
| `df.webhook.requests` | counter | | `plugin`, `outcome` (`accepted`, `unauthorized`, `not_found`, `too_large`, `invalid`, `error`) | every request to `POST /webhooks/{plugin}`; Prometheus: `df_webhook_requests_total` |

`operation` is one of `workspace_prepare`, `harness_start`, `turn`, `push`, `finalize`. `outcome`,
`stop_reason`, `kind`, `origin`, `target` and `status` are the lower-case protobuf enum names without
prefix (`end_turn`, `task_branch`); unknown numbers are `unspecified`. The `kind` of `df.outbox.jobs` is the
outbox job kind instead. `refused` means the project subscription's host is not a public address.
`plugin` of `df.webhook.requests` is the plugin name under `plugins` for a loaded plugin, or `unknown`
for any other name.

Attributes that originate from the sandbox are bounded: `model` is cut to 64 bytes and after 32 distinct
values per process further ones are `other`. Run ids, task keys, tool titles, repository URLs and hosts are
never attributes.

### Gateway

| Instrument | Type | Unit | Attributes | Meaning |
|------------|------|------|------------|---------|
| `df.gateway.connections` | counter | | `decision` (`allowed`, `denied`), `reason` (deny reason or `none`) | proxy requests by final decision, counted when they end |
| `df.gateway.active_connections` | up-down counter | | | requests connected to their destination |
| `df.gateway.bytes` | counter | By | `direction` (`upstream`: sandbox to destination, `downstream`) | relayed bytes |
| `df.gateway.policy_version` | gauge | | | version of the last applied allowlist snapshot |
| `df.gateway.inference.requests` | counter | | `wire_api` (`anthropic-messages`, `openai-chat`, `openai-responses`), `status_class` (`2xx`, `4xx`, `5xx`, `error`) | model requests of the inference endpoint, counted when their response ends |
| `df.gateway.inference.duration` | histogram | s | `wire_api` | time from the request to the end of its response |
| `df.gateway.inference.usage_dropped` | counter | | | usage records dropped because the buffer was full or the control plane refused them |

## Spans

Only with `OTEL_TRACES_EXPORTER=otlp`. Attributes never contain credentials, prompts or tool input/output.

| Span | Attributes |
|------|------------|
| `http.request` | `http.request.method`, `http.route`, `http.response.status_code` (UI and gateway health listeners; the `/metrics` scrape endpoint gets no span) |
| `agent.session` | `run_id` |
| `agent.persist_batch` | `messages` (count) |
| `gateway.connect` | `method`, `decision`, `host` (allowed requests only) |

Spans of the runtime and trace context propagation over the agent channel are not part of this.

## How to enable tracing

1. Run an OTLP/gRPC collector that Jaeger, Tempo or similar reads from.
2. Set the endpoint and enable traces, in Helm:

   ```yaml
   telemetry:
     traces:
       enabled: true
       sampler: parentbased_traceidratio
       samplerArg: '0.1'
     otlp:
       endpoint: https://otel-collector.observability.svc:4317
   ```

   or as variables: `OTEL_TRACES_EXPORTER=otlp`, `OTEL_EXPORTER_OTLP_ENDPOINT=...`.
3. Look for `service.name` `dark-factory-control-plane` and `dark-factory-gateway`.

Spans pass the same `RUST_LOG` filter as log events: `RUST_LOG=info` exports the INFO spans listed above.
