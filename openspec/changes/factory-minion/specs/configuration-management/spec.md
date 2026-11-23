## ADDED Requirements

### Requirement: Generate a persona description
The description box of a persona form that also holds the system prompt SHALL have a button in its lower right corner, labeled "Generate description from the system prompt" (wand icon), that runs a `persona-description` minion call with the system prompt currently in the form, saved or not. The answer SHALL replace the text in the description box without saving the persona.

#### Scenario: Description after a prompt change
- **WHEN** the minion is on and a board member changes the system prompt of "Linus" and presses the button
- **THEN** the description box shows a description of the changed prompt, and the stored persona is unchanged until the board member saves

#### Scenario: Discard the suggestion
- **WHEN** a board member presses the button and then closes the form without saving
- **THEN** the stored description of the persona is unchanged

### Requirement: Generate button states
While its call runs, the generate button SHALL show progress and be disabled, and the description box SHALL be read-only. A failed call SHALL show "The factory minion could not write a description. Try again." under the description box and keep the box unchanged. The button SHALL be disabled with an explanation while the minion is off or the system prompt is empty.

#### Scenario: Call fails
- **WHEN** the minion's provider is down and a board member presses the button
- **THEN** the error text is shown under the description box and the box keeps its text

#### Scenario: Minion off
- **WHEN** the minion is off and a board member opens the persona form
- **THEN** the button is disabled and its tooltip says that the factory minion is off

### Requirement: Fill empty persona descriptions
When the minion is on, saving a persona with an empty description and a non-empty system prompt, and turning the minion on (once per such persona), SHALL enqueue a `persona-description` minion job. The job SHALL read the system prompt when it runs and SHALL store the result only if, at the time of the write, the description is still empty and the system prompt is still the one it read.

#### Scenario: Persona without a description
- **WHEN** the minion is on and a board member saves persona "Linus" with a system prompt and no description
- **THEN** after the job ran, "Linus" has a generated description and `list_personas` returns it

#### Scenario: Description written while the job waits
- **WHEN** a board member saves a description for "Linus" before its `persona-description` job ran
- **THEN** the description stays the board member's

#### Scenario: Prompt changed while the job runs
- **WHEN** the description job of "Linus" read the system prompt, and a board member changes the prompt before the job writes
- **THEN** the job's result is dropped, and the job enqueued by the prompt change writes the description of the new prompt

#### Scenario: Minion turned on
- **WHEN** a board member turns the minion on while personas "Linus" (no description) and "Reviewer" (with description) exist
- **THEN** a job is enqueued for "Linus" only

#### Scenario: Persona saved while the minion is turned on
- **WHEN** one board member saves persona "Ada" with a system prompt and no description while another one turns the minion on, and both transactions overlap
- **THEN** after both have committed, at least one `persona-description` job for "Ada" exists

#### Scenario: Empty system prompt
- **WHEN** the minion is on and a board member saves persona "Ada" with an empty system prompt and no description
- **THEN** no job is enqueued

### Requirement: Factory minion setting
Board members SHALL turn the factory minion on by selecting one model of a configured model provider, or off, in a "Factory minion" section of the model providers list page. The setting SHALL be instance-wide and off by default. Changes SHALL be audited as `minion.update` and SHALL reach every control plane replica and the gateway snapshot without a restart. The section SHALL say that minion requests go through the gateway and count as usage, and that a small, fast model is enough.

#### Scenario: Turn the minion on
- **WHEN** a board member selects "Anthropic work · claude-haiku-5-5" in the Factory minion section and saves
- **THEN** the setting is stored, an audit entry `minion.update` exists, and the next task created without a title gets a `task-title` job

#### Scenario: Turn the minion off
- **WHEN** a board member selects "Off" and saves
- **THEN** no further minion job is enqueued and the next snapshot has no minion route

### Requirement: Minion reasoning effort
The Factory minion section SHALL have an optional "Reasoning effort" text field with the hint that the accepted values depend on the model. Its suggestions SHALL follow the selected model's wire API: `low`, `medium`, `high`, `xhigh`, `max` for Anthropic Messages; `none`, `minimal`, `low`, `medium`, `high` for the OpenAI APIs. A value SHALL be a lowercase word of at most 32 characters (`a-z`, digits, `-`, `_`). Changes SHALL be audited with the setting.

#### Scenario: Effort for an OpenAI model
- **WHEN** a board member selects "OpenAI · gpt-6-luna" and enters `minimal`
- **THEN** the setting is stored with reasoning effort `minimal`

#### Scenario: Invalid value
- **WHEN** a board member enters `very low`
- **THEN** the form rejects it and the setting is unchanged

#### Scenario: Effort for a Claude model
- **WHEN** a board member selects "Anthropic work · claude-haiku-5-5"
- **THEN** the field suggests `low`, `medium`, `high`, `xhigh` and `max`, and saving `low` stores the setting with reasoning effort `low`
