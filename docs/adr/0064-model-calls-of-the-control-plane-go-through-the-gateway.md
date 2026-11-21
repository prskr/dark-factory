# ADR-0064: Model calls of the control plane go through the gateway

- **Status:** Proposed (change `factory-minion`)
- **Date:** 2026-11-21
- **Related:** extends [ADR-0050](0050-model-traffic-and-usage-only-through-the-gateway.md); [ADR-0010](0010-credentials-in-the-sandbox.md), [ADR-0020](0020-configuration-in-database.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md)

## Context

The factory minion is an optional model call in the control plane: task titles, persona descriptions and, later, continuation summaries. ADR-0050 says all model traffic goes through the gateway's inference endpoint and that usage is measured only there. The inference endpoint identifies its caller by source pod IP from the sandbox registry. The control plane is not a sandbox and runs as several replicas, so it has no identity there.

The control plane already reads provider credentials (through `SecretStore`) to put them into the gateway's policy snapshot. It does not send them to any model provider. The gateway already trusts the control plane's mTLS client certificate on its internal API port (`GatewayLog.Tail`).

## Decision

- The control plane makes model calls only through the gateway, with the unary gRPC `GatewayInference.Complete` on the gateway's internal mTLS port. The control plane's client certificate is the caller identity. The sandbox listener and its pod-IP identification are unchanged.
- The control plane never sends a model credential to a model provider and never puts one into a request. The gateway injects the credential of the minion's provider from the policy snapshot, as it does for runs. Reading credentials to build the snapshot stays the control plane's only use of them.
- The route comes from the snapshot (`Snapshot.minion`), built from an instance setting that names one model of a configured model provider. Requests name only the path and body. The gateway accepts only the paths of that model's wire API.
- The gateway measures usage of these calls like any other request and reports it with the minion job and its project, task and persona as caller instead of a run.
- The minion is off until a board member selects a model. Without it, the snapshot carries no minion route and no extra credential.

## Consequences

- ADR-0050 holds without exceptions: one credential path, one usage channel, also for the control plane.
- The minion provider's credential is in every gateway pod's memory while the minion is on, also when no run uses that provider.
- A gateway outage also stops the minion. Minion jobs are optional and fail without effect.
- The internal mTLS port now carries model traffic. Its access stays limited to the control plane certificate. A compromised control plane could already read every credential, so this adds no new risk.
- Minion calls are non-streaming and bounded (request 1 MiB, response 4 MiB). Streaming calls from the control plane would need a new decision.
- Further control-plane model uses (for example the PR summary on mark done) use the same RPC with a new job kind.

## Alternatives considered

- **Direct provider calls from the control plane with the credential**: simplest code, but a second credential path and a second usage parser. This is what ADR-0050 rejected for harnesses.
- **The control plane on the sandbox listener with optional client certificates**: two identities on one listener that sandboxes can reach. A mistake in the identity choice would give a sandbox the minion route.
- **The control plane as a pseudo-sandbox identified by pod IP**: replicas and their IPs are not in the sandbox registry, and adding them mixes two lifecycles.
- **A separate minion deployment with its own pod identity**: one more component to deploy and scale for a few calls per task.
- **A dedicated, minion-only credential reference outside model providers**: duplicates the provider configuration (upstream URL, scheme, headers) and the snapshot rules.
