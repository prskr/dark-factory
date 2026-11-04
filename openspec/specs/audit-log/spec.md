# audit-log Specification

## Purpose

Records who did what in an instance — humans and agents — in an append-only log. Iteration 1 writes entries; viewing them comes later.

## Requirements

### Requirement: Append-only audit entries
The control plane SHALL write an audit entry for every state-changing action by a human or agent: creating or editing projects, personas, credential references, allowlist entries and event subscriptions, deleting event subscriptions, creating tasks, queueing messages and executing factory tool calls. Each entry SHALL contain time, actor, the actor's roles at that moment, action, resource type, resource id and details.

#### Scenario: Task creation is audited
- **WHEN** board member Peter creates task `DF-1`
- **THEN** an audit entry with action `task.create`, actor Peter, roles `[board-member]` and resource `DF-1` exists

#### Scenario: Agent tool call is audited
- **WHEN** the agent on run R calls `comment_on_issue`
- **THEN** an audit entry with an agent actor (persona, run, task) and action `tool.comment_on_issue` exists

#### Scenario: Subscription change is audited
- **WHEN** admin Peter adds a subscription to project `DF`
- **THEN** an audit entry with action `event_subscription.create`, roles `[board-member, admin]`, resource type `event_subscription` with the subscription ID as resource, and the project key, URL and event types in the details exists

### Requirement: Audit entries never contain secret values
Audit details SHALL reference credentials by name only and SHALL never contain secret values.

#### Scenario: Credential reference edited
- **WHEN** a credential reference is created
- **THEN** the audit entry contains its name, Secret name and key, and no secret value
