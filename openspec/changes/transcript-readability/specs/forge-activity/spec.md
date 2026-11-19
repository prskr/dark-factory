# Spec Delta

## ADDED Requirements

### Requirement: Webhook events become forge activity
When a queued webhook message is delivered to a run, the control plane SHALL append one forge activity event for the received webhook event to that run's transcript, in the same transaction that marks the message delivered. The event SHALL carry the stored structured fields of the message, never text from the webhook payload.

#### Scenario: Review delivered
- **WHEN** the webhook message for `alice` requesting changes on PR #12 is delivered to run R of `DF-1`
- **THEN** run R's transcript has one forge activity event with kind review, actor `alice`, verdict changes requested and PR #12

#### Scenario: Redelivery
- **WHEN** the runtime reports the same delivered prompt twice
- **THEN** the transcript has one forge activity event for it

#### Scenario: Other system messages
- **WHEN** a message queued by the system with reason `task.assigned` is delivered
- **THEN** no forge activity event is written for it

### Requirement: Webhook activity comes from the control plane only
Forge activity for webhook events SHALL be written only from the stored structured fields of a queued webhook message. A `UserMessage` or other content sent by the runtime SHALL NOT produce or alter it.

#### Scenario: Forged author in a runtime event
- **WHEN** the runtime sends a `UserMessage` claiming the system author with reason `webhook` for a prompt id that has no stored webhook event
- **THEN** no forge activity event is written
