# Spec Delta

## Purpose

Defines the project context: human-approved planning knowledge per project that every chat in the project receives. Covers editing, the size limit, injection into chat runs, and proposals by agents through approval requests (ADR-0026, ADR-0040).

## ADDED Requirements

### Requirement: Project context text
Every project SHALL have a project context: Markdown text, empty by default, at most 32 KiB (32,768 bytes of UTF-8). A longer text SHALL be rejected with a validation error. The project context SHALL NOT be written into any workspace.

#### Scenario: New project
- **WHEN** a board member creates project `DF`
- **THEN** its project context is empty

#### Scenario: Too long
- **WHEN** a board member saves a project context of 40,000 bytes
- **THEN** the form shows a validation error and the stored context is unchanged

### Requirement: Editing the project context
The project edit form SHALL have a Context tab with a textarea for the project context, saved with the other tabs of the form. Only board members SHALL edit it. A change SHALL be audited like other project changes, with the text before and after.

#### Scenario: Board member edits the context
- **WHEN** a board member writes "Release train: every second Tuesday" into the Context tab of `DF` and saves
- **THEN** the context of `DF` is that text and an audit entry names the board member with the text before and after

#### Scenario: Opened from a chat
- **WHEN** a board member follows the project context link of `DF-12-CHAT`
- **THEN** the edit form of `DF` opens on the Context tab

### Requirement: Chat runs receive the project context
A chat run SHALL receive its project's context, as read when the run starts, in its system prompt under the heading `## Project context`. An empty context SHALL be left out, heading included. An edit SHALL apply from the next run on. Task runs SHALL NOT receive the project context.

#### Scenario: Edit during a run
- **WHEN** a board member edits the context of `DF` while a run of `DF-12-CHAT` is active
- **THEN** that run keeps the old text and the next run of `DF-12-CHAT` receives the new one

#### Scenario: Task run
- **WHEN** a run of task `DF-20` in project `DF` with a non-empty context starts
- **THEN** its system prompt contains no project context

#### Scenario: Empty context
- **WHEN** a run of `DF-12-CHAT` starts and the context of `DF` is empty
- **THEN** its system prompt contains no `## Project context` heading

### Requirement: Project context proposals
The approval type `project-context` SHALL carry the full proposed text as `content` and offer the single scope `project`. Only chat runs SHALL raise it. The request SHALL record the version of the context the proposing run received. A `content` over 32 KiB SHALL be rejected.

#### Scenario: Chat proposes
- **WHEN** the agent of `DF-12-CHAT` calls `request_approval` with type `project-context`, `content` "Release train: every Tuesday" and reason "decided in this chat"
- **THEN** a pending `project-context` request on `DF-12-CHAT` exists and the context of `DF` is unchanged

#### Scenario: Task proposes
- **WHEN** the agent of task `DF-20` calls `request_approval` with type `project-context`
- **THEN** the call returns a tool error and nothing is stored

#### Scenario: Same text twice
- **WHEN** the agent of `DF-12-CHAT` proposes the same `content` twice while the first proposal is pending
- **THEN** one pending request exists

### Requirement: Approving a proposal
Approving a `project-context` request SHALL replace the project context with its `content` and audit the change with the deciding board member. If the context changed after the version the request recorded, approving SHALL fail with "context changed since this proposal", and the request SHALL stay pending until it is denied.

#### Scenario: Approve
- **WHEN** a board member approves the pending proposal of `DF-12-CHAT`
- **THEN** the context of `DF` is the proposed text and `DF-12-CHAT` receives a `request.decided` message

#### Scenario: Context changed in between
- **WHEN** `DF-12-CHAT` and `DF-14-CHAT` each proposed a context in runs that received the same version, and the proposal of `DF-12-CHAT` was approved
- **THEN** approving the proposal of `DF-14-CHAT` fails with "context changed since this proposal" and the context stays as approved

#### Scenario: Second proposal in the same run
- **WHEN** a run of `DF-12-CHAT` proposes twice, and the first proposal is approved before the second is decided
- **THEN** approving the second fails with "context changed since this proposal"

### Requirement: Proposals in the inbox
A `project-context` request SHALL show the reason and a unified line diff from the current project context to the proposed text, rendered as escaped text.

#### Scenario: Diff shown
- **WHEN** a board member opens a `project-context` request that changes one line of the context of `DF`
- **THEN** the card shows that line as removed and its replacement as added, with the unchanged lines around it

#### Scenario: Markup in the proposal
- **WHEN** the proposed text contains `<script>alert(1)</script>`
- **THEN** the diff shows it as text
