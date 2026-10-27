## ADDED Requirements

### Requirement: Notices as transcript rows
A runtime or control plane notice SHALL be shown as one line in the transcript, aligned with tool rows, with the `bot-message-square` icon, the notice code and the message, without a background box. The row SHALL be expandable (touch and keyboard) to show the full message. The level SHALL only tint the code. A notice row directly after a tool row or another notice row SHALL share its divider line.

#### Scenario: Notice row
- **WHEN** the transcript contains the notice `runtime.outbox_discarded`
- **THEN** it is one line with the icon in front, the code and the message, and a divider line above and below

#### Scenario: Long notice
- **WHEN** a notice message is longer than one line
- **THEN** the row shows it truncated and opening the row shows the full message

#### Scenario: Mixed rows
- **WHEN** a notice directly follows a tool call
- **THEN** both share one divider line between them

### Requirement: Hidden content blocks
By default, content blocks marked `hidden` SHALL NOT be shown in the transcript or in the queued message stack, and a message whose blocks are all hidden SHALL NOT be rendered. With the instance option `web.showHiddenBlocks` set, hidden blocks SHALL be shown in both places, visually distinct and labeled as an instruction for the agent, and a message of only hidden blocks SHALL be rendered. Hidden blocks SHALL still be delivered to the harness.

#### Scenario: Title instruction not shown
- **WHEN** a task without a title is created and the first run starts
- **THEN** the transcript shows the description as the user's message and does not show the `set_task_title` instruction

#### Scenario: Debug option
- **WHEN** `web.showHiddenBlocks` is `true` and the same transcript is opened
- **THEN** the instruction is shown below the description, muted, with the label "Instruction for the agent"

#### Scenario: Queued stack
- **WHEN** a message with a hidden block is queued and `web.showHiddenBlocks` is `false`
- **THEN** the queued stack shows only its visible text, and with the option set it also shows the hidden text

#### Scenario: Existing events
- **WHEN** a stored `UserMessage` event has no `hidden` key on its blocks
- **THEN** all its blocks are shown
