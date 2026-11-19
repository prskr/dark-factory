# Spec Delta

## MODIFIED Requirements

### Requirement: Personas
Board members SHALL list, create and edit personas with a persona identity (handle, display name, commit email; see `persona-identity`), system prompt, harness (`claude-code` or `opencode`), a required model provider and a required model of that provider, both compatible with the harness. A persona SHALL NOT have an adapter command. The form SHALL show a live preview of the generated avatar.

#### Scenario: Create a persona
- **WHEN** a board member creates persona `linus` with display name "Linus", email `linus@example.org`, a system prompt, model provider "Claude subscription" and model `claude-sonnet-5-5`
- **THEN** the persona is stored and can be selected when creating a task

#### Scenario: Create an OpenCode persona
- **WHEN** a board member creates persona `ada` with display name "Ada", email `ada@example.org`, harness `opencode`, model provider "Go" and model `glm-5.3`
- **THEN** the persona is stored and its runs start OpenCode

#### Scenario: Model is required
- **WHEN** a board member saves a persona without a model
- **THEN** the form rejects it

#### Scenario: Model select follows the provider
- **WHEN** a board member selects model provider "Go" in the persona form
- **THEN** the model select lists only Go's models whose wire API the persona's harness can use

#### Scenario: No adapter command on the form
- **WHEN** a board member opens the persona form
- **THEN** it has no adapter command field

#### Scenario: Avatar preview
- **WHEN** a board member types display name "Code Reviewer" in the persona form
- **THEN** the avatar preview shows "CR" without a page reload

#### Scenario: Existing persona after the upgrade
- **WHEN** persona "Linus" referenced harness credential `claude` before the upgrade
- **THEN** after migrating it references a model provider with scheme `claude-subscription`, Anthropic Messages at `https://api.anthropic.com` and credential reference `claude`

#### Scenario: Persona model becomes a provider model
- **WHEN** persona "Linus" with model `claude-sonnet-5-5` and model provider "Claude subscription" is migrated
- **THEN** "Claude subscription" has model `claude-sonnet-5-5` on Anthropic Messages without limits and "Linus" references it

#### Scenario: Persona without a model blocks the migration
- **WHEN** a persona has no model when the migration runs
- **THEN** the migration fails and names the persona

### Requirement: Persona cards
The persona list SHALL be ordered by handle and show each persona as a card with its avatar, display name, `@handle`, commit email, harness and model, and the start of its system prompt.

#### Scenario: Persona card
- **WHEN** persona `linus` with display name "Linus", email `linus@example.org`, harness `claude-code` and model `opus` exists
- **THEN** its card shows the avatar "L", "Linus", "@linus", `linus@example.org`, "Claude Code · opus" and the first lines of its prompt
