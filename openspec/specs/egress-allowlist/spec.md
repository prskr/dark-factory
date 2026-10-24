# egress-allowlist Specification

## Purpose
Lets board members define where sandboxes may connect: external HTTP(S) hosts through the proxy and in-cluster services directly, at instance or project scope (`design/egress.md` §4; iteration 1 subset).

## Requirements

### Requirement: Allowlist entries
Board members SHALL list, add and remove entries at instance or project scope of kind `http-host` (hostname or `*.suffix`, port default 443, 80 only explicitly) or `cluster-service` (namespace, service, ports, optional `allowDns`). A run's effective allowlist SHALL be the union of instance entries and its project's entries.

#### Scenario: Instance entry for Claude
- **WHEN** a board member adds instance entry `api.anthropic.com`
- **THEN** every run of every project may connect to it

#### Scenario: Project entry
- **WHEN** entry `*.crates.io` is added to project `DF`
- **THEN** runs of `DF` may connect to `static.crates.io` and runs of other projects may not

### Requirement: Entry validation
Hostnames SHALL be valid DNS names or a single leading `*.` wildcard; IP literals and bare `*` SHALL be rejected. Cluster-service entries SHALL name an existing Service at save time, in a namespace the instance's `clusterTargets` allow (the control plane's RBAC); other namespaces SHALL be rejected without a cluster lookup.

#### Scenario: IP literal
- **WHEN** a board member enters `10.0.0.5` as `http-host`
- **THEN** the form shows a validation error

#### Scenario: Unknown Service
- **WHEN** a cluster-service entry names `nativelink/schedular` which doesn't exist
- **THEN** the form shows a validation error

#### Scenario: Namespace outside the cluster targets
- **WHEN** a cluster-service entry names `kube-system/kube-dns` and `kube-system` is not a cluster target namespace
- **THEN** the form shows a validation error

### Requirement: Changes are audited and announced
Every change SHALL write an audit entry and publish an `egress_policy` notification in the same transaction commit.

#### Scenario: Entry removed
- **WHEN** a board member removes `pypi.org` from project `DF`
- **THEN** an audit entry `egress_allowlist.delete` exists and an `egress_policy` notification is published

### Requirement: Desktop-only page
The allowlist pages SHALL NOT appear in the mobile navigation.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation doesn't show the egress pages
