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
- **WHEN** the runtime delivers the chunk with `seq` 7 of run 2 twice
- **THEN** the indexed message holds its text once, because a chunk is identified by its event's run and `seq`

#### Scenario: Two chunks with the same text
- **WHEN** chunks `seq` 7 and `seq` 8 of one message both hold "ha"
- **THEN** the indexed message holds "haha"

### Requirement: Oversized messages
A message of any size and any characters SHALL be stored complete and SHALL NOT fail the transaction that stores its events or the migration. Only its first 32,768 characters SHALL be indexed.

#### Scenario: Pasted log of 3 MiB
- **WHEN** a board member's message of 3 MiB is stored in `DF-31`
- **THEN** the events are stored, the message text is complete, and a search finds words from its first 32,768 characters

#### Scenario: Multibyte text with many distinct words
- **WHEN** a message of 1 MiB consists of distinct words of 4-byte characters
- **THEN** the events are stored and the message is found by a word from its beginning

### Requirement: Index backfill
The migration that adds the index SHALL index the messages of transcripts stored before it, under the same rules.

#### Scenario: Existing transcripts
- **WHEN** the migration runs on an instance with existing transcripts whose agent messages are split into chunks
- **THEN** their user and agent messages are found by a history search as whole messages

#### Scenario: Oversized message before the migration
- **WHEN** a message of 3 MiB was stored before the migration
- **THEN** the migration succeeds and the message is found by a word from its beginning

### Requirement: Search within one project
A history search SHALL return only tasks and messages of one project, including archived chats, ranked by relevance and then newest first. Each hit SHALL name its source (`message` or `task`), task key, task kind, title, time, author and the matching text cut at 2 KiB with the matched words marked.
#### Scenario: Other project
- **WHEN** project `OPS` has a message with "RBE failure" and a search in `DF` runs for it
- **THEN** the `OPS` message is not in the result

#### Scenario: Archived chat
- **WHEN** archived chat `DF-12-CHAT` contains "release train"
- **THEN** a search in `DF` for "release train" returns a hit with task kind `chat`

#### Scenario: Forge comment not searchable
- **WHEN** forge user `mallory` comments "rotate the staging key" on the pull request of `DF-31`, which wakes `DF-31`, and the agent reads it with `read_pull_request_feedback`
- **THEN** a search in `DF` for "rotate the staging key" finds nothing from that comment, because only the hidden wake-up message and the tool result hold it

### Requirement: Hit shapes
A `message` hit SHALL carry the run number and the message's time, and its text SHALL be the message. A `task` hit (the title or description matched) SHALL carry no run number; its time SHALL be the task's creation time, its author the task's creator, and its text the title and description.

#### Scenario: Task without runs
- **WHEN** task `DF-50` has never run and its description contains "release train"
- **THEN** a search in `DF` for "release train" returns a `task` hit for `DF-50` without a run number, with its creation time, its creator and its description as text

#### Scenario: Message hit
- **WHEN** run 2 of `DF-31` has an agent message matching the query
- **THEN** the hit has source `message`, run 2, the message's time and the message as text

### Requirement: Secrets masked in history hits
Secret-rule matches (the rules of the push checks) SHALL be masked in the full title and the full message or task text of a history hit, before the text is cut to 2 KiB and the matched words are marked. The search SHALL hand the full text to the masking step; no excerpt SHALL be built from unmasked text.

#### Scenario: Token in a chat message
- **WHEN** a board member pasted a GitHub token into `DF-12-CHAT` and a search in `DF` matches that message
- **THEN** the hit shows the token masked

#### Scenario: Token across the cut
- **WHEN** a matching message is 5 KiB long and a GitHub token starts 10 bytes before the 2 KiB cut
- **THEN** the hit holds no unmasked part of the token
