# Spec Delta

## Purpose

Full-text index over the text of a project's tasks and transcripts (ADR-0040 phase 1, ADR-0012). The factory tool `search_history` (`factory-tools`) reads it; the iteration 6 UI search can reuse it.

## ADDED Requirements

### Requirement: Indexed text
The control plane SHALL keep a full-text index over the title and description of every task and chat, and over whole transcript messages: the visible text of each `user_message` event, and the text of all `agent_message_chunk` events of one agent message (same run and message id) joined in order. Hidden user content blocks, thoughts, tool calls, tool results, notices and activity events SHALL NOT be indexed. The index SHALL be updated in the transaction that stores the events.

#### Scenario: Agent message indexed
- **WHEN** the agent of `DF-31` writes "Retrying `foo` fixed the RBE failure"
- **THEN** a history search in `DF` for "RBE failure" finds that message

#### Scenario: Phrase across chunks
- **WHEN** the agent text "the RBE failure" arrives as chunks "the RB" and "E failure" of one message
- **THEN** a history search in `DF` for "RBE failure" finds the message, with the whole message text in the hit

#### Scenario: Tool output not indexed
- **WHEN** a tool result of `DF-31` contains the word "kubeconfig" and no message does
- **THEN** a history search in `DF` for "kubeconfig" finds nothing in `DF-31`

#### Scenario: Hidden instruction not indexed
- **WHEN** a user message of `DF-25-CHAT` holds the hidden `set_task_title` instruction
- **THEN** a history search for its wording finds nothing in `DF-25-CHAT`

#### Scenario: Duplicate chunk delivery
- **WHEN** the runtime delivers the same agent chunk twice
- **THEN** the indexed message holds its text once

### Requirement: Index backfill
The migration that adds the index SHALL index the messages of transcripts stored before it, under the same rules.

#### Scenario: Existing transcripts
- **WHEN** the migration runs on an instance with existing transcripts whose agent messages are split into chunks
- **THEN** their user and agent messages are found by a history search as whole messages

### Requirement: Search within one project
A history search SHALL return only tasks and messages of one project, including archived chats, ranked by relevance and then newest first. Each hit SHALL name the task key, task kind, title, run number, time, author and the matching text cut at 2 KiB with the matched words marked. For a forge message the author SHALL be the forge author with the `outside` flag.

#### Scenario: Other project
- **WHEN** project `OPS` has a message with "RBE failure" and a search in `DF` runs for it
- **THEN** the `OPS` message is not in the result

#### Scenario: Archived chat
- **WHEN** archived chat `DF-12-CHAT` contains "release train"
- **THEN** a search in `DF` for "release train" returns a hit with task kind `chat`

#### Scenario: Forge comment
- **WHEN** an outside author's issue comment delivered to `DF-31` matches the query
- **THEN** the hit names that forge author and marks them as outside

### Requirement: Secrets masked in history hits
Secret-rule matches (the rules of the push checks) in the title and text of a history hit SHALL be masked before the hit is returned.

#### Scenario: Token in a chat message
- **WHEN** a board member pasted a GitHub token into `DF-12-CHAT` and a search in `DF` matches that message
- **THEN** the hit shows the token masked
