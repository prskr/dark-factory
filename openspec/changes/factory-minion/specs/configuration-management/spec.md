## MODIFIED Requirements

### Requirement: Persona description
A persona SHALL have an optional plain-text description of its role and strengths, at most 500 characters, edited in the persona form and empty by default. The description SHALL have a source: `none` (empty), `generated` (written by the factory minion) or `user` (written by a board member). Saving the form with a changed non-empty description SHALL set source `user`; clearing it SHALL set `none`; saving it unchanged SHALL keep the source. When the minion is on, the control plane SHALL start a `persona-description` minion job with the system prompt as input after a persona save when the source is `none`, or `generated` and the system prompt changed, and once for every persona with source `none` when the minion is turned on. A successful job SHALL store the description with source `generated` unless the source is `user` at the time of the write. The form SHALL mark a generated description as generated. The persona card SHALL show the description when it is set, in place of the start of the system prompt. Agents read it through `list_personas`.

#### Scenario: Description on the card
- **WHEN** a board member saves persona "Reviewer" with description "Reviews Rust PRs for correctness and tests"
- **THEN** the card of "Reviewer" shows that description instead of the start of its system prompt

#### Scenario: No description
- **WHEN** persona "Linus" has no description
- **THEN** its card shows the start of its system prompt as before

#### Scenario: Too long
- **WHEN** a description of 501 characters is saved
- **THEN** the form shows a validation error and the persona is unchanged

#### Scenario: Generated description
- **WHEN** the minion is on and a board member saves persona "Linus" with a system prompt and no description
- **THEN** after the job returns, "Linus" has the generated description with source `generated`, `list_personas` returns it, and the form marks it as generated

#### Scenario: Human edit wins
- **WHEN** persona "Linus" has a generated description and a board member edits it and later changes the system prompt
- **THEN** the description is the board member's with source `user`, and no job runs

#### Scenario: Edit while the job runs
- **WHEN** a board member saves a description for "Linus" before its `persona-description` job returns
- **THEN** the description stays the board member's

#### Scenario: Minion turned on
- **WHEN** a board member turns the minion on while personas "Linus" (no description) and "Reviewer" (description by a board member) exist
- **THEN** a job runs for "Linus" only

## ADDED Requirements

### Requirement: Factory minion setting
Board members SHALL turn the factory minion on by selecting one model of a configured model provider, or off, in a "Factory minion" section of the model providers list page. The setting SHALL be instance-wide and off by default. Changes SHALL be audited as `minion.update` and SHALL reach every control plane replica and the gateway snapshot without a restart. The section SHALL say that minion requests go through the gateway and count as usage.

#### Scenario: Turn the minion on
- **WHEN** a board member selects "Anthropic work · claude-haiku-5-5" in the Factory minion section and saves
- **THEN** the setting is stored, an audit entry `minion.update` exists, and the next task created without a title gets a `task-title` job

#### Scenario: Turn the minion off
- **WHEN** a board member selects "Off" and saves
- **THEN** no further minion job runs and the next snapshot has no minion route
