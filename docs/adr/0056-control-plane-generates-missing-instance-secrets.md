# ADR-0056: The control plane generates missing instance secrets

- **Status:** Accepted
- **Date:** 2026-11-09
- **Related:** [ADR-0051](0051-webhook-wake-rules.md)

## Context

A webhook secret has to exist before the control plane starts and the forge has to know it. The chart used to require the operator to create the Secret and reference it. Generating it in the chart does not work: a Helm `lookup` returns nothing under `helm template`, which ArgoCD uses, so every sync would render a new value. Operators who do not set one get no webhook endpoint at all.

## Decision

- **The control plane creates a missing Secret.** `SecretStore::get_or_create` reads the Secret; if it does not exist it creates it with one key holding 32 random bytes (`ring`), hex-encoded, and labels that name the instance and the plugin. On `409 AlreadyExists` it reads again, so concurrent replicas converge on one value.
- **It never updates.** A Secret that exists without the key is an error, not a patch. Rotation means deleting or changing the Secret and restarting the control plane.
- **The operator can opt out.** `plugins.<name>.webhookSecret.existingSecret` in the chart values references a Secret the operator manages (`generate: false` in the instance configuration); the control plane then only reads it.
- **No owner reference.** The Secret outlives `helm uninstall`, so reinstalling keeps the value the forge holds. The labels allow a selector-based cleanup.
- **The pattern is general.** A later instance secret that needs a random value can use `get_or_create`; this ADR covers webhook secrets only. The lifecycle-event signing key stays with cert-manager.

## Consequences

- The default install has a working webhook endpoint without a manual step; the operator reads the value once and enters it in the forge.
- The instance Role gets `create` on `secrets` when any plugin generates its secret. `create` cannot be limited by `resourceNames`, so a compromised control plane could create any Secret in its namespace. It can already read all of them, so the added exposure is small. The Role has no `update`, `patch` or `delete`.
- Startup needs the Kubernetes API for webhook plugins. An API outage or a denied `create` stops the control plane with an error naming the setting, instead of silently disabling the endpoint.
- Generated Secrets of a removed plugin or a deleted release stay until the operator removes them.
- A chart upgrade without migrating `webhookSecret.name` to `existingSecret` fails on the values schema instead of silently switching to a generated Secret.

## Alternatives considered

- **Helm `lookup` with a random fallback.** Rejected: `lookup` is empty under `helm template` (ArgoCD), so the value changes on every render.
- **A pre-install hook Job that creates the Secret.** Rejected: an extra image, ServiceAccount and Role for one Secret, and hooks behave differently under ArgoCD.
- **Store the secret in PostgreSQL.** Rejected: the Secret is already the place operators look for instance secrets, and the plugin host reads Secrets only through `SecretStore`.
- **Derive it from a cert-manager-generated key.** Rejected: ties webhook secrets to the certificate lifecycle and its rotation.
