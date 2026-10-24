## ADDED Requirements

### Requirement: Telemetry values
The chart SHALL expose `telemetry.metrics.exporters` (list of `prometheus` and `otlp`, default `[prometheus]`; empty disables metrics), `telemetry.metrics.port` (default `9464`), `telemetry.traces.enabled` (default `false`), `telemetry.traces.sampler`, `telemetry.traces.samplerArg`, and `telemetry.otlp.endpoint`, `.protocol` (only `grpc`), `.insecure` and `.headersSecret`. When no `telemetry.otlp.endpoint` is set, the values schema SHALL reject selecting the `otlp` exporter and enabling traces; with an endpoint both are valid.

#### Scenario: OTLP without endpoint
- **WHEN** the chart is rendered with `telemetry.traces.enabled: true` and no endpoint
- **THEN** rendering fails schema validation

### Requirement: Telemetry wiring
The chart SHALL render the telemetry values as the standard `OTEL_*` variables on the control plane and gateway containers. `telemetry.otlp.headersSecret` SHALL name an existing Secret whose `headers` key becomes `OTEL_EXPORTER_OTLP_HEADERS`.

#### Scenario: Defaults
- **WHEN** the chart is rendered with default values
- **THEN** control plane and gateway have `OTEL_METRICS_EXPORTER=prometheus`, `OTEL_TRACES_EXPORTER=none` and a `metrics` container port 9464

#### Scenario: OTLP with auth headers
- **WHEN** the chart is rendered with `telemetry.metrics.exporters: [otlp]`, an endpoint and `telemetry.otlp.headersSecret: otel-auth`
- **THEN** `OTEL_EXPORTER_OTLP_HEADERS` is read from Secret `otel-auth`, key `headers`, and no `metrics` port is exposed

### Requirement: Metrics scraping
With `prometheus` among the exporters the control plane and gateway Services SHALL expose a `metrics` port. With `telemetry.metrics.serviceMonitor.enabled: true` the chart SHALL create one `ServiceMonitor` per component with the configured labels and interval.

#### Scenario: ServiceMonitor
- **WHEN** the chart is rendered with `telemetry.metrics.serviceMonitor.enabled: true`, `labels: {release: prometheus}` and `networkPolicy.metricsScraper.namespaceSelector: {kubernetes.io/metadata.name: monitoring}`
- **THEN** a `ServiceMonitor` for control plane and gateway selects their `metrics` port and carries `release: prometheus`

### Requirement: Scraper network access
With network policies enabled the chart SHALL allow ingress to the metrics port only from `networkPolicy.metricsScraper` (namespace and optional pod selector); the sandbox namespace SHALL never be allowed to reach it.

#### Scenario: Scraper ingress
- **WHEN** the chart is rendered with `networkPolicy.metricsScraper.namespaceSelector: {kubernetes.io/metadata.name: monitoring}`
- **THEN** the control plane and gateway NetworkPolicies allow TCP 9464 from the `monitoring` namespace only

### Requirement: Scraper selector validation
A `networkPolicy.metricsScraper.namespaceSelector` that selects the sandbox namespace by `kubernetes.io/metadata.name` SHALL fail the render; other labels the sandbox namespace shares with the selector are not checked (its default-deny egress still applies). With network policies enabled and the `ServiceMonitor`s rendered, an empty `namespaceSelector` SHALL fail the render; with network policies disabled no selector is required.

#### Scenario: Scraper selector names the sandbox namespace
- **WHEN** the chart is rendered with `networkPolicy.metricsScraper.namespaceSelector` set to `kubernetes.io/metadata.name: <sandboxNamespace.name>`
- **THEN** rendering fails

#### Scenario: ServiceMonitor without scraper selector
- **WHEN** the chart is rendered with `telemetry.metrics.serviceMonitor.enabled: true`, `networkPolicy.enabled: true` and an empty `networkPolicy.metricsScraper.namespaceSelector`
- **THEN** rendering fails with an error naming `networkPolicy.metricsScraper.namespaceSelector`

#### Scenario: ServiceMonitor without NetworkPolicy
- **WHEN** the chart is rendered with `telemetry.metrics.serviceMonitor.enabled: true` and `networkPolicy.enabled: false`
- **THEN** both `ServiceMonitor`s render without a scraper selector
