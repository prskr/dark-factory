# Spec Delta

## MODIFIED Requirements

### Requirement: Certificates
The chart SHALL create (or reference) a private CA Issuer usable by cert-manager csi-driver, and Certificates for the control plane (agent and internal ports) and the gateway, all issued by that CA. The gateway certificate SHALL include the gateway Service's DNS names used in Git proxy URLs.

#### Scenario: Gateway authenticates to the control plane
- **WHEN** a gateway pod connects to the control plane's internal port
- **THEN** both sides present certificates issued by the instance's private CA

#### Scenario: Git proxy name verified
- **WHEN** a sandbox connects to `df-gateway.df.svc:8443`
- **THEN** the gateway's certificate is valid for that name

### Requirement: Instance network policies
The chart SHALL restrict the instance namespace: control plane ingress only from the sandbox namespace (agent port), gateway pods (internal port) and the ingress controller (UI); gateway ingress only from the sandbox namespace (proxy and Git ports) and control plane pods (internal port).

#### Scenario: Sandbox cannot reach the UI port
- **WHEN** a sandbox pod connects to the control plane's UI port
- **THEN** the connection is dropped

#### Scenario: Sandbox reaches the Git port
- **WHEN** a sandbox pod connects to the gateway's Git port
- **THEN** the connection is accepted

## ADDED Requirements

### Requirement: Gateway Git settings
The chart SHALL expose the gateway's Git port (default 8443) on the gateway Service and mount an `emptyDir` with a configurable size limit for repository mirrors. The maximum push size SHALL be configurable.

#### Scenario: Default values
- **WHEN** the chart is rendered with default values
- **THEN** the gateway Service has a port `git` 8443 and the gateway pod mounts a mirror `emptyDir` with a size limit
