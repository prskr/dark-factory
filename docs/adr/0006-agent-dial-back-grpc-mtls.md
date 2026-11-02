# ADR-0006: Agent runtime dials back over gRPC with mandatory mTLS

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0010](0010-credentials-in-the-sandbox.md), [ADR-0027](0027-agent-protocol-delivery-and-typed-tool-calls.md)

## Context

The control plane and the runtime need a bidirectional, streaming channel. Options were the runtime dialing back to the control plane, or the control plane exec-ing into the pod and talking over stdin/stdout. Agent traffic should be isolatable with CiliumNetworkPolicies, and the control plane may run with several replicas.

## Decision

- The runtime dials the control plane's **dedicated agent port**; one bidirectional gRPC stream per sandbox.
- **mTLS is mandatory.** cert-manager's csi-driver issues a short-lived per-pod certificate (private CA Issuer) with a URI SAN containing namespace, pod name and pod UID. The control plane checks it against its informer cache: pod exists, UID matches, belongs to a factory Sandbox, peer IP equals pod IP.
- Sandbox pods run with `automountServiceAccountToken: false`.
- No warm pools (ADR-0022): every run cold-starts its Sandbox.

## Consequences

- Reconnect and resume by sequence number; works with multiple replicas and rolling updates.
- Network policy is trivial: sandbox egress to one port.
- An exfiltrated key is useless from another pod IP.
- The control plane needs a private CA and the csi-driver in the cluster.

## Alternatives considered

- **Exec/attach to stdin/stdout**: needs `pods/exec` RBAC (effectively a remote shell into every sandbox), routes all traffic through kube-apiserver, can't resume after a control plane restart.
- **Projected ServiceAccount tokens + TokenReview**: works, but mTLS was preferred; it also allows dropping the SA token from the pod entirely.
