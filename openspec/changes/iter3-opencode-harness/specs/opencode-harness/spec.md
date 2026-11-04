## Purpose
Running OpenCode (`opencode acp`) as an ACP harness: a configuration generated per run that points it at the inference endpoint, disabled network features and tools, and session state on the task volume.

## ADDED Requirements

### Requirement: Generated OpenCode configuration
For an `opencode` run the control plane SHALL pass one configuration document in `OPENCODE_CONFIG_CONTENT`: a single enabled provider `df` at `<inference endpoint>/v1` with the placeholder key, the persona's model as `model` and `small_model` with its limits when the model has them, and update checks off. The provider package SHALL follow the model's wire API.

#### Scenario: Chat Completions model with limits
- **WHEN** a run starts for an `opencode` persona with model `glm-5.3` (OpenAI Chat Completions, context 1000000, output 131072)
- **THEN** `OPENCODE_CONFIG_CONTENT` holds provider `df` with package `@ai-sdk/openai-compatible`, base URL `https://<gateway>/inference/v1`, API key `df-placeholder`, model `glm-5.3` with limit context 1000000 and output 131072, `model` and `small_model` `df/glm-5.3` and `enabled_providers: ["df"]`

#### Scenario: Package per wire API
- **WHEN** the persona's model uses OpenAI Responses, or Anthropic Messages
- **THEN** the provider package is `@ai-sdk/openai`, or `@ai-sdk/anthropic`

#### Scenario: Model without limits
- **WHEN** the persona's model has no context window
- **THEN** the model entry has no `limit`

#### Scenario: Repository configuration cannot redirect the model
- **WHEN** the repository's `opencode.json` sets another base URL for `df`, another provider and another `model`
- **THEN** model requests still go only to the inference endpoint with the persona's model

### Requirement: No network access beside the inference endpoint
An `opencode` run SHALL get `OPENCODE_DISABLE_MODELS_FETCH=1`, `OPENCODE_DISABLE_AUTOUPDATE=1` and `NODE_EXTRA_CA_CERTS` set to the instance CA. With an empty prompt history and no tool calls, OpenCode SHALL contact no host other than the inference endpoint.

#### Scenario: Start without egress
- **WHEN** an `opencode` run starts and answers a prompt without tool calls
- **THEN** the only host it resolves or connects to is the gateway

#### Scenario: Gateway certificate
- **WHEN** OpenCode calls the inference endpoint
- **THEN** it trusts the gateway's certificate through the instance CA

### Requirement: Plugins denied
The generated configuration SHALL deny the use of OpenCode plugins, so plugins listed in the repository's or a global OpenCode configuration are neither installed nor loaded.

#### Scenario: Plugin in the repository
- **WHEN** the repository's `opencode.json` lists a plugin
- **THEN** the session starts, no package registry is contacted and the plugin is not loaded

### Requirement: Tools without a counterpart denied
The generated configuration SHALL deny OpenCode's `question` and `websearch` tools, so they are not offered to the model. Questions to humans go through the factory tool `ask_question`.

#### Scenario: Tool list
- **WHEN** OpenCode sends a model request in an `opencode` run
- **THEN** its tool list contains neither `question` nor `websearch`, and the factory tools are available through the factory MCP server

### Requirement: Repository instructions kept
OpenCode's project configuration SHALL stay enabled, so the repository's `AGENTS.md` reaches the model together with the persona's system prompt.

#### Scenario: Both instructions
- **WHEN** the repository has an `AGENTS.md` and the persona has a system prompt
- **THEN** OpenCode's model request contains both

### Requirement: Session state on the task volume
OpenCode's session state SHALL live below `HOME` on the task volume, so the next run of the task resumes the session with `session/load`.

#### Scenario: Resume after a run
- **WHEN** run 1 of an `opencode` task ended and run 2 starts with run 1's harness session id
- **THEN** `session/load` succeeds and replays run 1's conversation
