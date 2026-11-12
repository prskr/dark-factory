# Spec Delta

## MODIFIED Requirements

### Requirement: One release is one instance
A release SHALL create the control plane Deployment, the gateway Deployment (`gateway.replicas`, default 1), their Services (incl. a headless Service for gateway pods), the instance config file and ServiceAccounts. Cluster-scoped objects SHALL be prefixed with the release name and only created where enabled.

#### Scenario: Two instances in one cluster
- **WHEN** two releases with different names and namespaces are installed
- **THEN** no object of one release conflicts with an object of the other

## ADDED Requirements

### Requirement: Gateway replicas
The chart SHALL run `gateway.replicas` gateway pods, default 1. The values schema SHALL accept any integer of at least 1 and reject smaller values.

#### Scenario: Default is one gateway
- **WHEN** the chart is rendered with default values
- **THEN** the gateway Deployment has `replicas: 1`

#### Scenario: Several gateways for high availability
- **WHEN** the chart is rendered with `gateway.replicas: 3`
- **THEN** the gateway Deployment has `replicas: 3`

#### Scenario: Zero replicas rejected
- **WHEN** the chart is rendered with `gateway.replicas: 0`
- **THEN** rendering fails schema validation

### Requirement: Gateway disruption budget
The chart SHALL render the gateway PodDisruptionBudget (`gateway.pdb.minAvailable`) only when `gateway.pdb.enabled` is true (default) and `gateway.replicas` is greater than 1.

#### Scenario: No budget for a single gateway
- **WHEN** the chart is rendered with default values
- **THEN** no PodDisruptionBudget exists for the gateway

#### Scenario: Budget for several gateways
- **WHEN** the chart is rendered with `gateway.replicas: 2`
- **THEN** a PodDisruptionBudget selects the gateway pods with `minAvailable` from `gateway.pdb.minAvailable`

#### Scenario: Budget disabled
- **WHEN** the chart is rendered with `gateway.replicas: 2` and `gateway.pdb.enabled: false`
- **THEN** no PodDisruptionBudget exists for the gateway
