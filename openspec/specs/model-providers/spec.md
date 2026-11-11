# model-providers Specification

## Purpose
Instance-wide configuration of model providers (an upstream model service plus the one credential Dark Factory uses for it) and the rules that decide which personas and harnesses can use them.

## Requirements

### Requirement: Model providers
Board members SHALL list, create, edit and delete model providers with name, one or more offered wire APIs (Anthropic Messages, OpenAI Chat Completions, OpenAI Responses) each with its own upstream base URL, a credential reference, a credential scheme and optional extra headers. Upstream URLs SHALL use `https`. Changes SHALL be audited.

#### Scenario: Create a provider
- **WHEN** a board member creates "Anthropic work" with Anthropic Messages at `https://api.anthropic.com`, credential reference `anthropic-key` and scheme `x-api-key`
- **THEN** the provider is stored and can be selected on personas

#### Scenario: Plain HTTP upstream
- **WHEN** a board member enters `http://llm.internal` as upstream URL
- **THEN** the form rejects it

### Requirement: Credential schemes
A model provider SHALL have one credential scheme: `x-api-key` (`x-api-key: <value>`), `bearer` (`Authorization: Bearer <value>`), `header` (a header name and optional prefix, `<name>: <prefix><value>`) or `claude-subscription` (`Authorization: Bearer <value>`, Anthropic Messages only, no extra headers).

#### Scenario: Claude subscription with another API
- **WHEN** a board member saves a `claude-subscription` provider that also offers OpenAI Chat Completions
- **THEN** the form rejects it

#### Scenario: Reserved header name
- **WHEN** a board member chooses scheme `header` with name `Host`
- **THEN** the form rejects it

### Requirement: Scheme preview in the provider form
The provider form SHALL show the header and the prefix the scheme sends and an example token (a fixed text, never derived from the credential), disabled where the scheme fixes them and editable for `header`, show the prefix exactly as sent (`Bearer ` with its space), and SHALL keep the same fields when the scheme changes. Values typed in Header, Value and Extra headers SHALL survive any sequence of scheme changes in the open form.

#### Scenario: Header and value preview
- **WHEN** a board member selects scheme `bearer` and then `header`
- **THEN** the Header and Value fields show `Authorization` and prefix `Bearer ` disabled for `bearer`, and become editable for `header`, and the form keeps the same fields

#### Scenario: Typed values survive a scheme change
- **WHEN** a board member types a custom header, a prefix and extra headers, selects `bearer` or `claude-subscription`, then `header` again
- **THEN** the header, prefix and extra headers are the ones typed, and a save stores only what the chosen scheme uses

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

### Requirement: Extra header templates
Extra header values SHALL be templates over `task_id`, `task_key`, `run_id`, `project_key` and `persona_id` with a `sha256` filter (lowercase hex). Templates SHALL be checked on save; names of credential, hop-by-hop, `Host` and `Content-Length` headers SHALL be rejected. The control plane SHALL render the templates once per run. Extra headers SHALL NOT be set for scheme `claude-subscription`: the form disables the field and a save with extra headers for that scheme is rejected.

#### Scenario: Stable session per task
- **WHEN** a provider has header `x-opencode-session: {{ task_id | sha256 }}` and task `DF-7` has two runs
- **THEN** both runs send the same `x-opencode-session` value

#### Scenario: Invalid template
- **WHEN** a board member saves the header value `{{ task_id | md5 }}`
- **THEN** the form rejects it and names the unknown filter

#### Scenario: Extra headers with the Claude subscription scheme
- **WHEN** a board member saves a `claude-subscription` provider with the extra header `x-session: {{ task_id | sha256 }}`
- **THEN** the form rejects it with "Extra headers are not available with the Claude subscription scheme." and nothing is saved

#### Scenario: Header that collides with the credential
- **WHEN** a provider with scheme `x-api-key` gets an extra header `X-Api-Key`
- **THEN** the form rejects it

### Requirement: Credentials stay write-only
A model provider SHALL reference an existing credential reference (Secret name and key) and never hold or display a secret value.

#### Scenario: Provider page
- **WHEN** a board member opens a model provider
- **THEN** the page shows the credential reference's name, Secret and key but no value

### Requirement: Providers in use cannot be deleted
Deleting a model provider that a persona references SHALL be refused, naming the personas.

#### Scenario: Delete a used provider
- **WHEN** a board member deletes "Claude subscription" while persona "Linus" uses it
- **THEN** the deletion is refused with a message naming "Linus"

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

### Requirement: Model provider settings page
Each model provider SHALL have a settings page at `/model-providers/{id}`, inside the settings area. It SHALL show a back link to the provider list, the provider's name, the tabs General and Models, and one "Save model provider" button. The provider list's name and edit action SHALL link to it. An unknown id and `/model-providers/{id}/edit` SHALL answer `404`. The layout follows the approved PenPot boards.

#### Scenario: Open a provider from the list
- **WHEN** a board member at 1440 px width chooses the edit action of provider "Anthropic work" on `/model-providers`
- **THEN** `/model-providers/<id>` opens as a page with the settings sidebar marking Model providers, the header "Anthropic work" and the General tab open

#### Scenario: Unknown provider and old route
- **WHEN** `/model-providers/<unknown id>` or `/model-providers/<id>/edit` is opened
- **THEN** the response is `404`

### Requirement: Model provider settings tab in the URL
The open tab of the model provider settings page SHALL be part of the URL as `tab=models`, and left out for General. Opening such a URL SHALL show that tab, choosing a tab SHALL update the URL without a page load, and an unknown value SHALL show General.

#### Scenario: Link to the Models tab
- **WHEN** a board member opens `/model-providers/<id>?tab=models`
- **THEN** the page opens with the Models tab open

#### Scenario: Choosing a tab updates the URL
- **WHEN** a board member on `/model-providers/<id>` chooses Models
- **THEN** the URL becomes `/model-providers/<id>?tab=models` without a page load

### Requirement: Saving the model provider settings page
Saving SHALL keep the model provider settings page open with the stored values and confirm "Model provider saved" visibly and to screen readers. A validation error SHALL keep the entered values and open the tab with the first error, and the URL SHALL name that tab.

#### Scenario: Save stays on the page
- **WHEN** a board member adds a model on the Models tab and saves
- **THEN** the page stays on the Models tab with the new model, and a "Model provider saved" status is shown and announced

#### Scenario: Validation error on another tab
- **WHEN** a board member on `/model-providers/<id>?tab=models` saves with an `http://` upstream URL entered on General
- **THEN** the page shows the General tab with the error at that URL, all entered values kept, and the URL becomes `/model-providers/<id>`
