# Deployment — Reference

> What an instance needs in the cluster and in PostgreSQL, and what the Helm chart provides. This is the basis for the setup documentation. Decision records: [ADR-0012](adr/0012-postgresql-and-blob-store.md) (database roles, migrations), [ADR-0013](adr/0013-single-tenant-instances.md) (one instance per tenant), [ADR-0030](adr/0030-kubernetes-only-with-sandbox-provider-seam.md) (scoped RBAC, sandbox namespace), [ADR-0031](adr/0031-helm-chart-deployment-packaging.md) (Helm chart). Status: **design draft**.

## 1. Layout of one instance

| Namespace | Contains | Created by |
|---|---|---|
| Instance namespace (e.g. `dark-factory`) | Control plane Deployment, gateway Deployment, `migrate` Job, Services, ServiceAccounts, credential Secrets (incl. a commit signing key, once used — created and rotated outside Dark Factory), Leases | Helm release (or pre-existing) |
| Sandbox namespace (e.g. `dark-factory-sandboxes`) | Sandboxes, task PVCs, generated network policies | Helm chart (`sandboxNamespace.create: true`) **or** the operator beforehand — **never the control plane** |

- The sandbox namespace is configured per instance (`sandboxNamespace.name` in the chart → control plane setting). Several instances in one cluster each get their own pair of namespaces.
- At startup the control plane checks that the sandbox namespace exists and that it holds the permissions below (`SelfSubjectAccessReview`); otherwise it refuses to start with a message naming what's missing.
- Prerequisites the chart doesn't install: agent-sandbox (CRDs + controller, pinned version), cert-manager and csi-driver, a CNI that enforces `NetworkPolicy`, PostgreSQL, an OIDC provider.

## 2. Sandbox namespace (defined by the chart or the operator)
- Pod Security Admission labels: `restricted`.
- Default-deny `NetworkPolicy` for ingress and egress as a baseline; the per-run policies (`design/egress.md`) open what each run needs.
- `ResourceQuota` (storage, PVC count, pods, CPU/memory) and a `LimitRange`.
- No Secrets: harness credentials are delivered over the agent stream, never stored in this namespace.

## 3. Kubernetes RBAC

| Component | Where | Permissions |
|---|---|---|
| Control plane | Role in the **sandbox namespace** | agent-sandbox API group (`agents.x-k8s.io`, verify against the pinned release): Sandboxes create/get/list/watch/patch/delete · `persistentvolumeclaims` create/get/list/watch/delete · `networkpolicies` (`networking.k8s.io`) create/get/list/watch/update/patch/delete · `ciliumnetworkpolicies` (`cilium.io`, Cilium mode only) same · `pods` get/list/watch · `events` create |
| Control plane | Role in the **instance namespace** | `leases` (`coordination.k8s.io`) get/create/update · `secrets` get (credential references) · `endpointslices` (`discovery.k8s.io`) get/list/watch (gateway pod discovery) |
| Control plane | Role per **cluster-target namespace** (listed in values), or a read-only ClusterRole if `clusterTargets.anyNamespace: true` | `services`, `endpointslices` get/list/watch |
| Gateway | — | None; `automountServiceAccountToken: false`. Everything comes from the control plane (`PolicyWatch`). |
| Sandboxes | — | Dedicated ServiceAccount without permissions; `automountServiceAccountToken: false`. |

Never granted: `pods/exec`, `pods/attach`, `namespaces` create/delete, Secrets in the sandbox namespace, write access outside the two namespaces. Cluster-scoped objects (ClusterRoles/-Bindings, if used) are prefixed with the release name so several instances can coexist.

## 4. PostgreSQL roles

Two roles per instance, in a dedicated schema:

| Role | Used by | Rights |
|---|---|---|
| Owner (e.g. `dark_factory_owner`) | `dark-factory migrate` (Job) | Owns database and schema; runs DDL; manages grants; `CREATEROLE` only if `migrate` should manage the application role |
| Application (e.g. `dark_factory_app`) | Control plane `serve` | DML on application tables, `INSERT`/`SELECT` only on the audit log, no DDL |

### 4.1 One-time setup (DBA, as superuser)

```sql
-- Owner role, used only by `dark-factory migrate`.
-- Drop CREATEROLE if the application role is created manually (4.3).
CREATE ROLE dark_factory_owner LOGIN PASSWORD '<owner-password>' CREATEROLE;
CREATE DATABASE dark_factory OWNER dark_factory_owner;

\connect dark_factory
CREATE SCHEMA dark_factory AUTHORIZATION dark_factory_owner;
REVOKE ALL ON DATABASE dark_factory FROM PUBLIC;
```

### 4.2 What `dark-factory migrate` does (idempotent, every run)
1. Takes sqlx's advisory lock and applies the embedded migrations in schema `dark_factory`.
2. If application-role management is enabled and a password is provided (from a Secret, via file), creates the role if missing — or updates its password (rotation):
   ```sql
   CREATE ROLE dark_factory_app LOGIN PASSWORD '<app-password>';
   -- or, when it exists:
   ALTER ROLE dark_factory_app PASSWORD '<app-password>';
   ```
3. Applies grants in code, because role names are configurable and migrations are static SQL:
   ```sql
   GRANT CONNECT ON DATABASE dark_factory TO dark_factory_app;
   GRANT USAGE ON SCHEMA dark_factory TO dark_factory_app;
   GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA dark_factory TO dark_factory_app;
   GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA dark_factory TO dark_factory_app;
   REVOKE UPDATE, DELETE, TRUNCATE ON dark_factory.audit_log FROM dark_factory_app;
   ```
   (`ALTER DEFAULT PRIVILEGES` isn't needed: grants are re-applied after every migration run.)
4. Never logs passwords; fails if the application role exists but isn't manageable by the owner (e.g. created by a DBA) — in that case management must be disabled and only grants are applied.

### 4.3 Without role management
If `migrate` shouldn't have `CREATEROLE` (or roles are managed declaratively, e.g. CloudNativePG managed roles), the DBA creates the application role once:

```sql
CREATE ROLE dark_factory_app LOGIN PASSWORD '<app-password>';
```

`migrate` then only applies migrations and grants (4.2, steps 1 and 3).

## 5. Helm chart (ADR-0031)

One release = one instance. The chart contains:
- Control plane Deployment (`serve`), gateway Deployment (`gateway`, ≥ 2 replicas, PodDisruptionBudget), Services (incl. a headless Service for gateway pod discovery).
- `migrate` Job as a pre-install/pre-upgrade hook, with the owner credentials Secret and the optional application password Secret.
- ServiceAccounts, Roles and RoleBindings from §3; release-prefixed cluster-scoped objects only where enabled.
- The sandbox namespace with the objects from §2 (`sandboxNamespace.create`, default `true`), or a reference to an existing one.
- cert-manager `Issuer` for the private CA used by sandbox mTLS (or a reference to an existing (Cluster)Issuer), and the `Certificate`s for control plane and gateway.
- Network policies for the instance namespace: control plane ingress from the sandbox namespace (agent port), the gateway pods (`PolicyWatch`) and the ingress controller (UI); gateway ingress from the sandbox namespace (proxy port) and the control plane pods (internal API port for `Tail`).
- Values for the instance configuration that isn't in the database: database connection, OIDC, sandbox namespace, enforcement mode (`kubernetes` / `cilium`), cluster-target namespaces, image references.

The dev loop (ADR-0024) uses the same chart: the Bazel deploy target pushes images and runs `helm upgrade --install` with the new image digests and dev values.
