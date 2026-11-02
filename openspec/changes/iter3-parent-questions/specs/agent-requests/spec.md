## ADDED Requirements

### Requirement: Question addressee
A question SHALL be addressed either to the agent of the asking task's parent task or to humans. An approval request SHALL always be addressed to humans. A question addressed to a parent that is Done, cancelled or failed, or that is an archived chat, SHALL be stored as addressed to humans. A parent that has only completed SHALL still be the addressee.

#### Scenario: Question to a running parent
- **WHEN** the agent of `DF-21` (parent `DF-20`, `running`) asks its parent "Keep the old API?"
- **THEN** a pending question of `DF-21` addressed to `DF-20` exists

#### Scenario: Parent is in review
- **WHEN** `DF-21` asks its parent while `DF-20` is `in_review`
- **THEN** the question is addressed to `DF-20`

#### Scenario: Parent cancelled
- **WHEN** `DF-21` asks its parent while `DF-20` is `cancelled`
- **THEN** the question is addressed to humans

#### Scenario: Parent chat archived
- **WHEN** a task whose parent is the archived chat `DF-12-CHAT` asks its parent
- **THEN** the question is addressed to humans

### Requirement: Question delivered to the parent
Raising a new question addressed to a parent SHALL queue one message on the parent, in the same transaction, authored by the system principal with reason `question.asked`. It SHALL name the asking task's key and title, the request id, the suggested answers and whether the asking agent waits, and quote the question text as the asking agent's words. A merged duplicate SHALL NOT queue another message.

#### Scenario: Parent woken
- **WHEN** `DF-21` asks `DF-20` "Keep the old API?" with suggested answers "yes" and "no" while `DF-20` is `waiting`
- **THEN** `DF-20` has a queued `question.asked` message with the request id, `DF-21`, both suggested answers and the quoted text, and becomes `queued`

#### Scenario: Message on an archived chat parent
- **WHEN** a question is addressed to the chat `DF-12-CHAT` and the chat is archived afterwards
- **THEN** the `question.asked` message stays queued and no run starts until the chat is unarchived

### Requirement: Questions readdressed when the parent ends
When a parent task becomes Done, cancelled or failed, or a chat is archived, every pending question addressed to it SHALL be readdressed to humans in the same transaction, remember the parent that ended, and publish a `request_changed` notification. Unarchiving a chat SHALL NOT readdress questions back to it.

#### Scenario: Parent cancelled with a pending question
- **WHEN** `DF-20` is cancelled while a question of `DF-21` addressed to it is pending
- **THEN** the question is addressed to humans, stays `pending`, and records that its parent `DF-20` ended

#### Scenario: Chat archived with a pending question
- **WHEN** a board member archives `DF-12-CHAT` while a question of `DF-13` addressed to the chat is pending
- **THEN** the archive succeeds and the question is addressed to humans

#### Scenario: Unarchived chat
- **WHEN** `DF-12-CHAT` is unarchived while the readdressed question of `DF-13` is still pending
- **THEN** the question stays addressed to humans

### Requirement: Answer by the parent's agent
The agent of the addressed parent task SHALL be able to answer a pending question addressed to its task. The decider SHALL be recorded as that agent's principal, and the answer SHALL be delivered like a human decision. A question that is not pending or not addressed to the caller's task SHALL NOT change.

#### Scenario: Parent answers
- **WHEN** the agent of `DF-20` answers the pending question of `DF-21` with "yes"
- **THEN** the question is `answered` with text "yes", the decider is the agent of `DF-20`, and `DF-21` has a queued `request.decided` message naming `DF-20` as the one who answered

#### Scenario: Sibling tries to answer
- **WHEN** the agent of `DF-22` answers a question of `DF-21` addressed to `DF-20`
- **THEN** the call fails and the question stays `pending`

#### Scenario: Readdressed question
- **WHEN** the agent of `DF-20` answers a question that was readdressed to humans
- **THEN** the call fails and the question stays `pending`

## MODIFIED Requirements

### Requirement: Decisions
Deciding SHALL require a human with `approval:decide` (iteration 2: the `board-member` role), or, for a question addressed to a parent task, the agent of that task. A human SHALL be able to answer any pending question, including one addressed to a parent task. An answer SHALL carry text. An approval SHALL carry one of the scopes the type offers. A denial MAY carry a comment.

#### Scenario: Answer by suggested answer
- **WHEN** a board member picks the suggested answer "17"
- **THEN** the question is `answered` with text "17" and the deciding user

#### Scenario: Human answers a question addressed to the parent
- **WHEN** a board member answers a pending question of `DF-21` addressed to `DF-20`
- **THEN** the question is `answered` by the board member and a later answer from the agent of `DF-20` fails with "already answered"
