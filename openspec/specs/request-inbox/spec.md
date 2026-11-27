# request-inbox Specification

## Purpose

Lets humans see every pending question and approval request and decide it quickly, on a phone or a desktop (ADR-0038, ADR-0035).

## Requirements

### Requirement: Pending requests page
The UI SHALL list all pending requests across tasks, newest first, with task key and title, kind, type, the agent's text or reason and the time raised, and SHALL update live when requests change.

#### Scenario: New request appears
- **WHEN** an agent raises a question while a board member has the inbox open
- **THEN** the question appears without a page reload

### Requirement: Pending count in the navigation
The navigation SHALL show the number of pending requests on every page and update it live.

#### Scenario: Count after a decision
- **WHEN** one of three pending requests is decided
- **THEN** the navigation shows 2

### Requirement: Requests on the task timeline
The task detail SHALL show each request of the task as a card on the timeline at the time it was raised, with its state, decision, decider and the actions for pending requests. A request card SHALL be visually distinct from messages and tool calls in both the light and the dark theme.

#### Scenario: Decided request in history
- **WHEN** a board member opens a task whose question was answered yesterday
- **THEN** the timeline shows the question card with the answer and who gave it

#### Scenario: Card stands out in the light theme
- **WHEN** a board member opens a task with a pending question in the light theme
- **THEN** the question card is set apart from the surrounding messages by the styling chosen in the request card design

### Requirement: Agent text is untrusted
Request text, reasons and payload values SHALL be rendered escaped, Markdown without raw HTML, and links SHALL NOT be followed automatically.

#### Scenario: Script in a reason
- **WHEN** an approval reason contains `<script>alert(1)</script>`
- **THEN** it is shown as text

### Requirement: Phone-ready inbox
The inbox and request cards SHALL be usable at 390 px width and appear in the mobile navigation.

#### Scenario: Phone navigation
- **WHEN** the UI is opened at 390 px width
- **THEN** the navigation contains the inbox with its pending count

### Requirement: Deny always
A pending `network-access` request SHALL offer "Deny always" next to approve and deny, with the same optional comment and permission as deny. It SHALL deny the request and create a deny rule for exactly the requested host in one step. Other pending requests matching that host SHALL then be denied as for any new deny rule.

#### Scenario: Deny always from the inbox
- **WHEN** a board member chooses "Deny always" on the request of `DF-7` for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** the request is `denied` by the board member, deny rule `http-intake.logs.us5.datadoghq.com` exists and `DF-7` receives a `request.decided` message

#### Scenario: Exact host only
- **WHEN** a board member chooses "Deny always" on a request for `http-intake.logs.us5.datadoghq.com:443`
- **THEN** the deny rule is `http-intake.logs.us5.datadoghq.com`, not `*.datadoghq.com`

#### Scenario: Phone
- **WHEN** a board member opens a `network-access` request at 390 px width
- **THEN** "Deny always" is offered

### Requirement: Requests from chats
A request raised by a chat run SHALL be listed like any other and link to the chat page instead of the task page.

#### Scenario: Approval request from a chat
- **WHEN** the agent of `DF-12-CHAT` requests an approval
- **THEN** the inbox lists it and its link opens `/chats/DF-12-CHAT`

### Requirement: Deciding a request from the UI
For a pending question the UI SHALL offer the input of its kind ("Answer input by kind") and one "Answer" action that answers without another confirmation step. For an approval request it SHALL offer approve with a scope chosen from the type's scopes (preselected: the narrowest) and deny with an optional comment.

#### Scenario: Single choice on a phone
- **WHEN** a board member taps the option "PostgreSQL 17" of a single-choice question and then "Answer" at 390 px width
- **THEN** the question is answered with the option `pg17` without another confirmation step

#### Scenario: Approval on a phone
- **WHEN** a board member taps "Approve" on a pending `network-access` request at 390 px width
- **THEN** the request is approved with the preselected scope

### Requirement: One question card
A question SHALL be shown by the same card in the inbox, on the task timeline and on the chat timeline, with the parts in this order: header ("Question · " and the kind, Blocking while pending, task), question text, input or answer, field error, "Answer" action, time. Only the input and the answer SHALL differ by kind.

#### Scenario: Same card in inbox and timeline
- **WHEN** a single-choice question of `DF-7` is pending and a board member opens `/requests` and then `DF-7`
- **THEN** both show the card with "Question · Single choice", the question, the options and "Answer"

#### Scenario: Same card in a chat
- **WHEN** the agent of `DF-12-CHAT` asks a multiple-choice question and a board member opens `/chats/DF-12-CHAT`
- **THEN** the chat timeline shows the card with "Question · Multiple choice", the checkboxes and "Answer"

### Requirement: Question text rendered as Markdown
The question text SHALL be rendered as Markdown under "Agent text is untrusted". Option labels and descriptions SHALL be shown as plain text.

#### Scenario: Markdown in the question
- **WHEN** a question text holds a list and inline code
- **THEN** the card shows the rendered list and code

#### Scenario: Markdown in a label
- **WHEN** an option label is `**bold** <b>x</b>`
- **THEN** the card shows the label exactly as written

### Requirement: Answer input by kind
A single-choice question SHALL offer its options as radio buttons, a multiple-choice question as checkboxes with a hint naming the limits above the options; both SHALL offer "Other" as the last option, with a text input (placeholder "Your Other answer") when allowed, and preselect nothing. A text question SHALL offer a Markdown field labelled "Your answer" with "Write" and "Preview" tabs and no syntax highlighting.

#### Scenario: Multiple choice with a limit
- **WHEN** a board member opens a multiple-choice question with `min_selected: 1` and `max_selected: 3`
- **THEN** the card shows a checkbox per option, an "Other" checkbox and the hint "Choose 1 to 3"

#### Scenario: Multiple choice with defaults
- **WHEN** a board member opens a multiple-choice question without limits
- **THEN** the card shows the hint "Choose one or more"

#### Scenario: Text answer preview
- **WHEN** a board member writes `**now**` in a text question's answer and opens "Preview"
- **THEN** the preview shows "now" in bold and the highlighting editor is not loaded

#### Scenario: Other chosen
- **WHEN** a board member checks "Other" on a single-choice question
- **THEN** a text input for the Other answer is shown below "Other" and focused

#### Scenario: Other text without Other chosen
- **WHEN** the card's script is not running and a board member checks `pg16`, types "maybe 15" into the Other input and answers
- **THEN** the question is answered with `pg16` and no Other text

### Requirement: Answer errors on the card
An answer the server rejects SHALL be shown again on its card with the submitted choices and text kept and the error directly below the input, marked invalid for assistive technology. The question SHALL stay pending.

#### Scenario: Nothing chosen
- **WHEN** a board member presses "Answer" on a single-choice question without choosing
- **THEN** the card shows "Choose one option." below the options and the question stays pending

#### Scenario: Too many checked
- **WHEN** a board member checks four options of a question with `max_selected: 3` and answers
- **THEN** the card shows "Choose at most 3 options." with the four options still checked

#### Scenario: Other chosen without text
- **WHEN** a board member checks "Other", leaves its input empty and answers
- **THEN** the card shows "Write your Other answer." below the input and the question stays pending

### Requirement: Answered question in history
A decided question card SHALL show the question, its kind, every option in order with the chosen ones marked (icon and accessible text), the Other text when given, or the text answer rendered as Markdown under "Agent text is untrusted" (it may come from an agent), and "Answered by <name> · <time>".

#### Scenario: Answered multiple choice
- **WHEN** a board member opens a task whose multiple-choice question was answered with `api` and Other "also the CLI"
- **THEN** the card lists all options with `api` marked chosen, shows "Other: also the CLI" and "Answered by Peter Kurfer · <time>"

### Requirement: Answer drafts survive live updates
A live update of the inbox, the task timeline or the chat timeline SHALL keep the choices and text typed into a pending card while its question stays pending and unchanged, also when the card is sent again (a reconnect) or other parts of it change. A card whose question was decided or closed elsewhere SHALL show that on the timeline and leave the inbox. A card decided in the inbox SHALL show the decision there until the page is reloaded.

#### Scenario: New request while typing in the inbox
- **WHEN** a board member has written half an answer to a text question in the inbox and another agent raises a request
- **THEN** the new request appears and the written text is still in the field

#### Scenario: New request while typing on the timeline
- **WHEN** a board member has checked two options of a pending question on `DF-7` and the agent of `DF-7` raises another request
- **THEN** the new card appears and both options are still checked

#### Scenario: Decided elsewhere while typing
- **WHEN** a board member is writing an answer on `DF-7` and another board member answers the same question
- **THEN** the card on `DF-7` shows the other board member's answer instead of the input

#### Scenario: Answered in the inbox
- **WHEN** a board member answers a question in the inbox and the live update takes it out of the pending list
- **THEN** the card stays in the inbox with "Answered by" and the answer

#### Scenario: Stream reconnects while typing
- **WHEN** a board member has written half an answer on `DF-7` and the live connection of the page drops and reconnects
- **THEN** the card is still there with the written text and the open tab

#### Scenario: Card changes while typing
- **WHEN** a board member has checked an option of a pending question on `DF-7` and `DF-7` is renamed
- **THEN** the card shows the new title and the option is still checked

#### Scenario: Rejected answer after a live update
- **WHEN** a new request arrived while a board member was typing, and the board member then submits an answer the server rejects
- **THEN** the card shows the error below the input with the typed text kept

### Requirement: Choice targets on phones
Below 1024 px every option, "Other" and "Answer" SHALL be at least 44 px high, and the whole option row SHALL toggle its control.

#### Scenario: Tap on the label
- **WHEN** a board member taps the label text of an option at 390 px width
- **THEN** the option's radio is checked
