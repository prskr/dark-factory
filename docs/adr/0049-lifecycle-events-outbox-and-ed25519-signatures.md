# ADR-0049: Lifecycle events via a PostgreSQL outbox, signed with Ed25519

- **Status:** Accepted
- **Date:** 2026-11-02
- **Related:** [ADR-0013](0013-single-tenant-instances.md), [ADR-0015](0015-webhook-filtering-and-routing.md), [ADR-0020](0020-configuration-in-database.md), [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md)

## Context

External systems (cost tracking, audit, chat notifications) want to react when a task's volume or a run's sandbox is created or destroyed. Dark Factory has no outgoing notifications. The existing LISTEN/NOTIFY fan-out carries IDs only and drops events when the connection is lost, so it cannot carry notifications to third parties.

Receivers are configured both for the whole instance (by the operator) and per project (by an admin in the UI). Every receiver must be able to verify that an event came from this instance.

## Decision

- **Lifecycle events** are notifications only. They are sent after the fact, and a receiver's response never affects a run.
- **Delivery is at least once.** An event is written to an outbox table in the same transaction as the state change that caused it. A dispatcher on the leader delivers it to every matching **event subscription** independently, with retries and backoff, and gives up after 24 h. Receivers deduplicate by the event ID. Events have no ordering guarantee.
- **The wire format follows [Standard Webhooks](https://www.standardwebhooks.com/)** (`webhook-id`, `webhook-timestamp`, `webhook-signature` headers; JSON envelope with `id`, `type`, `timestamp`, `data`).
- **Signatures are asymmetric:** the `v1a` scheme (Ed25519) with one instance-wide key. Receivers verify with the public key, which the control plane serves unauthenticated and shows in the UI. By default the Helm chart has cert-manager create the key (a `Certificate` with an Ed25519 private key and `rotationPolicy: Never`, the certificate itself unused). An `existingSecret` is optional.
- **One key at a time.** Rotation replaces the key, and receivers fetch the new public key.

## Consequences

- No event is lost on a crash or restart, and no event is sent for a rolled-back change.
- A receiver can verify events but cannot forge them, even though all projects share one key. There are no per-receiver secrets to create, store or rotate.
- The envelope and headers are a public contract. Changing them later breaks receivers.
- Fewer receiver libraries support `v1a` than the HMAC scheme `v1`. Receivers may need a few lines of Ed25519 verification code.
- Rotating the key breaks verification until receivers re-fetch the public key. Sending two signatures during a rotation window can be added without changing the format.
- Project-level receiver URLs are untrusted input. The control plane must refuse targets in private address ranges.

## Alternatives considered

- **HMAC-SHA256 with one shared secret (`v1`):** every receiver holding the secret could forge events for all projects and instance subscriptions.
- **HMAC with one secret per subscription, referenced through `credential_refs`:** isolates receivers, but every project subscription needs a hand-made Kubernetes Secret.
- **Fire-and-forget delivery, or retries in memory only:** events are lost on a receiver outage or a control plane restart, which makes the events useless for cost tracking and audit.
- **Blocking (admission-style) hooks:** would put an external system on the critical path of every run.
