## REMOVED Requirements

### Requirement: Deciding from the UI
**Reason**: Suggested answers as one-tap buttons and the free-text input are replaced by an input per question kind.
**Migration**: See "Deciding a request from the UI" and "Answer input by kind".

## ADDED Requirements

### Requirement: Deciding a request from the UI
For a pending question the UI SHALL offer the input of its kind ("Answer input by kind") and one "Answer" action that answers without another confirmation step. For an approval request it SHALL offer approve with a scope chosen from the type's scopes (preselected: the narrowest) and deny with an optional comment.

#### Scenario: Single choice on a phone
- **WHEN** a board member taps the option "PostgreSQL 17" of a single-choice question and then "Answer" at 390 px width
- **THEN** the question is answered with the option `pg17` without another confirmation step

#### Scenario: Approval on a phone
- **WHEN** a board member taps "Approve" on a pending `network-access` request at 390 px width
- **THEN** the request is approved with the preselected scope

### Requirement: One question card
A question SHALL be shown by the same card in the inbox and on the task timeline, with the parts in this order: header ("Question · " and the kind, Blocking while pending, task), question text, input or answer, field error, "Answer" action, time. Only the input and the answer SHALL differ by kind.

#### Scenario: Same card in inbox and timeline
- **WHEN** a single-choice question of `DF-7` is pending and a board member opens `/requests` and then `DF-7`
- **THEN** both show the card with "Question · Single choice", the question, the options and "Answer"

### Requirement: Question text rendered as Markdown
The question text SHALL be rendered as Markdown under "Agent text is untrusted". Option labels and descriptions SHALL be shown as plain text.

#### Scenario: Markdown in the question
- **WHEN** a question text holds a list and inline code
- **THEN** the card shows the rendered list and code

#### Scenario: Markdown in a label
- **WHEN** an option label is `**bold** <b>x</b>`
- **THEN** the card shows the label exactly as written

### Requirement: Answer input by kind
A single-choice question SHALL offer its options as radio buttons, a multiple-choice question as checkboxes with a hint naming the limits; both SHALL offer "Other" with a text input when allowed, and preselect nothing. A text question SHALL offer a Markdown field with "Write" and "Preview" tabs and no syntax highlighting.

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
- **THEN** a text input for the Other answer is shown and focused

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
A decided question card SHALL show the question, its kind, every option in order with the chosen ones marked (icon and accessible text), the Other text when given, or the text answer rendered as Markdown, and who answered and when.

#### Scenario: Answered multiple choice
- **WHEN** a board member opens a task whose multiple-choice question was answered with `api` and Other "also the CLI"
- **THEN** the card lists all options with `api` marked chosen, shows "Other: also the CLI" and who answered

### Requirement: Answer drafts survive live updates
A live update of the inbox or the task timeline SHALL send a card again only when its content changed, so choices and text in an unchanged pending card are kept. A card whose question was decided elsewhere SHALL show the decision on the timeline and leave the inbox.

#### Scenario: New request while typing in the inbox
- **WHEN** a board member has written half an answer to a text question in the inbox and another agent raises a request
- **THEN** the new request appears and the written text is still in the field

#### Scenario: New request while typing on the timeline
- **WHEN** a board member has checked two options of a pending question on `DF-7` and the agent of `DF-7` raises another request
- **THEN** the new card appears and both options are still checked

#### Scenario: Decided elsewhere while typing
- **WHEN** a board member is writing an answer on `DF-7` and another board member answers the same question
- **THEN** the card on `DF-7` shows the other board member's answer instead of the input

#### Scenario: Rejected answer after a live update
- **WHEN** a new request arrived while a board member was typing, and the board member then submits an answer the server rejects
- **THEN** the card shows the error below the input with the typed text kept

### Requirement: Choice targets on phones
Below 1024 px every option, "Other" and "Answer" SHALL be at least 44 px high, and the whole option row SHALL toggle its control.

#### Scenario: Tap on the label
- **WHEN** a board member taps the label text of an option at 390 px width
- **THEN** the option's radio is checked
