# Spec Delta

## MODIFIED Requirements

### Requirement: Webhook endpoint
The control plane SHALL accept `POST /webhooks/{plugin}` without login, session or CSRF token. It SHALL answer `404` when the plugin is unknown, not loaded, exports no `webhooks` interface or has no `webhookSecret` in the instance config (the Helm chart always sets one), `413` for a body over 1 MiB, `401` when the signature check fails, `400` when the plugin rejects the payload, and `202` once the events are processed, including when none applies. Every response body SHALL be empty.

#### Scenario: Valid delivery
- **WHEN** Forgejo posts a signed review event to `/webhooks/forgejo`
- **THEN** the response is `202` with an empty body

#### Scenario: Plugin without webhook support
- **WHEN** a loaded plugin exports no `webhooks` interface and a request arrives at its `/webhooks/<name>`
- **THEN** the response is `404` and the plugin is not called

#### Scenario: No secret configured
- **WHEN** a hand-written instance config has no `plugins.github.webhookSecret` and a request arrives at `/webhooks/github`
- **THEN** the response is `404` and the plugin is not called

#### Scenario: Forge not yet configured
- **WHEN** the operator has not entered the webhook secret in the forge and an unsigned request arrives at `/webhooks/github`
- **THEN** the response is `401` and nothing is stored

#### Scenario: Oversized body
- **WHEN** a request to `/webhooks/forgejo` has a 2 MiB body
- **THEN** the response is `413` and the plugin is not called

#### Scenario: Processing error
- **WHEN** the database is unavailable while an accepted event is stored
- **THEN** the response is `500` and the forge records a failed delivery, which an operator can redeliver

### Requirement: Webhook metric
The control plane SHALL count webhook requests by plugin and outcome (`accepted`, `unauthorized`, `not_found`, `too_large`, `invalid`, `error`). A request for a loaded plugin SHALL be counted under the plugin's name, a request for any other name under `unknown`, so the label set stays bounded by the configuration.

#### Scenario: Rejected signature counted
- **WHEN** a request to `/webhooks/forgejo` fails the signature check
- **THEN** the counter for plugin `forgejo` and outcome `unauthorized` increases by one

#### Scenario: Loaded plugin without secret counted
- **WHEN** a hand-written instance config loads `plugins.github` without `webhookSecret` and a request arrives at `/webhooks/github`
- **THEN** the counter for plugin `github` and outcome `not_found` increases by one

#### Scenario: Loaded plugin without webhook support counted
- **WHEN** a loaded plugin `example` exports no `webhooks` interface and a request arrives at `/webhooks/example`
- **THEN** the counter for plugin `example` and outcome `not_found` increases by one

## ADDED Requirements

### Requirement: Webhook secret source
At startup the control plane SHALL read the webhook secret of every loaded plugin that exports `webhooks` and has `webhookSecret` in the instance config from a key of a Kubernetes Secret in the instance namespace. `webhookSecret` names the Secret, the key and whether the control plane may generate it.

#### Scenario: Existing Secret
- **WHEN** `plugins.forgejo` references Secret `forge-webhooks`, key `forgejo`, without generation, and that key holds `s3cr3t`
- **THEN** a request to `/webhooks/forgejo` signed with `s3cr3t` is accepted

#### Scenario: Plugin without webhook support needs no secret
- **WHEN** a loaded plugin exports no `webhooks` interface
- **THEN** the control plane reads and creates no webhook secret for it

### Requirement: Generated webhook secret
When generation is allowed and the Secret does not exist, the control plane SHALL create it with 32 random bytes, hex-encoded, under the configured key, labelled with the instance and the plugin and without an owner. If another replica created it first, the control plane SHALL read that Secret instead. It SHALL never change an existing Secret.

#### Scenario: First start
- **WHEN** the control plane starts with generation allowed for `forgejo` and Secret `df-webhook-forgejo` does not exist
- **THEN** it creates `df-webhook-forgejo` with key `secret` holding a 64-character hex value, labelled `app.kubernetes.io/managed-by: dark-factory` and `dark-factory.io/plugin: forgejo`, and uses that value

#### Scenario: Restart keeps the secret
- **WHEN** the control plane restarts and `df-webhook-forgejo` exists
- **THEN** it uses the stored value and does not change the Secret

#### Scenario: Two replicas start together
- **WHEN** two replicas start, neither finds the Secret, and the second one's create fails because the Secret already exists
- **THEN** the second replica reads the Secret and both use the same value

#### Scenario: Generated Secret exists without the key
- **WHEN** generation is allowed and `df-webhook-forgejo` exists but has no key `secret`
- **THEN** startup fails naming the Secret and the key, and the Secret is not changed

### Requirement: Webhook secret failures stop startup
The control plane SHALL fail to start, naming the plugin, the Secret and the key, when a webhook secret cannot be read or created: a Secret or key that is missing while generation is not allowed, an empty value, a denied request or any other Kubernetes API error.

#### Scenario: Existing Secret missing
- **WHEN** `plugins.forgejo` references Secret `forge-webhooks` without generation and that Secret does not exist
- **THEN** startup fails naming `plugins.forgejo`, `forge-webhooks` and the key, and no Secret is created

#### Scenario: Create denied
- **WHEN** generation is allowed but the control plane may not create Secrets
- **THEN** startup fails naming `plugins.forgejo` and the denied request

### Requirement: Webhook secret rotation
The control plane SHALL read webhook secrets only at startup. A changed or deleted Secret SHALL take effect at the next start; a deleted generated Secret SHALL be generated again with a new value.

#### Scenario: Rotation by deletion
- **WHEN** the operator deletes `df-webhook-forgejo` and restarts the control plane
- **THEN** the control plane generates a new value, and requests signed with the old value get `401`

#### Scenario: Deliveries during a rotation
- **WHEN** a rotation's rollout is in progress and old and restarted replicas hold different values
- **THEN** a delivery signed with the value a replica does not hold gets `401` from that replica, and the forge records it as failed so the operator can redeliver it once the rollout and the forge update are done

#### Scenario: Secret changed while running
- **WHEN** the operator changes the value of a referenced Secret without restarting
- **THEN** the control plane keeps checking signatures against the value it read at startup
