# Design

## Context

See proposal.md for motivation; the requirements are in the delta specs (`forge-webhooks`, `helm-deployment`).

Current state:
- `crates/dark-factory/src/forge.rs` (`read_webhook_secrets`) reads `plugins.<name>.webhookSecretFile` synchronously in `load_forge_plugins`, before the database and the sandbox backend start.
- The control plane reads Kubernetes Secrets through the `SecretStore` trait (`df-core/src/seams.rs`, `get` only). `K8sSecretStore` (`df-k8s`) reads from the instance namespace. The store reaches `serve` only through `SandboxBackend::start` (`SandboxWiring.secrets`); `serve --no-cluster` gets `UnconfiguredSecretStore`, which fails every call.
- `ForgePlugins.webhook_secrets` feeds `forge::forge_client`, which feeds the webhook router.
- The chart mounts `webhookSecret.{name,key}` as a file (`control-plane-deployment.yaml`, `_helpers.tpl`); the instance Role grants `get` on `secrets` and `services` (`rbac.yaml`).

## Goals / Non-Goals

**Goals:**
- One code path for reading a webhook secret, whether the operator provided it or the control plane generated it.
- A chart render that does not depend on cluster state.

**Non-Goals:**
- A general "generated secrets" mechanism for other secrets: rejected for now; this change adds only what webhook secrets need, but the ADR states the pattern so a later secret can reuse it.
- Changing how plugins verify signatures (`webhook-signature.verify`): unchanged.

## Decisions

### D1: Get-or-create on `SecretStore`

Add one method to `SecretStore`:

```rust
async fn get_or_create(&self, r: &SecretKeyRef, labels: &BTreeMap<String, String>) -> Result<SecretString, SecretError>;
```

`K8sSecretStore` implements it as: `get`; on `404` create a Secret with one key holding 32 bytes from `ring::rand::SystemRandom`, hex-encoded (`hex`, already a workspace dependency); on `409 AlreadyExists` `get` again. It never updates or patches. A Secret that exists without the key is an error (`SecretError::NotFound`), not a patch.

`get` today takes a `CredentialRef` (with id and display name from the database), although `K8sSecretStore` uses only the Secret name and key. Webhook secrets have no id or display name, so both methods take a smaller `SecretKeyRef { secret_name, secret_key }`: `get(&self, r: &SecretKeyRef)` replaces `get(&self, r: &CredentialRef)`. `CredentialRef` embeds it as `pub secret: SecretKeyRef` in place of its own `secret_name` and `secret_key` fields, with `#[serde(flatten)]` so its serialized form stays the same. Existing callers pass `&credential.secret`, with no conversion or clone. This touches every place that builds a `CredentialRef` (mostly the row mapping in `df-db`) and reads its Secret name or key, all mechanically.

Alternatives: a `CredentialRef::key_ref()` helper that copies the two fields. Rejected: it keeps the duplicated fields and clones on every lookup; embedding models a credential reference as what it is, a database identity plus a key reference. The resolver calls `get` with the configured `{name, key}` when `generate` is false, and `get_or_create` when it is true. `UnconfiguredSecretStore`, `FakeSecretStore` and the gateway's test store implement the new method (the fake records created Secrets for tests).

Alternatives: a separate `WebhookSecretProvisioner` trait. Rejected: same backend, same namespace, same error type; a second trait doubles the wiring through `SandboxWiring` for one method.

Random generation lives in the store (it must happen only when the Secret is missing). `ring` is already in the graph (`df-plugins`); `df-k8s` adds it as a direct dependency.

### D2: Resolve webhook secrets after the backend starts

`load_forge_plugins` keeps loading plugins and stops reading secrets. A new async `resolve_webhook_secrets(&ForgePlugins, &dyn SecretStore)` runs in `serve` right after `backend.start(...)` and before `forge::forge_client(...)`. It visits every loaded plugin whose `capabilities().webhooks` is true and whose config has `webhookSecret`: `get_or_create` when `generate`, else `get`. Any error fails startup with `plugins.<name>.webhookSecret`, the Secret and the key in the message. Each call has the existing `K8S_API_TIMEOUT`.

A plugin with `webhookSecret` but no `webhooks` export is skipped with an `info` log, so the chart can render the reference for every plugin without knowing its exports.

Alternative: resolve before loading plugins (as today). Rejected: it would create Secrets for plugins that fail to load or export no webhooks.

### D3: Instance config shape

```yaml
plugins:
  forgejo:
    webhookSecret:
      name: dark-factory-webhook-forgejo   # Secret in the instance namespace
      key: secret
      generate: true                        # default false
```

`webhookSecretFile` is removed (serde `deny_unknown_fields` already rejects leftovers, so an old config fails loudly). `webhookSecret` stays optional in the config: without it the plugin has no webhook endpoint. That keeps `serve --no-cluster` and hand-written dev configs working without a cluster; the chart always renders it.

### D4: Chart rendering

- Values: `webhookSecret: {existingSecret: '', key: ''}` replaces `null`. Empty `existingSecret` → `name: <fullname>-webhook-<plugin>`, `key: secret` (or the given `key`), `generate: true`. Set → `name: <existingSecret>`, `key: <key or "secret">`, `generate: false`. `values.schema.json` drops `name` and accepts the new shape; `null` stays accepted and means "generated".
- The webhook volumes and mounts are removed.
- `rbac.yaml`: the instance Role adds a rule `secrets: [create]` when any non-null plugin has an empty `existingSecret`. `create` cannot be narrowed with `resourceNames`; `get` stays as it is.

### D5: Labels on generated Secrets

`app.kubernetes.io/name: dark-factory`, `app.kubernetes.io/managed-by: dark-factory`, `app.kubernetes.io/component: webhook-secret`, `app.kubernetes.io/instance: <instanceName>`, `dark-factory.io/plugin: <plugin>`, with `instanceName` taken from the existing top-level setting of the instance config. No owner reference, so the Secret outlives `helm uninstall` (decided in the grilling session).

### D6: ADR

New `docs/adr/0056-control-plane-generates-missing-instance-secrets.md`: context (ArgoCD and `lookup`), decision (control plane get-or-create, `existingSecret` escape hatch, never updates), alternatives (Helm `lookup`, hook Job, PostgreSQL, cert-manager-generated key), consequences (wider RBAC, Secrets outlive the release, lifecycle signing key stays on cert-manager). Indexed in `docs/adr/README.md`.

## Risks / Trade-offs

- [`create` on Secrets lets a compromised control plane create any Secret in the instance namespace] → Only rendered when needed (D4); the control plane already reads every Secret there, so the added risk is small. `design/deployment.md` §3 documents it.
- [Orphaned Secrets after uninstall or a removed plugin] → Labels (D5) and a documented cleanup command.
- [Startup now needs the Kubernetes API for webhooks; an API outage blocks restarts] → The control plane already needs the API for sandboxes at startup; failing loudly was chosen over a silently disabled endpoint.
- [An upgrade without migrating values switches an operator from their own Secret to a generated one, breaking existing forge webhooks] → The values schema rejects the old `name` field, so `helm upgrade` fails until the values are migrated; the upgrade note in the docs names the rename.

## Migration Plan

1. Operators rename `plugins.<name>.webhookSecret.name` to `existingSecret` (keeping `key`), or remove the value and copy the generated secret into the forge after the upgrade.
2. `deploy/values-e2e.yaml` uses `existingSecret`.
3. Rollback: the previous chart version mounts the referenced Secret again; generated Secrets stay. If the forge was switched to the generated value, the old chart and the forge disagree and deliveries get `401`: either enter the old value in the forge again, or point the old chart at the generated Secret with `webhookSecret: {name: <fullname>-webhook-<plugin>, key: secret}`. Then redeliver the failed events from the forge.

Rotation and replicas: during the rolling restart after a rotation, old and new replicas hold different values, and the forge holds whichever value the operator has entered so far. Deliveries in that window can get `401`. The operator docs say to finish the rollout and the forge update, then redeliver the failed events from the forge's delivery history.
