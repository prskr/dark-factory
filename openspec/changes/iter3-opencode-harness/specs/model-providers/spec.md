## ADDED Requirements

### Requirement: Provider models
A model provider SHALL hold a list of models, each with a model ID unique within the provider, a wire API the provider offers, and optional context window and max output tokens (positive, both or neither). Board members SHALL add, edit and remove models in the provider form; changes SHALL be audited.

#### Scenario: Add a model
- **WHEN** a board member adds model `glm-5.3` with OpenAI Chat Completions, context window 1000000 and max output 131072 to a provider offering that API
- **THEN** the model is stored and can be selected on personas using the provider

#### Scenario: API the provider does not offer
- **WHEN** a board member adds a model with OpenAI Responses to a provider offering only Anthropic Messages
- **THEN** the form rejects it

#### Scenario: Only one limit
- **WHEN** a board member saves a model with a context window but no max output tokens
- **THEN** the form rejects it

#### Scenario: Duplicate model ID
- **WHEN** a board member adds `claude-sonnet-5-5` twice to one provider
- **THEN** the form rejects it

### Requirement: Models in use cannot be removed
Removing a model that a persona uses, or an offered wire API that a model uses, SHALL be refused, naming the personas or models.

#### Scenario: Remove a used model
- **WHEN** a board member removes `claude-sonnet-5-5` while persona "Linus" uses it
- **THEN** the save is refused with a message naming "Linus"

#### Scenario: Remove a used API
- **WHEN** a board member removes Anthropic Messages from a provider whose model `claude-sonnet-5-5` uses it
- **THEN** the save is refused with a message naming `claude-sonnet-5-5`

## MODIFIED Requirements

### Requirement: Presets
The provider form SHALL offer presets that prefill wire APIs, upstream URLs, scheme and models with their wire API and limits: Anthropic, Claude subscription, OpenAI, and OpenCode Go (with an `x-opencode-session` header). Presets SHALL NOT be stored; the saved provider holds the values.

#### Scenario: OpenCode Go preset
- **WHEN** a board member picks the OpenCode Go preset
- **THEN** the form holds OpenAI Chat Completions and OpenAI Responses at `https://opencode.ai/inference/go/openai`, Anthropic Messages at `https://opencode.ai/inference/go/anthropic`, scheme `bearer` and header `x-opencode-session: {{ task_id | sha256 }}`

#### Scenario: Preset models
- **WHEN** a board member picks the OpenCode Go preset
- **THEN** the form holds models `glm-5.3`, `kimi-k3` and `deepseek-v4-pro` on OpenAI Chat Completions, `minimax-m3`, `qwen3.8-max` and `claude-haiku-5-5` on Anthropic Messages and `gpt-6-luna` on OpenAI Responses, each with context window and max output tokens

#### Scenario: Anthropic and OpenAI preset models
- **WHEN** a board member picks the Anthropic, Claude subscription or OpenAI preset
- **THEN** the form holds `claude-opus-5-5`, `claude-sonnet-5-5` and `claude-haiku-5-5` on Anthropic Messages (context 1000000, output 128000), or `gpt-6-luna`, `gpt-6.1-terra`, `gpt-6.1-sol` and `gpt-6-astra` on OpenAI Responses (context 1050000, output 128000)

#### Scenario: Preset models can be changed
- **WHEN** a board member picks a preset, removes one model and adds `claude-opus-5-6`
- **THEN** the saved provider holds exactly the models in the form

### Requirement: Harness compatibility
Each harness SHALL declare the wire APIs it can use: `claude-code` uses Anthropic Messages; `opencode` uses OpenAI Chat Completions, OpenAI Responses and Anthropic Messages. A persona SHALL only be saved with a model whose wire API its harness can use, and `claude-subscription` providers only with harness `claude-code`. A run SHALL use the wire API of its persona's model. The persona form SHALL only offer compatible providers and models and say so under the selects.

#### Scenario: Incompatible provider
- **WHEN** a board member saves a `claude-code` persona with a provider offering only OpenAI Chat Completions
- **THEN** the form rejects it

#### Scenario: Incompatible model
- **WHEN** a board member saves a `claude-code` persona with model `glm-5.3` (OpenAI Chat Completions) of a provider that also offers Anthropic Messages
- **THEN** the form rejects it

#### Scenario: Wire API chosen at run start
- **WHEN** a run starts for an `opencode` persona whose model `glm-5.3` uses OpenAI Chat Completions at a provider that also offers Anthropic Messages and OpenAI Responses
- **THEN** the run uses OpenAI Chat Completions

#### Scenario: OpenCode with a Chat Completions model
- **WHEN** a board member saves an `opencode` persona with model `glm-5.3` (OpenAI Chat Completions)
- **THEN** the persona is stored

#### Scenario: Claude subscription with OpenCode
- **WHEN** a board member saves an `opencode` persona with a `claude-subscription` provider
- **THEN** the form rejects it
