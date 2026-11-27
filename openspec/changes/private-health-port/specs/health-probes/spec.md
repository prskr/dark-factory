# Spec Delta

## Purpose

Liveness and readiness endpoints of the control plane and the gateway for the kubelet, what each one checks, and the private health port that keeps them out of reach of the ingress and of other pods.

## ADDED Requirements

### Requirement: Private health port
The control plane and the gateway SHALL each serve `GET /healthz` and `GET /readyz` over plain HTTP on a health port of their own, without login: `server.healthPort` (default `8081`) for the control plane, `gateway.healthPort` (default `8080`) for the gateway. The health port SHALL serve no other route.

#### Scenario: Control plane probes on the health port
- **WHEN** a client sends `GET /healthz` to the control plane's health port
- **THEN** the response is `200`

#### Scenario: No UI on the health port
- **WHEN** a client sends `GET /tasks` or `POST /webhooks/forgejo` to the control plane's health port
- **THEN** the response is `404`

### Requirement: No probes on the UI port
The control plane's UI port SHALL NOT serve `/healthz` or `/readyz`, with or without `auth` in the instance configuration. The other routes without login (static assets, `/lifecycle-events/public-key`, `/login`, `/auth/callback`, `/logout`, `/webhooks/{plugin}`) SHALL stay on the UI port.

#### Scenario: Probe paths through the ingress
- **WHEN** a client sends `GET /readyz` to the control plane's UI port
- **THEN** the response is `404` and the control plane does not ping the database

#### Scenario: Instance without auth
- **WHEN** the control plane runs without `auth` and a client sends `GET /healthz` to the UI port
- **THEN** the response is `404`, and `GET /assets/…` and `POST /webhooks/{plugin}` still answer as before

### Requirement: Liveness
`/healthz` SHALL answer `200` whenever the process serves the health port. It SHALL check no dependency, so that an outage of the database or of the control plane never restarts a pod.

#### Scenario: Database down
- **WHEN** the database is unreachable and the kubelet calls the control plane's `/healthz`
- **THEN** the response is `200`

### Requirement: Control plane readiness
The control plane's `/readyz` SHALL ping the database on every call and answer `200` when the ping succeeds within 2 seconds, `503` otherwise. It SHALL NOT cache the result.

#### Scenario: Database reachable
- **WHEN** the database answers the ping
- **THEN** `/readyz` answers `200`

#### Scenario: Database slow or down
- **WHEN** the database does not answer the ping within 2 seconds
- **THEN** `/readyz` answers `503`

### Requirement: Gateway readiness
The gateway's `/readyz` SHALL answer `503` until the gateway pod received its first policy snapshot and `200` afterwards (`egress-proxy`, "Fail closed").

#### Scenario: Before the first snapshot
- **WHEN** a gateway pod has no policy snapshot yet
- **THEN** its `/readyz` answers `503`

### Requirement: Health port starts after startup checks
The control plane SHALL start serving its health port only after its startup checks passed (TLS material, plugins, configuration), as it does for the UI port, so that a control plane that cannot start never reports live.

#### Scenario: Startup fails
- **WHEN** `serve` fails while loading the agent TLS material
- **THEN** the health port never answers and the process exits with an error
