# ADR-0031: Helm chart as deployment packaging

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0013](0013-single-tenant-instances.md), [ADR-0024](0024-build-tooling-and-dev-loop.md), [ADR-0030](0030-kubernetes-only-with-sandbox-provider-seam.md)

## Context

An instance consists of two Deployments, a migrate Job, ServiceAccounts, Roles in two namespaces, the sandbox namespace with its baseline objects, cert-manager resources and network policies. Several instances may share a cluster (ADR-0013), so names, namespaces and cluster-scoped objects must be templated per instance. The homelab is managed with GitOps.

## Decision

- Dark Factory ships a Helm chart; one release is one instance.
- The chart contains the control plane and gateway Deployments, Services, the `migrate` Job as a pre-install/pre-upgrade hook, ServiceAccounts, Roles/RoleBindings in the instance and sandbox namespaces, the sandbox namespace itself (optional — or a reference to an existing one) with Pod Security labels, default-deny policy and quotas, cert-manager Issuer/Certificates, and the instance's network policies.
- Cluster-scoped objects are only created where enabled and are prefixed with the release name.
- Prerequisites stay outside the chart: agent-sandbox, cert-manager and csi-driver, the CNI, PostgreSQL, the OIDC provider.
- The dev loop uses the same chart (ADR-0024).

## Consequences

- Multiple instances per cluster are a values file each; works with GitOps tools that render Helm.
- The RBAC the control plane needs is reviewable in one place, and the control plane never needs permission to create namespaces.
- Contents and permission list: `design/deployment.md`.

## Alternatives considered

- **Plain manifests / Kustomize**: per-instance naming and optional parts (Cilium mode, cluster-target namespaces, namespace creation) are clumsy without templating.
- **An operator managing instances**: more machinery than a handful of instances need.
- **Control plane creating its sandbox namespace and RBAC**: would require cluster-wide permissions.
