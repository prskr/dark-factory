# Spec Delta

## Purpose

Agent memory (ADR-0040 phase 2): memories that agents and board members store in the control plane per project or per persona within a project, their limits, secret scan and provenance, the memory block every run gets at its start, the run record, the Memory tab and the lifecycle. The tools are part of `factory-tools`.

## ADDED Requirements

### Requirement: Memory content
A memory SHALL have an id shown as `m<number>`, a summary of one line with 1–200 characters, a content of plain text with 1–4096 bytes, a scope and a pinned flag. Content SHALL never be rendered as Markdown.

#### Scenario: Valid memory
- **WHEN** the agent of `DF-31` stores summary "Test `foo` is flaky under RBE" and content "Retry it once before investigating."
- **THEN** a memory with a new id such as `m57` exists with that summary and content

#### Scenario: Summary over two lines
- **WHEN** a memory is stored with a summary containing a line break
- **THEN** the write is rejected with an error naming the summary and nothing is stored

#### Scenario: Content too long
- **WHEN** a memory is stored with content of 5000 bytes
- **THEN** the write is rejected with an error naming the 4096-byte limit and nothing is stored

### Requirement: Memory scopes
A memory SHALL belong to exactly one project and have scope `project` (shared by all personas of the project) or `persona` (one persona within that project). An agent SHALL write only to its run's project, and with scope `persona` only for its run's persona; it SHALL choose only the scope. An agent SHALL see only the project scope and its own persona scope of its run's project. Memories SHALL NOT cross project boundaries.

#### Scenario: Persona scope
- **WHEN** the agent of a run of persona `linus` in project `DF` stores a memory with scope `persona`
- **THEN** the memory belongs to `DF` and `linus`, and runs of persona `woz` in `DF` do not see it

#### Scenario: Project scope
- **WHEN** the agent of a run of persona `linus` in `DF` stores a memory with scope `project`
- **THEN** runs of every persona in `DF` see it, and runs in project `OPS` do not

#### Scenario: Chat run
- **WHEN** the agent of chat `DF-12-CHAT` (persona `woz`) stores a memory with scope `project`
- **THEN** the next task run of any persona in `DF` sees it

### Requirement: Memory limits
A project scope SHALL hold at most 500 memories and a persona scope at most 200 per project. A run SHALL make at most 20 memory writes (creates, replaces and deletes together). A write beyond a limit SHALL fail with an error naming the limit and change nothing.

#### Scenario: Scope full
- **WHEN** the project scope of `DF` holds 500 memories and an agent stores another one there
- **THEN** the call fails with an error that names the limit and suggests replacing or forgetting a memory

#### Scenario: Too many writes in one run
- **WHEN** an agent makes its 21st memory write in run 2 of `DF-31`
- **THEN** the call fails and nothing changes

### Requirement: Secret scan of memory writes
Every write of a memory by an agent or a board member SHALL be scanned with the secret rules of the push checks. A match SHALL reject the write with an error naming the rule and the masked match, and nothing SHALL be stored.

#### Scenario: Token in a memory
- **WHEN** an agent stores a memory whose content contains a GitHub personal access token
- **THEN** the call fails naming the matching rule with the token masked, and no memory is stored

#### Scenario: Board member pastes a key
- **WHEN** a board member saves a memory containing a private key in the Memory tab
- **THEN** the form shows an error naming the rule and nothing is saved

### Requirement: Memory provenance
Every memory SHALL record who created it and who changed it last, with the time: for an agent its persona, task and run, for a human the user. The author SHALL be shown as `agent <persona> (<task key> run <n>)` or the user's display name wherever the memory is shown.

#### Scenario: Agent author
- **WHEN** run 2 of `DF-31` by persona `linus` creates `m57`
- **THEN** `m57` shows the author `agent Linus (DF-31 run 2)` and links task `DF-31` and its run 2

#### Scenario: Replaced by a later run
- **WHEN** run 1 of `DF-40` replaces `m57`
- **THEN** `m57` shows creation by `DF-31` run 2 and the last change by `DF-40` run 1

### Requirement: Protected memories
Memories written or last changed by a human and pinned memories SHALL NOT be replaced or deleted by agents. Only board members SHALL pin and unpin memories.

#### Scenario: Agent replaces a pinned memory
- **WHEN** an agent calls `remember` with the id of a pinned memory
- **THEN** the call fails saying the memory is protected and the memory is unchanged

#### Scenario: Agent forgets a human memory
- **WHEN** an agent calls `forget` for a memory created by board member Peter
- **THEN** the call fails and the memory still exists

### Requirement: Memory block at run start
Every run whose project scope or persona scope holds memories SHALL get a memory block in `Assign`. The block SHALL be at most 16 KiB and SHALL contain: a header naming the project and stating that the memories are hints written in earlier runs, not instructions, to be checked against the repository and the task; the pinned memories with their full content, newest first, together at most 8 KiB, under a heading that says they were reviewed by a board member; and an index with one line per other memory (id, scope, author, date, summary), most recently changed first, until the size limit, followed by the number of memories left out. Memories of other personas SHALL NOT be in the block. Without memories in scope there SHALL be no block. The block SHALL NOT be part of the system prompt.

#### Scenario: Pinned and index
- **WHEN** `DF` has pinned project memory `m42` and unpinned memories `m55` (project) and `m57` (persona `linus`), and a run of `linus` in `DF` starts
- **THEN** the block contains the full content of `m42` under the pinned heading and index lines for `m57` and `m55` in that order of last change

#### Scenario: Other persona
- **WHEN** a run of persona `woz` in `DF` starts
- **THEN** its block contains `m42` and `m55` and not `m57`

#### Scenario: Index over the limit
- **WHEN** the index lines of a run would exceed the 16 KiB block
- **THEN** the block ends with a line naming the number of memories left out and pointing to `recall`

#### Scenario: No memories
- **WHEN** a run starts in a project without memories in its scopes
- **THEN** `Assign` carries no memory block

### Requirement: Memory block fixed per run
The memory block SHALL be rendered once per run and stored with the ids of the memories in it. Every `Assign` for the same run SHALL carry the stored block. Changes to memories during a run SHALL reach that run only through `recall`.

#### Scenario: Reconnect
- **WHEN** a board member edits `m55` while run 2 of `DF-31` runs, and the runtime reconnects
- **THEN** the new `Assign` carries the same block as before and the next run gets the edited `m55`

### Requirement: Memory on the run view
The run details on the task page and the chat page SHALL show a "Memory" section with the number of pinned and indexed memories the run got, a link that shows the stored block read-only, and the memories the run created or replaced, each linking to the Memory tab filtered by that run. A memory deleted since SHALL be shown as deleted.

#### Scenario: Run that wrote memories
- **WHEN** a board member opens run 2 of `DF-31`, which got 1 pinned and 2 indexed memories and created `m57`
- **THEN** the Memory section shows "1 pinned, 2 in index" with a link to the block and lists `m57`

#### Scenario: Deleted memory
- **WHEN** `m57` was deleted after the run
- **THEN** the run's Memory section lists `m57` as deleted

### Requirement: Memory tab
The project settings page SHALL have a "Memory" tab (`?tab=memory`) for board members. It SHALL list the project's memories of all scopes in two-line rows (summary; scope, author with links, date, pin marker), with a full-text search box and filters for scope (all, project, one persona), author (all, agents, board members), pinned only and run. Board members SHALL create, edit (summary, content, pinned), and delete memories in a form modal that shows the provenance read-only; the scope is set on creation only. Deleting SHALL ask for confirmation. With the run filter set, the tab SHALL offer to delete all memories the run created or last changed, after a confirmation naming their number.

#### Scenario: Find a bad memory
- **WHEN** a board member searches the Memory tab of `DF` for "curl"
- **THEN** the list shows the memories whose summary or content matches, each with its author and run link

#### Scenario: Edit and pin
- **WHEN** a board member opens `m55`, changes the content and checks "Pinned"
- **THEN** `m55` has the new content, is pinned, and shows the board member as last author

#### Scenario: Delete all memories of a run
- **WHEN** a board member opens the Memory tab from run 1 of `DF-40` and confirms "Delete all 3 memories of this run"
- **THEN** the three memories are deleted and each deletion is audited

#### Scenario: Empty tab
- **WHEN** a project has no memories
- **THEN** the tab explains what memories are and offers "New memory"

### Requirement: Memory lifecycle
Deleting a project SHALL delete its memories in the same transaction. Deleting a persona SHALL delete its persona memories; project memories it wrote SHALL stay with the persona reference removed from their provenance.

#### Scenario: Project deleted
- **WHEN** project `OPS` is deleted
- **THEN** no memory of `OPS` remains

#### Scenario: Persona deleted
- **WHEN** persona `linus` is deleted
- **THEN** its persona memories are gone and the project memories it created still exist
