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
The provider form SHALL offer presets that prefill wire APIs, upstream URLs and scheme: Anthropic, Claude subscription, OpenAI, and OpenCode Go (with an `x-opencode-session` header). Presets SHALL NOT be stored; the saved provider holds the values.

#### Scenario: OpenCode Go preset
- **WHEN** a board member picks the OpenCode Go preset
- **THEN** the form holds OpenAI Chat Completions and OpenAI Responses at `https://opencode.ai/inference/go/openai`, Anthropic Messages at `https://opencode.ai/inference/go/anthropic`, scheme `bearer` and header `x-opencode-session: {{ task_id | sha256 }}`

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
Each harness SHALL declare the wire APIs it can use in order of preference; `claude-code` uses Anthropic Messages. A persona SHALL only be saved with a model provider that offers at least one of its harness's wire APIs, and `claude-subscription` providers only with harness `claude-code`. The persona form SHALL only offer compatible providers and say so under the select.

#### Scenario: Incompatible provider
- **WHEN** a board member saves a `claude-code` persona with a provider offering only OpenAI Chat Completions
- **THEN** the form rejects it

#### Scenario: Wire API chosen at run start
- **WHEN** a run starts for a persona whose harness prefers Anthropic Messages over OpenAI Chat Completions and whose provider offers both
- **THEN** the run uses Anthropic Messages
