# Spec Delta

## MODIFIED Requirements

### Requirement: Instance network policies
The chart SHALL restrict the instance namespace: control plane ingress only from the sandbox namespace (agent port), gateway pods (internal port) and the ingress controller (UI); gateway ingress only from the sandbox namespace (proxy and Git ports) and control plane pods (internal port). No rule SHALL allow the health ports; the kubelet reaches them as node traffic.

#### Scenario: Sandbox cannot reach the UI port
- **WHEN** a sandbox pod connects to the control plane's UI port
- **THEN** the connection is dropped

#### Scenario: Sandbox reaches the Git port
- **WHEN** a sandbox pod connects to the gateway's Git port
- **THEN** the connection is accepted

#### Scenario: Health ports closed to pods
- **WHEN** the chart is rendered with defaults, and again with `networkPolicy.ingressController.allowAll: true`
- **THEN** every ingress rule of both NetworkPolicies has a `ports` list, no rule of the control plane policy names `controlPlane.ports.health`, and no rule of the gateway policy names `gateway.ports.health`

## ADDED Requirements

### Requirement: Probe wiring
The chart SHALL offer `controlPlane.ports.health` (default `8081`) and `gateway.ports.health` (default `8080`), render them as `server.healthPort` and `gateway.healthPort` of the instance configuration, and point the liveness (`/healthz`) and readiness (`/readyz`) probes of both Deployments at them. No Service and no HTTPRoute SHALL expose a health port.

#### Scenario: Control plane probes
- **WHEN** the chart is rendered with defaults
- **THEN** the control plane container declares port `health` 8081, and its liveness and readiness probes call `/healthz` and `/readyz` on port `health`

#### Scenario: Gateway probes
- **WHEN** the chart is rendered with defaults
- **THEN** the gateway container declares port `health` 8080, and its liveness and readiness probes call `/healthz` and `/readyz` on port `health`

#### Scenario: Health port not published
- **WHEN** the chart is rendered with `httpRoute.enabled: true`
- **THEN** the control plane and gateway Services have no port named `health`, and the HTTPRoute's backend port is `controlPlane.ports.http`

#### Scenario: Health port is required
- **WHEN** the chart is rendered with `controlPlane.ports.health: null`
- **THEN** rendering fails schema validation

### Requirement: Readiness probe timeout
The control plane's readiness probe SHALL set `timeoutSeconds` longer than the database ping timeout of `/readyz` (2 seconds), so that the kubelet sees the `503` of a slow database instead of cutting the call short.

#### Scenario: Probe timeout
- **WHEN** the chart is rendered with defaults
- **THEN** the control plane's readiness probe has `timeoutSeconds: 3`
