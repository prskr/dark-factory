# Spec Delta

## ADDED Requirements

### Requirement: Git proxy reachable by name
Sandbox pods SHALL get a `hostAliases` entry mapping the gateway Service's DNS names to its ClusterIP, SHALL list those names in `NO_PROXY`, and the generated per-run policy SHALL allow egress to gateway pods on the Git port.

#### Scenario: Clone without DNS
- **WHEN** the runtime clones `https://df-gateway.df.svc:8443/git/DF-7.git`
- **THEN** the name resolves through `/etc/hosts`, the connection bypasses the egress proxy and the network policy allows it
