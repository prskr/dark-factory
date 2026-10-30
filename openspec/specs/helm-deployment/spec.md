# helm-deployment Specification

## Purpose

Defines what one Helm release creates for a Dark Factory instance and the exact permissions each component receives, following `design/deployment.md` and ADR-0031.

## Requirements

### Requirement: One release is one instance
A release SHALL create the control plane Deployment, the gateway Deployment (≥ 2 replicas with a PodDisruptionBudget), their Services (incl. a headless Service for gateway pods), the instance config file and ServiceAccounts. Cluster-scoped objects SHALL be prefixed with the release name and only created where enabled.

#### Scenario: Two instances in one cluster
- **WHEN** two releases with different names and namespaces are installed
- **THEN** no object of one release conflicts with an object of the other

### Requirement: Migrations before rollout
The chart SHALL run `dark-factory migrate` as a pre-install/pre-upgrade hook Job with the owner credentials Secret and the optional application password Secret.

#### Scenario: Upgrade with a new migration
- **WHEN** a release is upgraded
- **THEN** the migrate Job completes before the control plane pods are replaced

### Requirement: Sandbox namespace
With `sandboxNamespace.create: true` the chart SHALL create the sandbox namespace with Pod Security Admission `restricted`, a default-deny ingress and egress NetworkPolicy, a ResourceQuota and a LimitRange; otherwise it SHALL reference an existing namespace. The sandbox namespace SHALL contain no Secrets.

#### Scenario: Default-deny baseline
- **WHEN** a pod without a run policy starts in the sandbox namespace
- **THEN** it can neither send nor receive any traffic

### Requirement: Least-privilege RBAC
The chart SHALL grant exactly the permissions in `design/deployment.md` §3: the control plane's write access only in the sandbox namespace, Secret read and Leases in the instance namespace, Service/EndpointSlice read in cluster-target namespaces; no Kubernetes API access for the gateway and sandboxes. `pods/exec`, `pods/attach` and namespace create/delete SHALL never be granted.

#### Scenario: No exec rights
- **WHEN** `kubectl auth can-i create pods/exec --as=<control plane SA> -n <sandbox ns>` is evaluated
- **THEN** the answer is `no`

#### Scenario: Gateway has no API token
- **WHEN** a gateway pod is inspected
- **THEN** `automountServiceAccountToken` is `false`

### Requirement: Certificates
The chart SHALL create (or reference) a private CA Issuer usable by cert-manager csi-driver, and Certificates for the control plane (agent and internal ports) and the gateway, all issued by that CA.

#### Scenario: Gateway authenticates to the control plane
- **WHEN** a gateway pod connects to the control plane's internal port
- **THEN** both sides present certificates issued by the instance's private CA

### Requirement: Instance network policies
The chart SHALL restrict the instance namespace: control plane ingress only from the sandbox namespace (agent port), gateway pods (internal port) and the ingress controller (UI); gateway ingress only from the sandbox namespace (proxy port) and control plane pods (internal port).

#### Scenario: Sandbox cannot reach the UI port
- **WHEN** a sandbox pod connects to the control plane's UI port
- **THEN** the connection is dropped

### Requirement: Logging values
The chart SHALL expose `logging.format` (`text` or `json`, default `text`) and the log levels `controlPlane.logLevel`, `gateway.logLevel` and `sandbox.logLevel` (default `info` each). The values schema SHALL reject any other `logging.format`.

#### Scenario: Invalid format rejected
- **WHEN** the chart is rendered with `logging.format: logfmt`
- **THEN** rendering fails schema validation

### Requirement: Logging wiring
The chart SHALL set `DF_LOG_FORMAT` from `logging.format` on the control plane, gateway and migrate containers, `RUST_LOG` from `controlPlane.logLevel` on the control plane and migrate containers, `RUST_LOG` from `gateway.logLevel` on the gateway container, and `sandbox.logLevel` in the instance config.

#### Scenario: JSON logging for the whole instance
- **WHEN** the chart is rendered with `logging.format: json`
- **THEN** the control plane, gateway and migrate containers have `DF_LOG_FORMAT=json`

#### Scenario: Gateway at debug
- **WHEN** the chart is rendered with `gateway.logLevel: debug`
- **THEN** the gateway container has `RUST_LOG=debug` and the control plane container has `RUST_LOG=info`

#### Scenario: Sandbox level in instance config
- **WHEN** the chart is rendered with `sandbox.logLevel: debug`
- **THEN** the instance config contains `sandbox.logLevel: debug`

### Requirement: Telemetry values
The chart SHALL expose `telemetry.metrics.exporters` (list of `prometheus` and `otlp`, default `[prometheus]`; empty disables metrics), `telemetry.metrics.port` (default `9464`), `telemetry.traces.enabled` (default `false`), `telemetry.traces.sampler`, `telemetry.traces.samplerArg`, and `telemetry.otlp.endpoint`, `.protocol` (only `grpc`), `.insecure` and `.headersSecret`. The values schema SHALL reject any other `telemetry.otlp.protocol`.

#### Scenario: Unsupported OTLP protocol
- **WHEN** the chart is rendered with `telemetry.otlp.protocol: http/protobuf`
- **THEN** rendering fails schema validation

### Requirement: OTLP endpoint required
When no `telemetry.otlp.endpoint` is set, the values schema SHALL reject selecting the `otlp` exporter and enabling traces; with an endpoint both are valid.

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

### Requirement: Harness image values
The chart SHALL offer `image.harness.claudeCode.repository`, `.tag` and `.digest` (digest wins over tag, an empty tag falls back to the chart `appVersion`) and render the resulting reference as `harness.claudeCode.image` of the instance configuration.

#### Scenario: Tag
- **WHEN** the repository is `registry.example.org/claude-harness` and the tag `0.95.0`
- **THEN** the instance configuration has `image: "registry.example.org/claude-harness:0.95.0"`

#### Scenario: Digest
- **WHEN** a digest `sha256:…` is set
- **THEN** the reference is `registry.example.org/claude-harness@sha256:…`

### Requirement: Image volumes and Kubernetes version
The chart SHALL declare `kubeVersion: ">=1.36.0-0"` and SHALL NOT offer a runtime delivery setting; the runtime and harness images are always delivered as image volumes.

#### Scenario: Older cluster
- **WHEN** the chart is installed on Kubernetes 1.35
- **THEN** Helm refuses the installation with the unsatisfied `kubeVersion`

#### Scenario: No delivery setting
- **WHEN** the chart is rendered
- **THEN** the instance configuration has no `runtimeDelivery` key and `values.yaml` has no `sandboxNamespace.runtimeDelivery`

### Requirement: Plugin images
For each entry under `plugins.<name>` with an image, the chart SHALL mount it as a read-only image volume at `/plugins/<name>` of the control plane pod and SHALL write `plugins.<name>.path: /plugins/<name>/plugin.wasm`, call timeout, memory limit, settings and, when set, `hosts` and `authScheme` into the instance config. An entry without an image SHALL require `path` and SHALL render only the config. The default values SHALL configure `forgejo` with its plugin image of the chart's app version.

#### Scenario: Default release
- **WHEN** the chart is rendered with default values
- **THEN** the control plane pod has an image volume with the Forgejo plugin image mounted read-only at `/plugins/forgejo`, and the instance config sets `plugins.forgejo.path` to `/plugins/forgejo/plugin.wasm`

#### Scenario: Pinned digest
- **WHEN** `plugins.forgejo.image.digest` is set
- **THEN** the image volume references the image by that digest

#### Scenario: Plugin baked into a custom image
- **WHEN** `plugins.forgejo` has no image and `path: /custom/forgejo.wasm`
- **THEN** no image volume is rendered and the instance config sets `plugins.forgejo.path` to `/custom/forgejo.wasm`

#### Scenario: Entry without image or path
- **WHEN** a plugin entry has neither an image nor a path
- **THEN** rendering fails with a message naming the plugin

#### Scenario: Routing values
- **WHEN** `plugins.github` sets `hosts: [{host: github.com, apiUrl: https://api.github.com}]` and `authScheme: bearer`
- **THEN** the instance config contains both values unchanged under `plugins.github`

#### Scenario: Default release has no hosts
- **WHEN** the chart is rendered with default values
- **THEN** `plugins.forgejo` in the instance config has no `hosts`, so it serves every project

### Requirement: Plugin names
Plugin names under `plugins` SHALL be lowercase DNS labels of at most 54 characters, so that the volume name `plugin-<name>` is a valid DNS label.

#### Scenario: Invalid plugin name
- **WHEN** a plugin entry is named `My_Plugin`
- **THEN** rendering fails with a values schema error naming the plugin

### Requirement: Plugin upgrades without storage
A changed plugin image reference SHALL roll the control plane pods. Plugins SHALL NOT require persistent volumes, and the control plane SHALL stay a Deployment with rolling updates.

#### Scenario: Plugin upgrade
- **WHEN** only the Forgejo plugin digest changes in an upgrade
- **THEN** the control plane Deployment performs a rolling update and no PersistentVolumeClaim is created
