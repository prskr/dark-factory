# Proposal

## Why

Operators have to create a Kubernetes Secret for every forge plugin and reference it in the Helm values before forge webhooks work. Until they do, `POST /webhooks/<plugin>` answers `404`. Generating the secret in Helm does not work with ArgoCD: it renders with `helm template`, `lookup` returns nothing, and every sync would produce a new secret and break the webhooks registered in the forges.

## What Changes

- At startup the control plane reads the webhook secret of each loaded plugin with a `webhooks` export from a Kubernetes Secret in the instance namespace. If the Secret does not exist and the plugin uses the generated default, the control plane creates it with a random value (32 bytes, hex-encoded).
- The generated Secret is `<fullname>-webhook-<plugin>` with key `secret`, carries `app.kubernetes.io/*` labels plus `dark-factory.io/plugin`, has no owner, and survives `helm uninstall`.
- **BREAKING** Helm values: `plugins.<name>.webhookSecret` changes from `{name, key}` to `{existingSecret, key}`. Unset means "generated". With `existingSecret` the control plane only reads, and a missing Secret or key stops startup.
- **BREAKING** Instance config: `plugins.<name>.webhookSecretFile` is removed and replaced by `plugins.<name>.webhookSecret: {name, key, generate}`, which Helm always renders. The chart no longer mounts the secret as a file.
- **BREAKING** Behavior: with the Helm chart, every loaded plugin with a `webhooks` export has a secret, so its endpoint no longer answers `404` before an operator configures one. Only a hand-written instance config without `webhookSecret` (for example `serve --no-cluster`) keeps the endpoint off.
- Any Kubernetes API error while reading or creating a webhook secret stops startup.
- RBAC: the control plane's instance Role gets `create` on Secrets only when at least one plugin uses a generated secret.
- Rotation: delete the Secret (or change the existing one) and restart the control plane, then enter the new value in the forge. There is no hot reload.
- A new ADR records that the control plane generates missing instance secrets, and why this beat a Helm `lookup`, a hook Job or storage in PostgreSQL.

Out of scope:
- Showing the webhook URL and secret on an admin page in the web UI: backlog.
- Hot reload of a rotated secret: rejected (rotation is rare, a restart is cheap).
- A secret per forge host or per project: rejected (one per plugin, see the glossary entry **Webhook secret**).
- Moving the lifecycle event signing key off cert-manager: rejected (it already works with ArgoCD).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `forge-webhooks`: the endpoint no longer answers `404` for a missing secret; a new requirement covers where the secret comes from (existing or generated Secret, startup failure, rotation).
- `helm-deployment`: `plugins.<name>.webhookSecret` takes `{existingSecret, key}` and renders a Secret reference instead of a mounted file; the least-privilege RBAC adds a conditional `create` on Secrets in the instance namespace.

## Impact

- Code: `df-core` (`PluginConfig`, `SecretStore` gets a get-or-create method), `df-k8s` (`K8sSecretStore`), `dark-factory` (`forge.rs`, `read_webhook_secrets` replaced), `df-plugins` / `df-web` only for the removed "no secret" case.
- Helm chart: `values.yaml`, `values.schema.json`, `_helpers.tpl`, `control-plane-deployment.yaml`, `rbac.yaml`, Helm unit tests; `deploy/values-e2e.yaml` switches to `existingSecret`.
- Docs: `docs/content/integrations/forges/webhooks.md`, `crates/df-plugins/docs/configuration.md`, `deploy/helm/dark-factory/README.md`, `design/deployment.md` §3, new ADR in `docs/adr/`.
- Operators upgrading: rename `webhookSecret.name` to `webhookSecret.existingSecret`, or drop the value to let the control plane generate one and re-enter it in the forge.
