# Spec Delta

## Purpose

Full-text index over the text of a project's tasks and transcripts (ADR-0040 phase 1, ADR-0012). The factory tool `search_history` (`factory-tools`) reads it; the iteration 6 UI search can reuse it.

## ADDED Requirements

### Requirement: Indexed text
The control plane SHALL keep a full-text index over the title and description of every task and chat, and over the text of every `UserMessage` and `AgentMessage` transcript event. Thoughts, tool calls and tool results SHALL NOT be indexed. Events stored before this change SHALL be indexed by the migration.

#### Scenario: Agent message indexed
- **WHEN** the agent of `DF-31` writes "Retrying `foo` fixed the RBE failure"
- **THEN** a history search in `DF` for "RBE failure" finds that message

#### Scenario: Tool output not indexed
- **WHEN** a tool result of `DF-31` contains the word "kubeconfig" and no message does
- **THEN** a history search in `DF` for "kubeconfig" finds nothing in `DF-31`

#### Scenario: Existing transcripts
- **WHEN** the migration runs on an instance with existing transcripts
- **THEN** their user and agent messages are found by a history search

### Requirement: Search within one project
A history search SHALL return only tasks and messages of one project, including archived chats, ranked by relevance and then newest first. Each hit SHALL name the task key, task kind, title, run number, time, author and the matching text cut at 2 KiB with the matched words marked.

#### Scenario: Other project
- **WHEN** project `OPS` has a message with "RBE failure" and a search in `DF` runs for it
- **THEN** the `OPS` message is not in the result

#### Scenario: Archived chat
- **WHEN** archived chat `DF-12-CHAT` contains "release train"
- **THEN** a search in `DF` for "release train" returns a hit with task kind `chat`
