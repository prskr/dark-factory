# Spec Delta

## Purpose

Defines the web pages for chats: choosing a project and a chat, following and steering the conversation, reading its subtasks and artifacts, starting new chats and browsing archived ones, on desktop and phone.

## ADDED Requirements

### Requirement: Chat pages
The chat list of a project SHALL be served at `/chats?project=<KEY>` and a chat at `/chats/<CHAT KEY>`. `/chats` without a project SHALL show the project of the most recently active chat, or the first project if no chat exists.

#### Scenario: Open a chat
- **WHEN** a board member opens `/chats/DF-12-CHAT`
- **THEN** the page shows project `DF` selected, `DF-12-CHAT` selected in the chat list and its transcript

#### Scenario: Default project
- **WHEN** the most recently active chat is `WEB-4-CHAT` and a board member opens `/chats`
- **THEN** project `WEB` is selected

### Requirement: Project and chat list
At desktop widths the left column SHALL list the projects with their number of chats and a "needs you" marker, and below them the selected project's chats that are not archived. Each chat entry SHALL show the persona, its status unless the chat is `waiting`, the time of the last activity, the topic and the last message. The column SHALL end with an entry to the archived chats with their count.

#### Scenario: Idle chat without badge
- **WHEN** `DF-12-CHAT` is `waiting` after a reply
- **THEN** its entry shows no status badge

#### Scenario: Running chat
- **WHEN** `DF-12-CHAT` is `running`
- **THEN** its entry shows the badge "Running"

#### Scenario: Topic not set yet
- **WHEN** the agent of a new chat has not set the topic
- **THEN** the entry shows the start of the first message in italics and "<persona> sets the topic when the run starts"

### Requirement: Chats that need you
A chat SHALL need you while it has an open approval request. Chats that need you SHALL be listed first and marked, the other chats SHALL follow by last activity, newest first, and a project SHALL be marked while one of its chats needs you.

#### Scenario: Open approval request
- **WHEN** `DF-16-CHAT` has a pending approval request and `DF-12-CHAT` had activity more recently
- **THEN** `DF-16-CHAT` is listed first with the marker and project `DF` carries the marker

### Requirement: Collapsible chat list
At desktop widths a board member SHALL be able to collapse the left column to a narrow strip with one avatar per persona that has a chat in the project. Selecting an avatar SHALL open that persona's chat that needs you, or otherwise its most recently active chat.

#### Scenario: Persona with a chat that needs you
- **WHEN** Linus has chats `DF-12-CHAT` (most recent) and `DF-19-CHAT` (open approval request) and the board member selects Linus in the collapsed strip
- **THEN** `DF-19-CHAT` opens

### Requirement: Chat header and transcript
The center SHALL show a header with only the action "New chat", the transcript with the same rendering as on the task page, and the message box addressed to the persona. The transcript SHALL show tool calls of `create_task` with the created task's key, title and persona.

#### Scenario: Delegation in the transcript
- **WHEN** the agent of `DF-12-CHAT` created `DF-20` for Linus
- **THEN** the transcript shows a `create_task` row naming `DF-20`, its title and Linus

### Requirement: Compaction notice in the transcript
A `context_compacted` notice SHALL be shown like other notices, one line with the bot icon, expandable to the summary the run was seeded with.

#### Scenario: Expand the summary
- **WHEN** a board member expands the `context_compacted` notice of `DF-12-CHAT`
- **THEN** the summary text is shown

### Requirement: Details tabs
The right column SHALL have the tabs Overview, Subtasks and Artifacts, the latter two with counts. Overview SHALL show the persona with its description, the current activity, and Status with an Archive action, Topic, Project, Persona (harness and model), Key, Checkout and the runs. Archive SHALL be disabled while archiving is not allowed.

#### Scenario: Archive disabled while running
- **WHEN** `DF-12-CHAT` is `running`
- **THEN** the Archive action is shown disabled with the reason

#### Scenario: Subtasks tab
- **WHEN** `DF-12-CHAT` created `DF-20` (running) and `DF-21` (awaiting dependencies)
- **THEN** the Subtasks tab lists both with their status and shows "0 of 2 done"

### Requirement: Collapsible details
A board member SHALL be able to collapse the details column to a strip as on the task page.

#### Scenario: Collapse details
- **WHEN** a board member hides the details of `DF-12-CHAT`
- **THEN** the details column becomes a strip and the transcript gets the freed width

### Requirement: Artifacts tab
The Artifacts tab SHALL list the chat's artifacts as cards with a preview, the name, the time, the size and the type, newest first, and scroll when they don't fit.

#### Scenario: Many artifacts
- **WHEN** `DF-12-CHAT` has seven artifacts
- **THEN** the tab shows the count 7 and the cards scroll below the fixed tab bar

### Requirement: Artifact split view
Opening a Markdown artifact from the transcript or the Artifacts tab SHALL show it rendered next to the transcript, with a divider the board member can drag to change the split, a Preview/Source toggle and a close action.

#### Scenario: Open from the transcript
- **WHEN** a board member opens `task-dependencies-plan.md` from its card in the transcript
- **THEN** the center splits into transcript and the rendered artifact

#### Scenario: Resize
- **WHEN** the board member drags the divider to the left
- **THEN** the transcript gets narrower and the artifact wider

### Requirement: New chat dialog
"New chat" SHALL open a dialog with the selected project, not changeable, a message field and a persona picker listing every persona with harness, model and description. Sending SHALL start the chat and open it.

#### Scenario: Start from the dialog
- **WHEN** a board member picks Grace, writes a message and sends it
- **THEN** the new chat is selected, listed first and its transcript shows the message

### Requirement: Archive modal
The archived chats entry SHALL open a modal listing the project's archived chats with persona, topic, key, last message, number of artifacts and archive date, filtered by a search over topic, persona and messages. Selecting a chat SHALL turn the list into a sidebar and show the chat with the tabs Transcript and Artifacts and the action Unarchive. The transcript SHALL be read-only.

#### Scenario: Search
- **WHEN** a board member types "helm" into the search of the archive modal
- **THEN** only archived chats whose topic, persona or messages contain "helm" are listed

#### Scenario: Unarchive from the modal
- **WHEN** a board member unarchives `DF-24-CHAT` in the modal
- **THEN** the modal closes and `DF-24-CHAT` is opened in the chat view

### Requirement: Artifacts of an archived chat
The Artifacts tab of the archive modal SHALL show the chat's artifacts as cards, several per row. Opening one SHALL show it in place of the cards, with a back action to the cards.

#### Scenario: Open an archived artifact
- **WHEN** a board member opens `triage-summary.md` of archived `DF-3-CHAT`
- **THEN** the rendered document replaces the cards and a back action returns to them

### Requirement: Empty chat list
A project without chats SHALL show an empty state with a short explanation of chats and the action "New chat".

#### Scenario: No chats yet
- **WHEN** a board member selects project `DOC` without chats
- **THEN** the center shows "No chats in Docs yet" and the action "New chat"

### Requirement: Chats on phones
Below 768 px the chat list SHALL be a full-screen page with a project picker, the chats and the archived entry; a chat SHALL open full-screen with persona, status and topic in the header; the details SHALL open as a bottom sheet with the three tabs; an artifact SHALL open full-screen; new chat SHALL open as a bottom sheet; the archive SHALL be a full-screen list and chat.

#### Scenario: Open a chat on a phone
- **WHEN** a board member selects `DF-12-CHAT` at 390 px width
- **THEN** the chat fills the screen with a back action to the list and the message box at the bottom

#### Scenario: Artifact on a phone
- **WHEN** a board member opens an artifact at 390 px width
- **THEN** it fills the screen with a back action to the chat
