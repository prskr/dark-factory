## ADDED Requirements

### Requirement: Markdown in user messages
The visible text of a user message in the transcript SHALL be rendered as Markdown with the same rules as agent messages (untrusted input, code highlighting by fence language, code themes, Mermaid), except that a single newline SHALL be a line break. One or more blank lines SHALL separate paragraphs, as in agent messages. Hidden blocks shown with `web.showHiddenBlocks` and the queued message stack SHALL stay plain text.

#### Scenario: Code in a user message
- **WHEN** a board member sends a message with a fence tagged `rust`
- **THEN** the message bubble shows the block highlighted like the same block in an agent message

#### Scenario: Line breaks kept
- **WHEN** a board member sends "first line" and "second line" separated by a single newline
- **THEN** the message shows them on two lines

#### Scenario: Several blank lines
- **WHEN** a board member sends "first" and "second" separated by three blank lines
- **THEN** the message shows two paragraphs with the usual paragraph spacing

#### Scenario: Script in a user message
- **WHEN** a user message contains `<script>alert(1)</script>`
- **THEN** the text is shown literally and no script runs

#### Scenario: Queued message stays plain
- **WHEN** a message with a fence tagged `rust` is queued
- **THEN** the queued stack shows its text plain, without highlighting
