# Spec Delta

## MODIFIED Requirements

### Requirement: Conversation layout
The transcript SHALL read as a conversation: human messages aligned to the right, agent messages to the left, each with author name and time below it. The author of an agent message SHALL be the persona's display name with its `@handle`, and its avatar the persona's generated avatar. Consecutive messages of one author SHALL show the avatar only once, at the last message. Tool calls SHALL be one-line rows between the messages with a tool icon in front, an expand chevron at the end and a thin separator line above and below, shared by consecutive tool calls. The start of each run SHALL be marked with a divider naming the run.

#### Scenario: Agent writes twice
- **WHEN** the agent sends two messages in a row, with a tool call between them
- **THEN** both agent messages are on the left, only the second shows the avatar, and the tool call is a collapsed one-line row between them

#### Scenario: Persona as author
- **WHEN** the agent of a task with persona `reviewer` ("Code Reviewer") sends a message
- **THEN** the message shows author "Code Reviewer @reviewer" and the avatar "CR"

#### Scenario: Second run
- **WHEN** run #2 of a task starts
- **THEN** the transcript shows a divider "Run #2 started" with its start time before the run's first event

#### Scenario: Tool call row
- **WHEN** the transcript shows a tool call
- **THEN** the row starts with the tool icon, ends with a chevron and has a separator line above and below

#### Scenario: Consecutive tool calls
- **WHEN** two tool calls follow each other, also when the second arrives while the page is open
- **THEN** a single separator line is between them

#### Scenario: Expand a tool call
- **WHEN** a board member opens a tool call
- **THEN** the chevron points the other way and the input and output of the call are shown

## ADDED Requirements

### Requirement: Persona on task rows and in the task details
Each task list row SHALL show the task persona's avatar before the time, with the display name as its label. In the task details, the persona value SHALL show the avatar, the display name and the `@handle`, and no longer the harness. The persona value keeps linking to `/personas`.

#### Scenario: Task row
- **WHEN** task `DF-21` with persona `linus` ("Linus") is listed
- **THEN** its row shows the avatar "L" labelled "Linus"

#### Scenario: Task details
- **WHEN** a board member opens `DF-21`
- **THEN** the persona value shows the avatar "L", "Linus" and "@linus" and links to `/personas`
