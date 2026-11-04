# Spec Delta

## Purpose

Notifies external systems, after the fact and at least once, when a task's volume or a run's sandbox is created or destroyed, in a signed format they can verify.

## ADDED Requirements

### Requirement: Volume provisioned event
The control plane SHALL emit `volume.provisioned` exactly once per volume creation, when a run is scheduled and the task's volume does not exist. A volume created again after `volume.destroyed` SHALL emit it again. The event SHALL carry the run whose scheduling created the volume.

#### Scenario: First run creates the volume
- **WHEN** the first run of task `DF-42` is scheduled and the volume `task-df-42` does not exist yet
- **THEN** one `volume.provisioned` event for `DF-42` with that run is emitted

#### Scenario: Continuation run
- **WHEN** a later run of `DF-42` is scheduled and the volume already exists
- **THEN** no `volume.provisioned` event is emitted

#### Scenario: Volume created again after a reset
- **WHEN** the workspace of `DF-42` was reset (`volume.destroyed` with reason `reset`) and the next run of `DF-42` is scheduled
- **THEN** one `volume.provisioned` event for `DF-42` with that run is emitted

### Requirement: Volume destroyed event
The control plane SHALL emit `volume.destroyed` when a task's volume is deleted, with a `reason` of `retention` or `reset` and without a run.

#### Scenario: Retention deletes a volume
- **WHEN** the retention scan deletes the volume of closed task `DF-42`
- **THEN** one `volume.destroyed` event for `DF-42` with reason `retention` is emitted

### Requirement: Sandbox provisioning event
The control plane SHALL emit `sandbox.provisioning` once per run, when the scheduler starts provisioning it, before the run's volume and sandbox are created. It SHALL NOT be emitted while a run waits to be scheduled, and the control plane SHALL NOT wait for any receiver before provisioning.

#### Scenario: Scheduler picks a run
- **WHEN** the scheduler starts provisioning run 3 of `DF-42`
- **THEN** one `sandbox.provisioning` event for that run is emitted before the sandbox is created

#### Scenario: Run waits in the queue
- **WHEN** run 3 of `DF-42` is created but not yet picked by the scheduler
- **THEN** no `sandbox.provisioning` event is emitted for it

### Requirement: Sandbox events are paired
Every `sandbox.provisioning` SHALL be followed by exactly one `sandbox.provisioning_failed` or `sandbox.destroyed` for the same run.

#### Scenario: Counting active sandboxes
- **WHEN** a receiver adds one per `sandbox.provisioning` and removes one per `sandbox.provisioning_failed` or `sandbox.destroyed`, keyed by run ID
- **THEN** once all runs have ended and all events are delivered, its count is zero

### Requirement: Sandbox provisioned event
The control plane SHALL emit `sandbox.provisioned` once per run, when the provider reports the run's pod as running. It SHALL NOT wait for the agent to connect.

#### Scenario: Pod starts
- **WHEN** the pod of run 3 of `DF-42` is reported running
- **THEN** one `sandbox.provisioned` event for that run is emitted

### Requirement: Sandbox provisioning failed event
The control plane SHALL emit `sandbox.provisioning_failed` with an `error` text when provisioning a run's sandbox fails. For that run, no `sandbox.destroyed` SHALL follow.

#### Scenario: Provisioning fails
- **WHEN** creating the sandbox for run 3 of `DF-42` fails
- **THEN** one `sandbox.provisioning_failed` event with the error is emitted and no `sandbox.destroyed` event for that run

### Requirement: Sandbox destroyed event
The control plane SHALL emit `sandbox.destroyed` once per run whose provisioning began and did not fail, when the run ends and its teardown is issued. The event SHALL carry the run's outcome, the time the sandbox was provisioned (`started_at`, `null` if the pod never came up) and the time the run ended (`ended_at`).

#### Scenario: Run completes
- **WHEN** run 3 of `DF-42` ends with outcome `completed` and its sandbox is torn down
- **THEN** one `sandbox.destroyed` event with outcome `completed`, `started_at` and `ended_at` is emitted

#### Scenario: Agent never connects
- **WHEN** the pod of run 3 started but the agent did not connect in time
- **THEN** a `sandbox.destroyed` event with outcome `connect_timeout` is emitted

#### Scenario: Cancelled before the pod came up
- **WHEN** run 3 of `DF-42` is cancelled while it is still provisioning
- **THEN** one `sandbox.destroyed` event with outcome `cancelled` and `started_at: null` is emitted

#### Scenario: Sandbox lost
- **WHEN** the pod of a running run disappears
- **THEN** a `sandbox.destroyed` event with outcome `sandbox_lost` is emitted

### Requirement: Event envelope
Every event SHALL be a JSON object with `id` (`evt_` followed by a UUIDv7), `type`, `timestamp` (RFC 3339, the time the event happened) and `data`. `data` SHALL contain `project` (`id`, `key`, `repo_url`), `task` (`id`, `key`), `persona` (`id`, `name`) and, except for `volume.destroyed`, `run` (`id`, `number`).

#### Scenario: Sandbox provisioned payload
- **WHEN** `sandbox.provisioned` is sent for run 3 of `DF-42` in project `DF`
- **THEN** the body contains the event ID, type, time and the project, task, run and persona objects with IDs and keys, and the project's repository URL

### Requirement: Events only for committed changes
An event SHALL be recorded in the same transaction as the state change that causes it. A rolled-back change SHALL emit no event, and a committed change SHALL emit its event even if the control plane restarts before delivery.

#### Scenario: Restart before delivery
- **WHEN** the control plane restarts after a run's state change was committed but before its event was sent
- **THEN** the event is delivered after the restart

### Requirement: Standard Webhooks headers and signature
Every delivery SHALL be an HTTP POST with `Content-Type: application/json` and the headers `webhook-id` (the event ID), `webhook-timestamp` (Unix seconds of the send attempt) and `webhook-signature`. The signature SHALL be `v1a,<base64 Ed25519 signature>` over `<webhook-id>.<webhook-timestamp>.<body>`, made with the instance signing key.

#### Scenario: Receiver verifies an event
- **WHEN** a receiver verifies a delivery with the instance public key per the Standard Webhooks `v1a` scheme
- **THEN** verification succeeds

#### Scenario: Tampered body
- **WHEN** the body of a delivery is changed in transit
- **THEN** verification with the public key fails

### Requirement: Public key endpoint
The control plane SHALL serve the instance public key without authentication at `GET /lifecycle-events/public-key` as `text/plain` in the Standard Webhooks form `whpk_<base64>`.

#### Scenario: Anonymous key fetch
- **WHEN** an unauthenticated client requests `/lifecycle-events/public-key`
- **THEN** the response is 200 with the `whpk_` encoded public key

### Requirement: At-least-once delivery with retries
A delivery SHALL count as successful on any 2xx response within 10 seconds. Failures and timeouts SHALL be retried with exponential backoff from 30 seconds, capped at 1 hour, and given up 24 hours after the event. Each subscription SHALL be delivered independently. Events SHALL have no ordering guarantee.

#### Scenario: Receiver down temporarily
- **WHEN** a receiver answers 503 twice and then 200
- **THEN** the event is delivered on the third attempt with the same `webhook-id`

#### Scenario: Receiver down for a day
- **WHEN** a receiver fails for more than 24 hours after the event
- **THEN** delivery to that receiver is given up and other subscriptions are unaffected

### Requirement: Events do not affect runs
Delivery outcome SHALL NOT influence any run or task. A failing or slow receiver SHALL NOT delay provisioning, teardown or any state change.

#### Scenario: Receiver times out
- **WHEN** every receiver times out
- **THEN** runs are provisioned and torn down as without subscriptions
