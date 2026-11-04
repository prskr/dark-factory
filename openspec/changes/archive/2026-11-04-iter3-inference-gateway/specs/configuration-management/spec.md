## MODIFIED Requirements

### Requirement: Personas
Board members SHALL list, create and edit personas with name, system prompt, harness (iteration 1: `claude-code`), optional model and a required model provider compatible with the harness. A persona SHALL NOT have an adapter command.

#### Scenario: Create a persona
- **WHEN** a board member creates persona "Linus" with a system prompt and model provider "Claude subscription"
- **THEN** the persona is stored and can be selected when creating a task

#### Scenario: No adapter command on the form
- **WHEN** a board member opens the persona form
- **THEN** it has no adapter command field

#### Scenario: Existing persona after the upgrade
- **WHEN** persona "Linus" referenced harness credential `claude` before the upgrade
- **THEN** after migrating it references a model provider with scheme `claude-subscription`, Anthropic Messages at `https://api.anthropic.com` and credential reference `claude`
