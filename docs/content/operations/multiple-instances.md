---
title: Several instances in one cluster
weight: 4
---

Two or more Dark Factory instances can run in one cluster, for example a production and a dev
instance. Each instance is its own Helm release in its own namespace. The release name may be the
same.

## What each instance needs of its own

| Item | Value | Why |
|---|---|---|
| Release namespace | `--namespace` | The chart names its cluster-scoped objects after it. |
| Sandbox namespace | `sandboxNamespace.name` | A shared sandbox namespace collides on the namespace and its policies, and each control plane would see the other's sandboxes. The chart cannot detect it. |
| Database | `database.host`, `database.database` | Each instance migrates and owns its schema. |
| URL and OIDC client | `publicUrl`, `oidc.*`, `httpRoute.hostnames` | Logins and redirects go to one instance. |

Cluster-scoped objects (RBAC for reading the sandbox namespace and cluster targets, the CA issuer)
and the reader roles in shared `clusterTargets.namespaces` carry the release namespace in their
names, so the releases do not collide on them. Names stay within 63 characters; the
[chart README](https://github.com/prskr/dark-factory/blob/main/deploy/helm/dark-factory/README.md#several-instances-in-one-cluster)
lists them.

## Example: production and dev

`values-dev.yaml`, next to the production values:

```yaml
database:
  host: postgres.databases.svc
  database: dark_factory_dev

publicUrl: https://factory-dev.example.org

oidc:
  issuerUrl: https://id.example.org/realms/dark-factory-dev

sandboxNamespace:
  name: dark-factory-dev-sandboxes

httpRoute:
  enabled: true
  parentRefs:
    - name: public
      namespace: gateway-system
  hostnames:
    - factory-dev.example.org
```

```bash
helm install dark-factory oci://ghcr.io/prskr/dark-factory/charts/dark-factory \
  --namespace dark-factory-dev --create-namespace \
  --version 2026.11.1 \
  --values values-dev.yaml
```

Check that the cluster-scoped roles of both instances exist side by side:

```bash
kubectl get clusterroles -l app.kubernetes.io/name=dark-factory
```
