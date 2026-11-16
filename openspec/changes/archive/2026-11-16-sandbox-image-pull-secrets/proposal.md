# Proposal

## Why

Sandbox Pods carry no `imagePullSecrets`. A project image, runtime image or harness image in a private registry cannot be pulled, so every run fails with `ImagePullBackOff`. `deploy/README.md` documents this gap; operators must publish all three images anonymously today.

## What Changes

- New Helm value `sandboxNamespace.imagePullSecrets` (default `[]`): references to existing `kubernetes.io/dockerconfigjson` Secrets in the sandbox namespace. The chart never renders registry credentials; the operator creates the Secrets.
- New instance configuration key `sandbox.imagePullSecrets` (list of Secret names, default empty). The control plane rejects a name that is not a valid DNS-1123 subdomain at startup.
- Every sandbox Pod lists these Secrets in `spec.imagePullSecrets`. The pod-level list covers the project image, the runtime image volume and the harness image volume. An empty list leaves the Pod spec unchanged.
- `sandboxNamespace.imagePullSecrets` is independent of `image.pullSecrets`: no fallback in either direction.
- The sandbox namespace may now hold the operator's image pull secrets; it still holds no other Secrets. The control plane gets no `secrets` permission in the sandbox namespace.
- Docs: setup how-to for private sandbox images, reference entries, updated security overview.

## Out of scope

- Per-project image pull secrets configured in the UI, merged after the instance-wide list (instance first, then project, deduplicated; never replacing the instance list). Target: iteration 5 "Configurable projects" in `product/iterations/roadmap.md`.
- Chart-rendered registry credentials (`dockerconfigjson` built from values): rejected.
- Copying Secrets from the instance namespace into the sandbox namespace: rejected.
- Checking at startup or provisioning that the referenced Secrets exist: rejected (a missing Secret surfaces as `ImagePullBackOff` like any failed sandbox start).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `sandbox-provisioning`: sandbox Pods carry the configured image pull secrets.
- `helm-deployment`: new value `sandboxNamespace.imagePullSecrets`; the "Sandbox namespace" requirement allows image pull secrets; RBAC grants the control plane no `secrets` access in the sandbox namespace.

## Impact

- `crates/df-core/src/config.rs`: `SandboxProvisioningConfig.image_pull_secrets`, name validation.
- `crates/df-k8s/src/render.rs`: `render_pod` sets `image_pull_secrets`.
- `deploy/helm/dark-factory`: `values.yaml`, `values.schema.json`, `_helpers.tpl` (`dark-factory.sandboxConfig`), `README.md`, helm-unittest suites.
- Docs: `deploy/README.md`, `docs/content/security/_index.md`, new how-to under `docs/content/operations/`; `design/deployment.md` (sandbox namespace contents).
- `product/iterations/roadmap.md`: iteration 5 entry for per-project pull secrets.
- No database migration, no API change, no new dependency.
