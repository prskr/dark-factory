# task-delegation Specification

## Purpose
Defines who a task reports to and how work flows back up the delegation chain: the parent and creator of a task, the parent's wake-up whenever a child ends, and messages from a parent to its children.

## Requirements

### Requirement: Creator and parent
Every task SHALL record its creator as a principal: a human, or the agent of the task that created it. A task created by an agent SHALL have that task as its parent; a task created by a board member SHALL have no parent. The parent SHALL NOT change after creation.

#### Scenario: Created by an agent
- **WHEN** the agent of `DF-20` creates a task
- **THEN** the new task's creator is the agent of `DF-20` and its parent is `DF-20`

#### Scenario: Created by a board member
- **WHEN** a board member creates a task in the UI
- **THEN** the task's creator is that board member and it has no parent

### Requirement: Parent wakes when a child ends
When a child completes (its run ends with outcome `completed`), becomes Done, fails or is cancelled, the control plane SHALL queue one message on its parent in the same transaction, authored by the system principal with reason `task.child_ended`. Each event SHALL produce its own message. A child waiting for a human (`waiting`, `blocked`, `limit_reached`) or for time (`rate_limited`) SHALL NOT wake its parent.

#### Scenario: Child completes
- **WHEN** child `DF-21` of `DF-20` ends a run with outcome `completed` while `DF-20` is `in_review`
- **THEN** a message with reason `task.child_ended` is queued on `DF-20` and `DF-20` becomes `queued`

#### Scenario: Completion, then merge
- **WHEN** child `DF-21` completes and later its PR is merged
- **THEN** its parent receives two messages, one for the Completion and one for Done

#### Scenario: Child fails
- **WHEN** child `DF-21` ends a run with outcome `failed`
- **THEN** its parent receives a `task.child_ended` message saying it failed and why

#### Scenario: Child cancelled
- **WHEN** a board member cancels child `DF-21`
- **THEN** its parent receives a `task.child_ended` message saying it was cancelled

#### Scenario: Child waits for a human
- **WHEN** child `DF-21` ends a turn without calling `complete_task` and becomes `waiting`
- **THEN** its parent receives no message

### Requirement: Completed runs on finished children
A run that ends `completed` on a child that is already Done or `cancelled` SHALL NOT wake its parent.

#### Scenario: Run completes on a cancelled child
- **WHEN** child `DF-21` was cancelled and its run then ends with outcome `completed`
- **THEN** its parent receives no further message

#### Scenario: Run completes on a done child
- **WHEN** child `DF-21` is Done and a later run of it ends with outcome `completed`
- **THEN** its parent receives no message for it

### Requirement: Final parents are not woken
No child-ended message SHALL be queued on a parent that is Done (`completed`), `cancelled` or `failed`.

#### Scenario: Parent already done
- **WHEN** child `DF-21` completes after its parent `DF-20` was marked done
- **THEN** no message is queued on `DF-20` and it stays `completed`

### Requirement: Content of the child-ended message
The child-ended message SHALL contain the child's key and title, how it ended (completed, done, failed or cancelled), its completion summary or failure reason when one exists, its PR link when one exists, and the keys of tasks that are still `awaiting_dependencies` on the child. Text from the child SHALL be marked as coming from the child's agent.

#### Scenario: Failed child with waiting dependents
- **WHEN** child `DF-20` fails and `DF-21` still awaits its Completion
- **THEN** the parent's message names `DF-20`, says it failed with its failure reason and lists `DF-21` as still waiting on it

#### Scenario: Completed child with a PR
- **WHEN** child `DF-21` completes with summary "Review done, two findings" and has PR #12
- **THEN** the parent's message contains the summary and the link to PR #12

### Requirement: Messages from a parent to its children
The agent of a task SHALL be able to queue a message on any of its own children, and on no other task. Such a message SHALL be authored by that agent and SHALL have the same effects as a board member's message on the child: delivery between turns, status change, and reopening of an `in_review`, Done or `cancelled` child.

#### Scenario: Relay to the implementer
- **WHEN** the agent of `DF-19` sends "address the review findings on PR #12" to its child `DF-20`, which is `in_review`
- **THEN** the message is queued on `DF-20` with the agent of `DF-19` as author and `DF-20` becomes `queued`

#### Scenario: Not a child
- **WHEN** the agent of `DF-20` sends a message to its sibling `DF-21`
- **THEN** the message is refused and nothing is queued
