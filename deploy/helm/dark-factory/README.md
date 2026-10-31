# Dark Factory Helm Chart (`deploy/helm/dark-factory`)

Deploys a single-tenant Dark Factory instance (`design/deployment.md`, ADR-0031), including:
- Control plane `Deployment` and `Service` (`8080` UI/health, `9443` agent mTLS gRPC, `9444` internal `PolicyWatch` mTLS gRPC)
- Egress gateway `Deployment` (`>= 2` replicas), `PodDisruptionBudget`, `Service` (`3128` proxy), and headless `Service` (`9444` internal `Tail` mTLS gRPC, discovered by the control plane for the live egress log)
- `migrate` pre-install/pre-upgrade hook `Job`
- Least-privilege Kubernetes RBAC (`design/deployment.md` §3)
- Dedicated sandbox namespace (`sandboxNamespace.create: true`) with PSA `restricted`, default-deny `NetworkPolicy`, `ResourceQuota`, `LimitRange`, and tokenless `ServiceAccount`
- Private CA behind a `ClusterIssuer` (or reference to an existing `ClusterIssuer`) and mTLS `Certificate`s for control plane and gateway; sandbox pods request their client certificates from the same issuer
- Instance namespace `NetworkPolicy` objects (`design/deployment.md` §5)

The chart requires Kubernetes 1.36 or newer (`kubeVersion`): the runtime and the harness are delivered as image volumes.

## Values Reference

Every key in `values.yaml` is documented below:

| Key | Type | Default | Description |
|---|---|---|---|
| `nameOverride` | `string` | `""` | Override the chart name used in resource labels and names. |
| `fullnameOverride` | `string` | `""` | Override the full resource name prefix (`<release>-dark-factory`). |
| `instanceName` | `string` | `"dark-factory"` | Logical Dark Factory instance identifier written to `/etc/dark-factory/config.yaml`. |
| `image.pullPolicy` | `string` | `"IfNotPresent"` | Image pull policy (`Always`, `IfNotPresent`, `Never`) for control plane, gateway, and migrate pods. |
| `image.pullSecrets` | `list` | `[]` | List of image pull secret references (`[{ name: "..." }]`). |
| `image.controlPlane.repository` | `string` | `"ghcr.io/prskr/dark-factory/control-plane"` | OCI repository for the control plane (`dark-factory`) image. |
| `image.controlPlane.tag` | `string` | `"0.1.0"` | Tag used for the control plane image when `digest` is empty. |
| `image.controlPlane.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding `image.controlPlane.tag`. |
| `image.runtime.repository` | `string` | `"ghcr.io/prskr/dark-factory/runtime"` | OCI repository for the `factory-agent` scratch runtime image that sandboxes mount (control plane config `sandbox.runtimeImage`). |
| `image.runtime.tag` | `string` | `"0.1.0"` | Tag used for the runtime image when `digest` is empty; empty falls back to the chart `appVersion`. |
| `image.runtime.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding `image.runtime.tag`. |
| `image.harness.claudeCode.repository` | `string` | `"ghcr.io/prskr/dark-factory/claude-harness"` | OCI repository of the Claude Code harness image (adapter, Node.js, CLI) that sandboxes mount at `/opt/harness/claude-code`. Rendered as `harness.claudeCode.image`. |
| `image.harness.claudeCode.tag` | `string` | `"0.95.0"` | Tag of the harness image when `digest` is empty (the adapter version). Required when `digest` is empty; the render fails otherwise. |
| `image.harness.claudeCode.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding `image.harness.claudeCode.tag`. |
| `image.testProject.repository` | `string` | `"ghcr.io/prskr/dark-factory/test-project"` | OCI repository for the `fake-acp-agent` test project image. Create its project with the adapter command `/opt/harness/bin/fake-acp-agent`, so that no harness image is mounted. |
| `image.testProject.tag` | `string` | `"0.1.0"` | Tag used for the test project image when `digest` is empty. |
| `image.testProject.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding `image.testProject.tag`. |
| `database.host` | `string` | `"postgres.default.svc"` | PostgreSQL server hostname. |
| `database.port` | `int` | `5432` | PostgreSQL server TCP port. |
| `database.database` | `string` | `"dark_factory"` | PostgreSQL database name. |
| `database.schema` | `string` | `"dark_factory"` | PostgreSQL schema owned by `database.owner.user`. |
| `database.maxConnections` | `int` | `16` | Maximum connection pool size for the control plane `serve` process. |
| `database.app.user` | `string` | `"dark_factory_app"` | Least-privilege PostgreSQL role used by `dark-factory serve`. |
| `database.app.existingSecret` | `string` | `"dark-factory-db-app"` | Name of the existing Secret in the instance namespace containing the application role password. |
| `database.app.passwordKey` | `string` | `"password"` | Key within `database.app.existingSecret` holding the application role password. |
| `database.owner.user` | `string` | `"dark_factory_owner"` | PostgreSQL owner role used exclusively by the `dark-factory migrate` hook Job. |
| `database.owner.existingSecret` | `string` | `"dark-factory-db-owner"` | Name of the existing Secret in the instance namespace containing the owner role password. |
| `database.owner.passwordKey` | `string` | `"password"` | Key within `database.owner.existingSecret` holding the owner role password. |
| `database.migrate.resources.requests.cpu` | `string` | `"50m"` | Migrate Job CPU request. |
| `database.migrate.resources.requests.memory` | `string` | `"64Mi"` | Migrate Job memory request. |
| `database.migrate.resources.limits.cpu` | `string` | `"500m"` | Migrate Job CPU limit. |
| `database.migrate.resources.limits.memory` | `string` | `"256Mi"` | Migrate Job memory limit. |
| `database.migrate.manageAppRole` | `bool` | `true` | When `true`, `dark-factory migrate` mounts `database.app.existingSecret` and creates/rotates `database.app.user`. |
| `oidc.issuerUrl` | `string` | `"https://id.example.com/realms/dark-factory"` | OIDC provider issuer URL for web UI authentication. |
| `oidc.clientId` | `string` | `"dark-factory"` | OIDC relying-party client ID. |
| `oidc.existingSecret` | `string` | `"dark-factory-oidc"` | Name of the Secret in the instance namespace containing the OIDC client secret. |
| `oidc.clientSecretKey` | `string` | `"client-secret"` | Key within `oidc.existingSecret` holding the OIDC client secret. |
| `oidc.publicUrl` | `string` | `""` | Externally visible base URL of the UI. The OIDC redirect URI is `<publicUrl>/auth/callback`. Empty renders no `auth` section: `dark-factory serve` then logs a warning and mounts only `/healthz`, `/readyz` and `/assets/*`; the web UI is unavailable. See `docs/keycloak.md`. |
| `oidc.rolesClaim` | `string` | `"roles"` | Token claim with the flat role list; the UI requires the role `board-member`. |
| `oidc.extraScopes` | `list` | `[]` | Scopes requested besides `openid` and `profile`, e.g. `[groups, offline_access]` for Dex (`auth.extraScopes` of the instance config). |
| `logging.format` | `text`/`json` | `text` | `DF_LOG_FORMAT` of the control plane, gateway and migrate job (`json`: one JSON object per line). The control plane sets the same format in every sandbox. Other values fail schema validation. |
| `telemetry.metrics.exporters` | `list` | `[prometheus]` | `OTEL_METRICS_EXPORTER` of the control plane and gateway: `prometheus` (scrape endpoint on `telemetry.metrics.port`), `otlp` (push), both, or `[]` (no metrics). `otlp` needs `telemetry.otlp.endpoint`. |
| `telemetry.metrics.port` | `int` | `9464` | Port of `GET /metrics` (`OTEL_EXPORTER_PROMETHEUS_PORT`); exposed as container and Service port `metrics` when `prometheus` is an exporter. |
| `telemetry.metrics.serviceMonitor.enabled` | `bool` | `false` | Render one prometheus-operator `ServiceMonitor` for the control plane and one for the gateway. Needs the CRDs and `prometheus` in `telemetry.metrics.exporters`. Fails the render while `networkPolicy.metricsScraper.namespaceSelector` is empty and the NetworkPolicy is on. |
| `telemetry.metrics.serviceMonitor.labels` | `object` | `{}` | Extra labels of the `ServiceMonitor`s, e.g. `release: prometheus` for kube-prometheus-stack. |
| `telemetry.metrics.serviceMonitor.interval` | `string` | `''` | Scrape interval, e.g. `30s`. Empty: the Prometheus default. |
| `telemetry.traces.enabled` | `bool` | `false` | Export spans of the control plane and gateway over OTLP (`OTEL_TRACES_EXPORTER=otlp`); otherwise `none`. Needs `telemetry.otlp.endpoint`. |
| `telemetry.traces.sampler` | `string` | `parentbased_always_on` | `OTEL_TRACES_SAMPLER`, only rendered with enabled traces. |
| `telemetry.traces.samplerArg` | `string` | `''` | `OTEL_TRACES_SAMPLER_ARG`, e.g. `0.1` with `traceidratio`. Empty: not set. |
| `telemetry.otlp.endpoint` | `string` | `''` | `OTEL_EXPORTER_OTLP_ENDPOINT`. A value without scheme uses `https`. Required with the `otlp` metrics exporter or enabled traces (schema validation fails otherwise). |
| `telemetry.otlp.protocol` | `grpc` | `grpc` | `OTEL_EXPORTER_OTLP_PROTOCOL`. Only gRPC is supported. |
| `telemetry.otlp.insecure` | `bool` | `false` | `OTEL_EXPORTER_OTLP_INSECURE`: connect without TLS. |
| `telemetry.otlp.headersSecret` | `string` | `''` | Name of an existing Secret in the release namespace whose key `headers` becomes `OTEL_EXPORTER_OTLP_HEADERS` (gRPC metadata, e.g. `authorization=Bearer ...`). |
| `sandbox.logLevel` | `string` | `info` | Rendered as `sandbox.logLevel` in the instance config; the control plane sets it as `RUST_LOG` of every sandbox. DEBUG adds agent message chunks. |
| `sandboxNamespace.create` | `bool` | `true` | Create the sandbox namespace, PSA labels, default-deny NetworkPolicy, ResourceQuota, LimitRange, and sandbox ServiceAccount. |
| `sandboxNamespace.name` | `string` | `"dark-factory-sandboxes"` | Name of the Kubernetes namespace where sandboxes and task PVCs run. |
| `sandboxNamespace.runtimeClassName` | `string` | `""` | Optional Kubernetes `RuntimeClass` for sandbox pods. Empty uses the cluster default runtime; set `"kata"` for microVM isolation. |
| `sandboxNamespace.storageClass` | `string` | `""` | StorageClass of task PVCs. Empty uses the cluster's default StorageClass. |
| `sandboxNamespace.pvcSize` | `string` | `"20Gi"` | Storage request of task PVCs. |
| `sandboxNamespace.issuerName` | `string` | `""` | cert-manager issuer for sandbox client certificates. Empty uses the chart's issuer (see `tls.ca`). Must be a `ClusterIssuer`. |
| `sandboxNamespace.issuerKind` | `string` | `""` | Kind of that issuer. Empty uses the chart's issuer kind. |
| `sandboxNamespace.issuerGroup` | `string` | `"cert-manager.io"` | API group of that issuer. |
| `sandboxNamespace.certificateDuration` | `string` | `"6h"` | Lifetime of sandbox client certificates. |
| `sandboxNamespace.certificateRenewBefore` | `string` | `"2h"` | Renewal window of sandbox client certificates. |
| `sandboxNamespace.clusterDomain` | `string` | `"cluster.local"` | Cluster DNS domain used for Service FQDN host aliases. |
| `sandboxNamespace.controlPlaneIp` | `string` | `""` | Static control plane ClusterIP; empty resolves the Service at startup. |
| `sandboxNamespace.gatewayIp` | `string` | `""` | Static gateway ClusterIP; empty resolves the Service at startup. |
| `sandboxNamespace.serviceAccountName` | `string` | `"dark-factory-sandbox"` | Name of the zero-privilege `ServiceAccount` (`automountServiceAccountToken: false`) in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.pods` | `string` | `"20"` | Hard pod count quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.requestsCpu` | `string` | `"16"` | Hard total CPU requests quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.requestsMemory` | `string` | `"32Gi"` | Hard total memory requests quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.limitsCpu` | `string` | `"32"` | Hard total CPU limits quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.limitsMemory` | `string` | `"64Gi"` | Hard total memory limits quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.requestsStorage` | `string` | `"200Gi"` | Hard total PVC storage requests quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.persistentVolumeClaims` | `string` | `"40"` | Hard PVC count quota in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultCpu` | `string` | `"2"` | Default container CPU limit in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultMemory` | `string` | `"4Gi"` | Default container memory limit in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultRequestCpu` | `string` | `"250m"` | Default container CPU request in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultRequestMemory` | `string` | `"512Mi"` | Default container memory request in the sandbox namespace. |
| `enforcement.mode` | `string` | `"kubernetes"` | Network policy enforcement backend (`"kubernetes"` for `networking.k8s.io/v1` or `"cilium"` to also grant `ciliumnetworkpolicies`). |
| `clusterTargets.anyNamespace` | `bool` | `false` | When `true`, creates a release-prefixed read-only `ClusterRole`/`ClusterRoleBinding` for `services` and `endpointslices`. |
| `clusterTargets.namespaces` | `list` | `[]` | List of cluster-target namespaces in which per-namespace read-only `Role`/`RoleBinding`s are created when `anyNamespace` is `false`. |
| `plugins.forgejo.image.repository` | `string` | forgejo: `"ghcr.io/prskr/dark-factory/plugin-forgejo"` | Plugin image. With an image, the control plane pod mounts it read-only as an image volume at `/plugins/<name>` and the instance config sets `plugins.<name>.path` to `/plugins/<name>/plugin.wasm`. Set `plugins.<name>.image: null` for an entry without an image. |
| `plugins.forgejo.image.tag` | `string` | `""` | Tag when `digest` is empty; empty falls back to the chart `appVersion`. |
| `plugins.forgejo.image.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding the tag. |
| `plugins.forgejo.path` | `string` | `""` | Component file in the control plane image. Required without an image (the values schema rejects an entry naming neither), ignored with one. |
| `plugins.forgejo.callTimeoutSeconds` | `int` | `30` | Upper bound of one plugin call (`plugins.<name>.callTimeoutSeconds` of the instance config). |
| `plugins.forgejo.memoryMib` | `int` | `64` | Linear memory limit of a plugin instance. |
| `plugins.forgejo.settings` | `map` | `{}` | String map handed to the plugin unchanged (Forgejo: `wip-prefixes`). |
| `plugins.<name>.hosts` | `list` | unset | Forge hosts the plugin serves: a host (`ghe.example.org`) or `{host, apiUrl}` with an `https://` API URL. Written to the instance config only when set; an entry without `hosts` is the default plugin and serves every other project. See [Plugins](#plugins) for rollback. |
| `plugins.<name>.authScheme` | `string` | unset (`token`) | `token` or `bearer`: how the control plane sends the forge credential (`Authorization: token …` / `Bearer …`). Written to the instance config only when set. |
| `runs.keepAliveSeconds` | `int` | `600` | Seconds a run stays up after a normal turn end before the control plane releases it (`runs.keepAliveSeconds` of the instance config). `0` releases right after the turn. |
| `forge.allowPlainHttp` | `bool` | `false` | Accept `http://` repository URLs: forge plugin calls, sandbox git (`DF_GIT_ALLOW_PLAIN_HTTP=true` in the sandbox pods) and the project form (`forge.allowPlainHttp` of the instance config). Off, only `https://` works. The forge token travels unencrypted, so use it for throwaway clusters only; the control plane logs a warning at startup. A change restarts the control plane. |
| `web.showHiddenBlocks` | `bool` | `false` | Show transcript content blocks marked `hidden` (instructions for the agent, such as the title request) in the task UI, muted and labeled (`web.showHiddenBlocks` of the instance config). For debugging prompts. Read at startup: a change restarts the control plane. |
| `controlPlane.replicas` | `int` | `1` | Number of control plane pod replicas. |
| `controlPlane.logLevel` | `string` | `info` | `RUST_LOG` (`EnvFilter` syntax, e.g. `info,df_web=debug`) of the control plane and the migrate job. |
| `controlPlane.serviceAccountName` | `string` | `""` | Optional override for the control plane `ServiceAccount` name. |
| `controlPlane.ports.http` | `int` | `8080` | Control plane UI and health HTTP port (`/healthz`, `/readyz`). |
| `controlPlane.ports.agent` | `int` | `9443` | Control plane agent mTLS gRPC port. |
| `controlPlane.ports.internalGrpc` | `int` | `9444` | Control plane internal gRPC port (`PolicyWatch`, mTLS). |
| `controlPlane.resources.requests.cpu` | `string` | `"50m"` | Control plane CPU request. |
| `controlPlane.resources.requests.memory` | `string` | `"128Mi"` | Control plane memory request. |
| `controlPlane.resources.limits.memory` | `string` | `"512Mi"` | Control plane memory limit. No CPU limit is set by default. |
| `gateway.replicas` | `int` | `2` | Number of egress gateway pod replicas (minimum `2`). |
| `gateway.serviceAccountName` | `string` | `""` | Optional override for the gateway `ServiceAccount` name (`automountServiceAccountToken: false`). |
| `gateway.pdb.minAvailable` | `int` | `1` | Minimum available gateway pods in the `PodDisruptionBudget`. |
| `gateway.ports.proxy` | `int` | `3128` | Egress HTTP/CONNECT proxy port for sandboxes. |
| `gateway.ports.internalGrpc` | `int` | `9444` | Gateway internal gRPC port (`Tail`, mTLS). |
| `gateway.ports.health` | `int` | `8080` | Gateway health HTTP port (`/healthz`, `/readyz`; ready after the first policy snapshot). |
| `gateway.bufferSize` | `int` | `10000` | Flow events of the live egress log kept in memory per gateway pod. |
| `gateway.bufferMaxAgeSeconds` | `int` | `3600` | Maximum age of the flow events in the gateway and control plane buffers. Minimum 1: the chart schema rejects 0 (outside the chart, the runtime raises 0 to 1 s). |
| `gateway.logLevel` | `string` | `info` | `RUST_LOG` of the gateway. |
| `gateway.blockedCidrs` | `list` | `[]` | Pod and Service CIDRs the proxy refuses as destinations, on top of the built-in private, loopback, link-local, CGNAT and unique-local ranges. |
| `gateway.resources.requests.cpu` | `string` | `"100m"` | Gateway CPU request. |
| `gateway.resources.requests.memory` | `string` | `"128Mi"` | Gateway memory request. |
| `gateway.resources.limits.memory` | `string` | `"512Mi"` | Gateway memory limit. No CPU limit is set by default. |
| `tls.ca.create` | `bool` | `true` | Create a private CA and a `ClusterIssuer` for it. Control plane, gateway and sandbox certificates are all issued by it, so they share one CA. See [TLS issuer](#tls-issuer). |
| `tls.ca.clusterResourceNamespace` | `string` | `"cert-manager"` | cert-manager's cluster resource namespace. The CA `Certificate` and its self-signed bootstrap `Issuer` are created here, because a CA `ClusterIssuer` reads its key pair from this namespace. |
| `tls.ca.existingIssuer.name` | `string` | `""` | Existing cert-manager issuer used when `tls.ca.create` is `false`. |
| `tls.ca.existingIssuer.kind` | `string` | `"ClusterIssuer"` | Kind of the existing issuer. Sandboxes need a `ClusterIssuer`; with `"Issuer"` you must also set `sandboxNamespace.issuerName` and `sandboxNamespace.issuerKind: ClusterIssuer` to a `ClusterIssuer` that trusts the same CA, otherwise the chart refuses to render. |
| `tls.duration` | `string` | `"2160h"` | Certificate lifetime for control plane and gateway mTLS certificates. |
| `tls.renewBefore` | `string` | `"360h"` | Renewal window before expiry for control plane and gateway mTLS certificates. |
| `httpRoute.enabled` | `bool` | `false` | Render a Gateway API `HTTPRoute` (`gateway.networking.k8s.io/v1`) that sends all paths to the control plane UI port. Needs the Gateway API CRDs and an existing `Gateway`. Set `oidc.publicUrl` to the route's URL and allow the Gateway in `networkPolicy.ingressController` (with Cilium: `allowAll: true`). |
| `httpRoute.parentRefs` | `array` | `[]` | `parentRefs` of the route, e.g. `[{name: public, namespace: gateway-system}]`. At least one is required when `httpRoute.enabled` is `true`. |
| `httpRoute.hostnames` | `array` | `[]` | Hostnames the route matches. Empty: every hostname of the Gateway listener. |
| `networkPolicy.enabled` | `bool` | `true` | Render instance namespace `NetworkPolicy` resources restricting control plane and gateway ingress. |
| `networkPolicy.ingressController.allowAll` | `bool` | `false` | Render the UI port rule without `from`, so every source can reach it; `namespaceSelector` and `podSelector` are then ignored. Needed when the ingress proxy's traffic carries no pod identity, e.g. Cilium Gateway API or Ingress, whose Envoy connects with the reserved identity `ingress`. The agent and internal gRPC ports stay restricted. The UI requires a login on every route except `/healthz`, `/readyz` and `/assets/*`. |
| `networkPolicy.ingressController.namespaceSelector` | `object` | `{"kubernetes.io/metadata.name": "ingress-nginx"}` | Namespace label selector allowed to reach the control plane UI port (`8080`). Point it at the Gateway's namespace when you use `httpRoute`. |
| `networkPolicy.ingressController.podSelector` | `object` | `{}` | Optional pod label selector within the ingress controller namespace allowed to reach the UI port. |
| `networkPolicy.metricsScraper.namespaceSelector` | `object` | `{}` | Namespace label selector allowed to reach the metrics port of the control plane and gateway. Empty: no rule, nobody can scrape. Selecting the sandbox namespace (`kubernetes.io/metadata.name: <sandboxNamespace.name>`) fails the render; only that label is checked, other labels the sandbox namespace shares with the selector are not (its default-deny egress still blocks sandboxes). |
| `networkPolicy.metricsScraper.podSelector` | `object` | `{}` | Optional pod label selector within that namespace. |

## Telemetry

The control plane and gateway export OpenTelemetry metrics (and optionally traces); the sandbox runtime does not. Instruments and attributes are listed in [`docs/telemetry.md`](../../../docs/telemetry.md).

### Scrape with kube-prometheus-stack

Metrics are served on port `9464` by default. Select the chart's `ServiceMonitor`s with the label your Prometheus uses and allow its namespace through the NetworkPolicy:

```yaml
telemetry:
  metrics:
    serviceMonitor:
      enabled: true
      labels:
        release: kube-prometheus-stack
networkPolicy:
  metricsScraper:
    namespaceSelector:
      kubernetes.io/metadata.name: monitoring
```

### Push to an OTel Collector

Push metrics and spans over gRPC OTLP; keep or drop the scrape endpoint with `exporters`:

```yaml
telemetry:
  metrics:
    exporters: [otlp]        # no metrics port is exposed
  traces:
    enabled: true
    sampler: parentbased_traceidratio
    samplerArg: '0.1'
  otlp:
    endpoint: https://otel-collector.observability.svc:4317
    headersSecret: otel-auth # Secret with key `headers`
```

## Plugins

Each plugin ships as its own image (`FROM scratch`, one file `/plugin.wasm`). The kubelet pulls it and the control plane pod mounts it read-only as an image volume at `/plugins/<name>`; the control plane never fetches components and plugins need no PersistentVolume. The default release configures `forgejo` with `plugin-forgejo:<appVersion>`. Config reference: `crates/df-plugins/docs/configuration.md`.

### Pin a plugin version

```yaml
plugins:
  forgejo:
    image:
      digest: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

### Upgrade a plugin

Set the new `plugins.forgejo.image.tag` (or `digest`) and run `helm upgrade`. The image reference is part of the pod template, so the control plane Deployment performs a rolling update; no checksum annotation and no storage are involved. A plugin that cannot be pulled keeps the new pod in `ContainerCreating` and the old pods keep serving.

### Cluster without image volumes

Clusters without image volume support cannot mount plugin images (no init-container fallback: a `scratch` image has no copy tool). Build a control plane image that contains the component and configure a path:

```dockerfile
FROM ghcr.io/prskr/dark-factory/control-plane:0.1.0
COPY plugin.wasm /custom/forgejo.wasm
```

```yaml
image:
  controlPlane:
    repository: registry.example.org/control-plane-with-plugin
plugins:
  forgejo:
    image: null
    path: /custom/forgejo.wasm
```

### Route forges to plugins

A plugin with `hosts` serves projects on those hosts; the entry without `hosts` serves all others. Give every entry `hosts`, so a project on a forge nobody configured gets a clear `config` error instead of reaching the wrong plugin:

```yaml
plugins:
  forgejo:
    hosts:
      - git.example.org
  github:
    image:
      repository: registry.example.org/plugin-github
      tag: 0.1.0
    hosts:
      - host: github.com
        apiUrl: https://api.github.com
    authScheme: bearer
```

See [Add a second forge](../../../crates/df-plugins/docs/configuration.md#add-a-second-forge) for the routing rules and the checks after a restart.

Rollback: control planes before `hosts`/`authScheme` refuse to start with either key in the instance config (`deny_unknown_fields`). Remove both from every plugin entry before rolling back to such a release.

### Troubleshooting

The control plane pod does not start and `kubectl describe pod` shows a failed pull of the image volume `plugin-<name>` in the pod's reason, message and events (retried with volume backoff, not reported as a container `ErrImagePull`): the plugin image or tag does not exist, the registry needs credentials (`image.pullSecrets` apply to plugin images) or the cluster has no image volume support.

A plugin whose component is missing or invalid is logged at `error` ("forge plugin not loaded", fields `plugin`, `path`, `error`) and stays in the routing table as unavailable: its projects get a `config` error naming the plugin. `serve` exits only if no forge plugin loaded (`no forge plugin loaded: <every load error>`). Configuration errors (a host claimed by two plugins, two plugins without `hosts`, an `apiUrl` without `https://`) still stop startup. See [Startup](../../../crates/df-plugins/docs/configuration.md#startup).

## Cluster targets

`clusterTargets.anyNamespace` and `clusterTargets.namespaces` are also written to the control plane config (`clusterTargets.*`). The control plane watches `services` and `endpointslices` exactly in these namespaces (cluster-wide with `anyNamespace`) to keep run NetworkPolicies current, and verifies the matching permissions at startup. An egress allowlist rule that names a Service outside these namespaces cannot be resolved, which fails provisioning of the affected runs. The egress allowlist page applies the same restriction when a `cluster-service` entry is saved: Services outside these namespaces are rejected.

## TLS issuer

A CA `ClusterIssuer` reads its key pair `Secret` from cert-manager's cluster resource namespace (`tls.ca.clusterResourceNamespace`), not from the release namespace. With `tls.ca.create: true` the chart therefore creates:

| Object | Namespace | Name |
|---|---|---|
| `Issuer` (self-signed bootstrap) | `tls.ca.clusterResourceNamespace` | `<release-namespace>-<fullname>-selfsigned-issuer` |
| `Certificate` (CA, `isCA: true`) | `tls.ca.clusterResourceNamespace` | `<release-namespace>-<fullname>-ca` |
| `ClusterIssuer` | cluster-scoped | `<release-namespace>-<fullname>-ca-issuer` |

The release namespace is part of the names so that releases with the same name in different namespaces do not collide.

**Why the CA is not in the release namespace.** A CA `ClusterIssuer` can only read its `Secret` from cert-manager's cluster resource namespace (`--cluster-resource-namespace`, default `cert-manager`). Sandboxes need a `ClusterIssuer`: their CSI `CertificateRequest`s are created in the sandbox namespace, which must hold no `Secret`s, so a namespaced `Issuer` cannot serve them. Sandbox and control plane certificates must come from the same CA, so the CA lives in the cluster resource namespace. Setting `tls.ca.clusterResourceNamespace` to the release namespace only works if cert-manager runs with `--cluster-resource-namespace=<release namespace>`.

One CA signs everything. The control plane trusts sandbox client certificates through the `ca.crt` of its own certificate, so sandboxes must use the same CA. Sandbox certificates are requested by the cert-manager CSI driver from the sandbox namespace, where only a `ClusterIssuer` can be referenced; the sandbox namespace itself holds no `Secret`.

Installing the chart with `tls.ca.create: true` needs permission to create cert-manager `Issuer`s and `Certificate`s in `tls.ca.clusterResourceNamespace`. Without it, set `tls.ca.create: false` and point `tls.ca.existingIssuer` at a `ClusterIssuer` that already exists.

## PostgreSQL Role Setup (`owner` & `app`)

Dark Factory separates database privileges between two PostgreSQL roles (`design/deployment.md` §4, ADR-0012):

1. **Owner role (`database.owner.user`, default `dark_factory_owner`)** — used **only** by the `dark-factory migrate` pre-install/pre-upgrade Helm hook `Job`. Owns the database and schema (`database.schema`), runs DDL migrations, and grants object-level permissions to the application role.
2. **Application role (`database.app.user`, default `dark_factory_app`)** — used at runtime by `dark-factory serve`. Has DML (`SELECT, INSERT, UPDATE, DELETE`) on application tables, `USAGE, SELECT` on sequences, and append-only (`SELECT, INSERT` — `UPDATE, DELETE, TRUNCATE` revoked) on `audit_log`. Holds no DDL rights.

The chart supports two provisioning modes controlled by `database.migrate.manageAppRole`:

### Option A: Declarative / Pre-Provisioned Roles without `CREATEROLE` (`database.migrate.manageAppRole: false`)

Recommended when managing PostgreSQL declaratively (e.g., **Crossplane** `postgresql.sql.m.crossplane.io/v1alpha1` `Role` + `Grant`, **CloudNativePG** managed roles, Terraform, or a DBA) where granting `CREATEROLE` to an application migration user is undesirable.

1. **Provision both roles externally (`LOGIN`, no `CREATEROLE` needed)**:
   - `dark_factory_owner`: owner of the target database (`database.database`) or granted `CREATE` + `CONNECT` (with `WITH GRANT OPTION` on `CONNECT`) on the database so it can run `CREATE SCHEMA IF NOT EXISTS "<schema>"` and grant `CONNECT` / schema / table privileges to `dark_factory_app`.
   - `dark_factory_app`: standard `LOGIN` role with no special role attributes.
   - Equivalent SQL when provisioning manually as a superuser:
     ```sql
     CREATE ROLE dark_factory_owner LOGIN PASSWORD '<owner-password>';
     CREATE ROLE dark_factory_app   LOGIN PASSWORD '<app-password>';
     CREATE DATABASE dark_factory OWNER dark_factory_owner;

     \connect dark_factory
     CREATE SCHEMA IF NOT EXISTS dark_factory AUTHORIZATION dark_factory_owner;
     ```
2. **Create the two Kubernetes `Secret`s in the instance namespace**:
   - `database.owner.existingSecret` (default `dark-factory-db-owner`, key `password`) — mounted **only** into the `migrate` hook `Job`.
   - `database.app.existingSecret` (default `dark-factory-db-app`, key `password`) — mounted **only** into the `control-plane` `Deployment` (not mounted into the `migrate` `Job` when `manageAppRole: false`).
3. **Disable role management in Helm values**:
   ```yaml
   database:
     migrate:
       manageAppRole: false
   ```
   On every install/upgrade, `dark-factory migrate` connects as `database.owner.user`, creates the schema if missing, runs embedded `sqlx` migrations, detects that `database.app.user` exists in `pg_roles`, and automatically re-applies all schema/table/sequence `GRANT`s and `audit_log` `REVOKE`s (`crates/df-db/src/migrate.rs`).

### Option B: `migrate`-Managed Application Role (`database.migrate.manageAppRole: true`, default)

When `database.migrate.manageAppRole: true`, only `dark_factory_owner` needs to be pre-provisioned, but it **must** hold the `CREATEROLE` attribute:

```sql
CREATE ROLE dark_factory_owner LOGIN PASSWORD '<owner-password>' CREATEROLE;
CREATE DATABASE dark_factory OWNER dark_factory_owner;
```

In this mode, the `migrate` hook `Job` mounts both `database.owner.existingSecret` and `database.app.existingSecret`, executes `CREATE ROLE "<app_role>" LOGIN PASSWORD ...` (or `ALTER ROLE ... PASSWORD ...` to rotate passwords), and then applies all schema/table grants.
