# Spec Delta

## MODIFIED Requirements

### Requirement: Desktop navigation rail
At widths of 768 px and more, every authenticated page SHALL show a navigation rail with the logo, a new-task action, links to Chats, Tasks, Projects, Personas and Credentials, and a log-out action. The links SHALL show icons only; each section's name SHALL be its accessible name and its tooltip. The current section SHALL be marked visually with a bar at the rail's edge and with `aria-current`.

#### Scenario: Rail on the task page
- **WHEN** a board member opens `/tasks` at 1440 px width
- **THEN** the rail shows the new-task action, Chats, Tasks marked as current, Projects, Personas, Credentials and log out

#### Scenario: Rail on the chat page
- **WHEN** a board member opens `/chats` at 1440 px width
- **THEN** the rail shows Chats above Tasks, marked as current

#### Scenario: Section names as tooltips
- **WHEN** the rail is rendered
- **THEN** no section name is shown as visible text, and each section link has its name as accessible name and title

#### Scenario: Rail in a split-screen window
- **WHEN** a board member opens `/tasks` at 900 px width
- **THEN** the rail is shown and no bottom bar

### Requirement: Phone bottom bar
Below 768 px, every authenticated page SHALL show a bottom bar instead of the rail. The bar SHALL hold only navigation entries, evenly distributed across its width: Chats, Tasks, Requests (with the pending count) and Projects. Neither the bottom bar nor the floating create button SHALL appear while a task's or a chat's transcript is shown; its message input takes the place.

#### Scenario: Task list on a phone
- **WHEN** the task list is opened at 390 px width
- **THEN** a bottom bar with Chats, Tasks, Requests and Projects is shown, a floating button "New task" sits above it at the bottom right, and no rail is shown

#### Scenario: Chat list on a phone
- **WHEN** the chat list is opened at 390 px width
- **THEN** the bottom bar marks Chats as current and the floating button is "New chat"

#### Scenario: Entries evenly distributed
- **WHEN** the bottom bar is rendered at 390 px width
- **THEN** its entries have equal widths and the bar contains no create action

#### Scenario: Transcript on a phone
- **WHEN** a task is opened at 390 px width
- **THEN** neither the bottom bar nor the floating button is shown and the message input sits at the bottom of the screen
