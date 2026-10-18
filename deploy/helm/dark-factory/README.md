# Dark Factory Helm Chart (`deploy/helm/dark-factory`)

Deploys a single-tenant Dark Factory instance (`design/deployment.md`, ADR-0031), including:
- Control plane `Deployment` and `Service` (`8080` UI/health, `9443` agent mTLS gRPC, `9444` internal `PolicyWatch` mTLS gRPC)
- Egress gateway `Deployment` (`>= 2` replicas), `PodDisruptionBudget`, `Service` (`3128` proxy), and headless `Service` (`9444` internal `Tail` mTLS gRPC)
- `migrate` pre-install/pre-upgrade hook `Job`
- Least-privilege Kubernetes RBAC (`design/deployment.md` §3)
- Dedicated sandbox namespace (`sandboxNamespace.create: true`) with PSA `restricted`, default-deny `NetworkPolicy`, `ResourceQuota`, `LimitRange`, and tokenless `ServiceAccount`
- Private CA `Issuer` (or reference to an existing issuer) and mTLS `Certificate`s for control plane and gateway
- Instance namespace `NetworkPolicy` objects (`design/deployment.md` §5)

## Values Reference

Every key in `values.yaml` is documented below:

| Key | Type | Default | Description |
|---|---|---|---|
| `nameOverride` | `string` | `""` | Override the chart name used in resource labels and names. |
| `fullnameOverride` | `string` | `""` | Override the full resource name prefix (`<release>-dark-factory`). |
| `instanceName` | `string` | `"dark-factory"` | Logical Dark Factory instance identifier written to `/etc/dark-factory/config.yaml`. |
| `image.pullPolicy` | `string` | `"IfNotPresent"` | Image pull policy (`Always`, `IfNotPresent`, `Never`) for control plane, gateway, and migrate pods. |
| `image.pullSecrets` | `list` | `[]` | List of image pull secret references (`[{ name: "..." }]`). |
| `image.controlPlane.repository` | `string` | `"harbor.home.icb4dc0.de/dark-factory/control-plane"` | OCI repository for the control plane (`dark-factory`) image. |
| `image.controlPlane.tag` | `string` | `"0.1.0"` | Tag used for the control plane image when `digest` is empty. |
| `image.controlPlane.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding `image.controlPlane.tag`. |
| `image.runtime.repository` | `string` | `"harbor.home.icb4dc0.de/dark-factory/runtime"` | OCI repository for the `factory-agent` scratch runtime image volume. |
| `image.runtime.tag` | `string` | `"0.1.0"` | Tag used for the runtime image when `digest` is empty. |
| `image.runtime.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding `image.runtime.tag`. |
| `image.testProject.repository` | `string` | `"harbor.home.icb4dc0.de/dark-factory/test-project"` | OCI repository for the `fake-acp-agent` test project image. |
| `image.testProject.tag` | `string` | `"0.1.0"` | Tag used for the test project image when `digest` is empty. |
| `image.testProject.digest` | `string` | `""` | Immutable SHA-256 digest (`sha256:...`) overriding `image.testProject.tag`. |
| `database.host` | `string` | `"postgres.default.svc"` | PostgreSQL server hostname. |
| `database.port` | `int` | `5432` | PostgreSQL server TCP port. |
| `database.database` | `string` | `"dark_factory"` | PostgreSQL database name. |
| `database.schema` | `string` | `"dark_factory"` | PostgreSQL schema owned by `database.owner.user`. |
| `database.maxConnections` | `int` | `16` | Maximum connection pool size for the control plane `serve` process. |
| `database.app.user` | `string` | `"dark_factory_app"` | Least-privilege PostgreSQL role used by `dark-factory serve`. |
| `database.app.existingSecret` | `string` | `"dark-factory-db-app"` | Name of the existing Secret in the instance namespace containing the application role password. |
| `database.app.passwordKey` | `string` | `"password"` | Key within `database.app.existingSecret` holding the application role password. |
| `database.owner.user` | `string` | `"dark_factory_owner"` | PostgreSQL owner role used exclusively by the `dark-factory migrate` hook Job. |
| `database.owner.existingSecret` | `string` | `"dark-factory-db-owner"` | Name of the existing Secret in the instance namespace containing the owner role password. |
| `database.owner.passwordKey` | `string` | `"password"` | Key within `database.owner.existingSecret` holding the owner role password. |
| `database.migrate.manageAppRole` | `bool` | `true` | When `true`, `dark-factory migrate` mounts `database.app.existingSecret` and creates/rotates `database.app.user`. |
| `oidc.issuerUrl` | `string` | `"https://id.example.com/realms/dark-factory"` | OIDC provider issuer URL for web UI authentication. |
| `oidc.clientId` | `string` | `"dark-factory"` | OIDC relying-party client ID. |
| `oidc.existingSecret` | `string` | `"dark-factory-oidc"` | Name of the Secret in the instance namespace containing the OIDC client secret. |
| `oidc.clientSecretKey` | `string` | `"client-secret"` | Key within `oidc.existingSecret` holding the OIDC client secret. |
| `sandboxNamespace.create` | `bool` | `true` | Create the sandbox namespace, PSA labels, default-deny NetworkPolicy, ResourceQuota, LimitRange, and sandbox ServiceAccount. |
| `sandboxNamespace.name` | `string` | `"dark-factory-sandboxes"` | Name of the Kubernetes namespace where sandboxes and task PVCs run. |
| `sandboxNamespace.runtimeClassName` | `string` | `""` | Optional Kubernetes `RuntimeClass` name for sandbox pods (e.g., `"kata"`). |
| `sandboxNamespace.serviceAccountName` | `string` | `"dark-factory-sandbox"` | Name of the zero-privilege `ServiceAccount` (`automountServiceAccountToken: false`) in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.pods` | `string` | `"20"` | Hard pod count quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.requestsCpu` | `string` | `"16"` | Hard total CPU requests quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.requestsMemory` | `string` | `"32Gi"` | Hard total memory requests quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.limitsCpu` | `string` | `"32"` | Hard total CPU limits quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.limitsMemory` | `string` | `"64Gi"` | Hard total memory limits quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.requestsStorage` | `string` | `"200Gi"` | Hard total PVC storage requests quota in the sandbox namespace. |
| `sandboxNamespace.resourceQuota.persistentVolumeClaims` | `string` | `"40"` | Hard PVC count quota in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultCpu` | `string` | `"2"` | Default container CPU limit in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultMemory` | `string` | `"4Gi"` | Default container memory limit in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultRequestCpu` | `string` | `"250m"` | Default container CPU request in the sandbox namespace. |
| `sandboxNamespace.limitRange.defaultRequestMemory` | `string` | `"512Mi"` | Default container memory request in the sandbox namespace. |
| `enforcement.mode` | `string` | `"kubernetes"` | Network policy enforcement backend (`"kubernetes"` for `networking.k8s.io/v1` or `"cilium"` to also grant `ciliumnetworkpolicies`). |
| `clusterTargets.anyNamespace` | `bool` | `false` | When `true`, creates a release-prefixed read-only `ClusterRole`/`ClusterRoleBinding` for `services` and `endpointslices`. |
| `clusterTargets.namespaces` | `list` | `[]` | List of cluster-target namespaces in which per-namespace read-only `Role`/`RoleBinding`s are created when `anyNamespace` is `false`. |
| `controlPlane.replicas` | `int` | `1` | Number of control plane pod replicas. |
| `controlPlane.serviceAccountName` | `string` | `""` | Optional override for the control plane `ServiceAccount` name. |
| `controlPlane.ports.http` | `int` | `8080` | Control plane UI and health HTTP port (`/healthz`, `/readyz`). |
| `controlPlane.ports.agent` | `int` | `9443` | Control plane agent mTLS gRPC port. |
| `controlPlane.ports.internalGrpc` | `int` | `9444` | Control plane internal gRPC port (`PolicyWatch`, mTLS). |
| `controlPlane.resources.requests.cpu` | `string` | `"100m"` | Control plane CPU request. |
| `controlPlane.resources.requests.memory` | `string` | `"128Mi"` | Control plane memory request. |
| `controlPlane.resources.limits.cpu` | `string` | `"1000m"` | Control plane CPU limit. |
| `controlPlane.resources.limits.memory` | `string` | `"512Mi"` | Control plane memory limit. |
| `gateway.replicas` | `int` | `2` | Number of egress gateway pod replicas (minimum `2`). |
| `gateway.serviceAccountName` | `string` | `""` | Optional override for the gateway `ServiceAccount` name (`automountServiceAccountToken: false`). |
| `gateway.pdb.minAvailable` | `int` | `1` | Minimum available gateway pods in the `PodDisruptionBudget`. |
| `gateway.ports.proxy` | `int` | `3128` | Egress HTTP/CONNECT proxy port for sandboxes. |
| `gateway.ports.internalGrpc` | `int` | `9444` | Gateway internal gRPC port (`Tail`, mTLS). |
| `gateway.ports.health` | `int` | `8080` | Gateway health HTTP port (`/healthz`, `/readyz`). |
| `gateway.resources.requests.cpu` | `string` | `"100m"` | Gateway CPU request. |
| `gateway.resources.requests.memory` | `string` | `"128Mi"` | Gateway memory request. |
| `gateway.resources.limits.cpu` | `string` | `"1000m"` | Gateway CPU limit. |
| `gateway.resources.limits.memory` | `string` | `"512Mi"` | Gateway memory limit. |
| `tls.ca.create` | `bool` | `true` | Create a self-signed bootstrap `Issuer`, private CA `Certificate`, and CA `Issuer` in the instance namespace. |
| `tls.ca.existingIssuer.name` | `string` | `""` | Existing cert-manager issuer name used when `tls.ca.create` is `false`. |
| `tls.ca.existingIssuer.kind` | `string` | `"Issuer"` | Existing cert-manager issuer kind (`"Issuer"` or `"ClusterIssuer"`). |
| `tls.duration` | `string` | `"2160h"` | Certificate lifetime for control plane and gateway mTLS certificates. |
| `tls.renewBefore` | `string` | `"360h"` | Renewal window before expiry for control plane and gateway mTLS certificates. |
| `networkPolicy.enabled` | `bool` | `true` | Render instance namespace `NetworkPolicy` resources restricting control plane and gateway ingress. |
| `networkPolicy.ingressController.namespaceSelector` | `object` | `{"kubernetes.io/metadata.name": "ingress-nginx"}` | Namespace label selector allowed to reach the control plane UI port (`8080`). |
| `networkPolicy.ingressController.podSelector` | `object` | `{}` | Optional pod label selector within the ingress controller namespace allowed to reach the UI port. |

## PostgreSQL Role Setup (`owner` & `app`)

Dark Factory separates database privileges between two PostgreSQL roles (`design/deployment.md` §4, ADR-0012):

1. **Owner role (`database.owner.user`, default `dark_factory_owner`)** — used **only** by the `dark-factory migrate` pre-install/pre-upgrade Helm hook `Job`. Owns the database and schema (`database.schema`), runs DDL migrations, and grants object-level permissions to the application role.
2. **Application role (`database.app.user`, default `dark_factory_app`)** — used at runtime by `dark-factory serve`. Has DML (`SELECT, INSERT, UPDATE, DELETE`) on application tables, `USAGE, SELECT` on sequences, and append-only (`SELECT, INSERT` — `UPDATE, DELETE, TRUNCATE` revoked) on `audit_log`. Holds no DDL rights.

The chart supports two provisioning modes controlled by `database.migrate.manageAppRole`:

### Option A: Declarative / Pre-Provisioned Roles without `CREATEROLE` (`database.migrate.manageAppRole: false`)

Recommended when managing PostgreSQL declaratively (e.g., **Crossplane** `postgresql.sql.m.crossplane.io/v1alpha1` `Role` + `Grant`, **CloudNativePG** managed roles, Terraform, or a DBA) where granting `CREATEROLE` to an application migration user is undesirable.

1. **Provision both roles externally (`LOGIN`, no `CREATEROLE` needed)**:
   - `dark_factory_owner`: owner of the target database (`database.database`) or granted `CREATE` + `CONNECT` (with `WITH GRANT OPTION` on `CONNECT`) on the database so it can run `CREATE SCHEMA IF NOT EXISTS "<schema>"` and grant `CONNECT` / schema / table privileges to `dark_factory_app`.
   - `dark_factory_app`: standard `LOGIN` role with no special role attributes.
   - Equivalent SQL when provisioning manually as a superuser:
     ```sql
     CREATE ROLE dark_factory_owner LOGIN PASSWORD '<owner-password>';
     CREATE ROLE dark_factory_app   LOGIN PASSWORD '<app-password>';
     CREATE DATABASE dark_factory OWNER dark_factory_owner;

     \connect dark_factory
     CREATE SCHEMA IF NOT EXISTS dark_factory AUTHORIZATION dark_factory_owner;
     ```
2. **Create the two Kubernetes `Secret`s in the instance namespace**:
   - `database.owner.existingSecret` (default `dark-factory-db-owner`, key `password`) — mounted **only** into the `migrate` hook `Job`.
   - `database.app.existingSecret` (default `dark-factory-db-app`, key `password`) — mounted **only** into the `control-plane` `Deployment` (not mounted into the `migrate` `Job` when `manageAppRole: false`).
3. **Disable role management in Helm values**:
   ```yaml
   database:
     migrate:
       manageAppRole: false
   ```
   On every install/upgrade, `dark-factory migrate` connects as `database.owner.user`, creates the schema if missing, runs embedded `sqlx` migrations, detects that `database.app.user` exists in `pg_roles`, and automatically re-applies all schema/table/sequence `GRANT`s and `audit_log` `REVOKE`s (`crates/df-db/src/migrate.rs`).

### Option B: `migrate`-Managed Application Role (`database.migrate.manageAppRole: true`, default)

When `database.migrate.manageAppRole: true`, only `dark_factory_owner` needs to be pre-provisioned, but it **must** hold the `CREATEROLE` attribute:

```sql
CREATE ROLE dark_factory_owner LOGIN PASSWORD '<owner-password>' CREATEROLE;
CREATE DATABASE dark_factory OWNER dark_factory_owner;
```

In this mode, the `migrate` hook `Job` mounts both `database.owner.existingSecret` and `database.app.existingSecret`, executes `CREATE ROLE "<app_role>" LOGIN PASSWORD ...` (or `ALTER ROLE ... PASSWORD ...` to rotate passwords), and then applies all schema/table grants.
