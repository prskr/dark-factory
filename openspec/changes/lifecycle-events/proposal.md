# Proposal

## Why

External systems such as cost tracking, audit and chat notifications cannot react to what Dark Factory does with a task's infrastructure. Dark Factory sends no outgoing notifications, so an integration can only poll. Sandbox and volume lifecycle is the smallest useful set of events: it marks when compute and storage start and stop being used for a task. Decided in [ADR-0049](../../../docs/adr/0049-lifecycle-events-outbox-and-ed25519-signatures.md).

## What Changes

- **Lifecycle events** `volume.provisioned`, `volume.destroyed`, `sandbox.provisioning`, `sandbox.provisioned`, `sandbox.provisioning_failed` and `sandbox.destroyed`. Each carries project (ID, key, repository URL), task (ID, key), run (ID, number, where applicable), persona (ID, name) and the time the event happened, plus event-specific fields.
- **Early signal:** `sandbox.provisioning` fires when the scheduler starts provisioning a run, before the volume and sandbox are created. Integrations can prepare compute in time, for example a wake-on-LAN proxy that boots a node. Every `sandbox.provisioning` ends in exactly one `sandbox.provisioning_failed` or `sandbox.destroyed`, so receivers can count active sandboxes.
- **Delivery:** HTTP POST in the Standard Webhooks format, at least once, from a PostgreSQL outbox written in the same transaction as the state change. Retries with backoff and give-up after 24 h.
- **Generic outbox:** the outbox and its leader-only dispatcher are not specific to lifecycle events. Each kind of job has its own handler, so other components can use it. Moving `pr.rs` onto it is TD-91.
- **Signing:** Standard Webhooks `v1a` (Ed25519), one key for the whole instance. The public key is served unauthenticated at `GET /lifecycle-events/public-key` and shown in the UI.
- **Event subscriptions:**
  - Instance level, from Helm values (`lifecycleEvents.subscriptions`). These are trusted and may target cluster-internal URLs.
  - Project level, managed in the project settings UI by users with the new `admin` role. `https` only, and targets in private, loopback or link-local address ranges are refused.
  - Each subscription filters by event type and shows its last delivery result.
- **Helm:** by default the chart creates the signing key through cert-manager (Ed25519, `rotationPolicy: Never`). `lifecycleEvents.signingKey.existingSecret` is optional.
- **Roles:** new role `admin`, required only for managing project subscriptions. The local and e2e Dex users `admin@example.com` get it.
- **Volume tracking:** the task records when its volume was first created, so `volume.provisioned` fires once per task. `volume.destroyed` is emitted from the volume deletion of `iter2-task-volume-lifecycle`, which this change builds on.
- **Receiver reference:** a docs page with the event catalog, fields, headers and a verification example.

Out of scope:
- A "started" event for volumes (`volume.provisioning`): backlog in `product/iterations/roadmap.md` ("Lifecycle event extensions").
- Run and task state events (run succeeded, task in review, …): backlog, same entry.
- Two signatures during a key rotation window: backlog, same entry.
- Delivery history browser and manual redelivery: backlog, same entry.
- The general permission check `can(principal, action, resource)` of ADR-0011: backlog, same entry. This change only checks a role.
- Blocking or admission-style hooks that can stop a run: rejected (ADR-0049).
- Per-subscription HMAC secrets: rejected (ADR-0049).

## Capabilities

### New Capabilities
- `lifecycle-events`: which lifecycle events exist, when each fires, the payload and headers, signing, the public key endpoint and the delivery guarantee.
- `event-subscriptions`: how receivers are registered at instance and project level, event filters, URL restrictions, who may manage them, and the delivery status shown.

### Modified Capabilities
- `human-auth`: a second role `admin` that, in addition to `board-member`, is required for managing project subscriptions.
- `audit-log`: creating, editing and deleting event subscriptions is audited.
- `helm-deployment`: `lifecycleEvents` values, the signing key Certificate and its Secret mount.

## Impact

- **`df-db`:**
  - migrations for the outbox, project subscriptions with delivery status, and `tasks.volume_provisioned_at`;
  - outbox writes in the run and volume transactions;
  - `.sqlx/` metadata.
- **`df-agent-channel`:** the scheduler, `mark_started`, `provisioning_failed` and `finish_run` enqueue events.
- **New dispatcher (leader only):**
  - uses `reqwest` (already a workspace dependency, new in the control plane) and `ring` Ed25519 signing (already a dependency);
  - resolves DNS and checks addresses for project subscriptions.
- **`df-web`:**
  - subscription management in project settings and the `admin` role check;
  - the public key endpoint.
- **`dark-factory`:** config for subscriptions and the signing key file.
- **Helm chart:**
  - `lifecycleEvents` values and schema;
  - `Certificate` for the signing key;
  - Secret mount, unit tests and README.
- **Dev setup:** `dev/dex.yaml` and `deploy/e2e/dex.yaml` get the `admin` group. On df-dev, the `admin` role is assigned in Keycloak by hand (manual tasks).
- **Depends on:** `iter2-task-volume-lifecycle` (volume deletion, retention scan, reset workspace).
- **Docs:**
  - ADR-0049 and `product/GLOSSARY.md` (written);
  - receiver reference page;
  - configuration reference;
  - roadmap entry.
