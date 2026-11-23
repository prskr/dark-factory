# ADR-0064: Model calls of the control plane go through the gateway

- **Status:** Proposed (change `factory-minion`)
- **Date:** 2026-11-21
- **Related:** extends [ADR-0050](0050-model-traffic-and-usage-only-through-the-gateway.md); [ADR-0010](0010-credentials-in-the-sandbox.md), [ADR-0020](0020-configuration-in-database.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md)

## Context

The factory minion is an optional model call in the control plane: task titles, persona descriptions and, later, continuation summaries. ADR-0050 says all model traffic goes through the gateway's inference endpoint and that usage is measured only there. The inference endpoint identifies its caller by source pod IP from the sandbox registry. The control plane is not a sandbox and runs as several replicas, so it has no identity there.

The control plane already reads provider credentials (through `SecretStore`) to put them into the gateway's policy snapshot. It does not send them to any model provider. The gateway already trusts the control plane's mTLS client certificate on its internal API port (`GatewayLog.Tail`).

## Decision

- The control plane makes model calls only through the gateway, with the unary gRPC `Complete` of the service `Inference` on the gateway's internal mTLS port. The service has its own contract package `darkfactory.inference.v1`, separate from the gateway link package `darkfactory.gateway.v1`, because it is a model API for control-plane features and not link plumbing. Only the transport is shared. The caller identity is the control plane's DNS name in its client certificate, checked by the same interceptor as for `Tail`. The TLS layer alone is not enough, because the instance CA also signs sandbox certificates. Without a configured control-plane name the gateway does not serve `Complete` at all. The sandbox listener and its pod-IP identification are unchanged.
- The control plane never sends a model credential to a model provider and never puts one into a request. The gateway injects the credential of the minion's provider from the policy snapshot, as it does for runs. Reading credentials to build the snapshot stays the control plane's only use of them.
- The RPC is provider-neutral. A request carries the instructions (system prompt), one input text, the output token limit and the `caller` (minion job, project, task and persona) for usage attribution. The answer is the output text and whether it was cut at the limit. Errors are gRPC status codes, never provider statuses or bodies.
- The gateway owns everything provider-specific. The route comes from the snapshot (`Snapshot.minion`), built from an instance setting that names one model of a configured model provider: wire API, upstream URL, model, header templates, credential. For each call the gateway builds the request body of the route's wire API, renders the header templates (every run variable is `minion-<request ID>`, so a header built from a run variable, such as a session ID, differs per call; a header without a variable is fixed provider configuration and stays fixed, as for runs), injects the credential and reads the output text.
- This amends ADR-0050's "no protocol translation, no body rewriting" for one case: the gateway speaks the three wire APIs as the client of its own `Complete` calls. Sandbox traffic stays a pass-through.
- The control plane sends each call once. A resend after `UNAVAILABLE` could repeat a call that already reached the provider. Retries belong to the caller's outbox job, which accepts that a provider call can happen twice.
- The gateway measures usage of these calls like any other request and reports it with the minion job and its project, task and persona as caller instead of a run.
- The minion is off until a board member selects a model. Without it, the snapshot carries no minion route and no extra credential.

## Consequences

- ADR-0050 holds for credentials and usage without exceptions: one credential path, one usage channel, also for the control plane.
- The control plane knows no wire API, provider header or credential scheme. A new wire API or provider feature changes only the gateway.
- The gateway gets three small request builders and output-text readers, next to the usage parsers it already has for the same wire APIs.
- During a setting change, a gateway pod with the previous snapshot can make a complete call to the previous provider for a moment. No call mixes two providers, because the request carries nothing provider-specific.
- The minion provider's credential is in every gateway pod's memory while the minion is on, also when no run uses that provider.
- A gateway outage also stops the minion. Minion jobs are optional and fail without effect.
- The internal mTLS port now carries model traffic. Its access stays limited to the control plane certificate. A compromised control plane could already read every credential, so this adds no new risk.
- Minion calls are non-streaming and bounded (request 1 MiB, response 4 MiB). Streaming calls from the control plane would need a new decision.
- Further control-plane model uses (for example the PR summary on mark done) use the same RPC with a new job kind.

## Alternatives considered

- **A pass-through RPC (path, provider-specific body, headers rendered by the control plane)**: no request building in the gateway, but the control plane would need every wire API and the provider's header templates, while the gateway owns the route. It also needs a check that a call built for one provider never reaches another one during a setting change.
- **Direct provider calls from the control plane with the credential**: simplest code, but a second credential path and a second usage parser. This is what ADR-0050 rejected for harnesses.
- **The control plane on the sandbox listener with optional client certificates**: two identities on one listener that sandboxes can reach. A mistake in the identity choice would give a sandbox the minion route.
- **The control plane as a pseudo-sandbox identified by pod IP**: replicas and their IPs are not in the sandbox registry, and adding them mixes two lifecycles.
- **A separate minion deployment with its own pod identity**: one more component to deploy and scale for a few calls per task.
- **A dedicated, minion-only credential reference outside model providers**: duplicates the provider configuration (upstream URL, scheme, headers) and the snapshot rules.
