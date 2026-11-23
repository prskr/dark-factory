## ADDED Requirements

### Requirement: Model calls of the control plane
Every gateway pod SHALL serve the provider-neutral `Inference.Complete` on its internal mTLS port, only for the control plane's client certificate. It SHALL build a non-streaming, tool-less request of the minion route's wire API from instructions, input, output token limit and the route's model, send it like an inference endpoint request (credential, timeouts, usage), and return the output text and whether it was cut at the limit.

#### Scenario: Anthropic model
- **WHEN** the minion route uses Anthropic Messages with model `claude-haiku-5-5` and `Complete` arrives with instructions, input and a ceiling of 2048 tokens
- **THEN** the gateway sends one `POST <upstream>/v1/messages` with `content-type: application/json`, `anthropic-version: 2023-06-01`, `model: claude-haiku-5-5`, `system` set to the instructions, one user message with the input, `max_tokens: 2048`, no `tools` and no `stream: true`, and returns the text of the `text` blocks

#### Scenario: Responses model
- **WHEN** the minion route uses OpenAI Responses
- **THEN** the upstream request is `POST <upstream>/v1/responses` with `instructions`, `input`, `max_output_tokens`, `store: false` and no `tools`

### Requirement: Only the control plane may call Complete
The gateway SHALL serve `Complete` only to a peer whose certificate has the control plane's DNS name, checked like for `Tail`, and SHALL answer every other peer, including sandbox pods with a certificate of the instance CA, with `PERMISSION_DENIED` without contacting an upstream. Without a configured control-plane name, the gateway SHALL NOT serve `Complete` at all.

#### Scenario: Sandbox calls Complete
- **WHEN** a sandbox pod with its instance CA certificate calls `Complete` on the gateway's internal port
- **THEN** the call ends with `PERMISSION_DENIED` and no upstream is contacted

#### Scenario: No control-plane name configured
- **WHEN** a gateway pod starts without a control-plane name
- **THEN** it does not serve `Complete`, and a call ends with `UNIMPLEMENTED`

### Requirement: Complete times out before its caller
The gateway SHALL end the upstream part of a `Complete` call 2 s before the call's gRPC deadline, answer `DEADLINE_EXCEEDED`, and report a usage record with unknown usage for every call that ends without a response, also when the caller goes away.

#### Scenario: Slow provider
- **WHEN** the control plane calls `Complete` with a 60 s deadline and the provider sends no response head
- **THEN** after 58 s the gateway answers `DEADLINE_EXCEEDED` and reports one usage record with unknown usage

### Requirement: Provider headers per control-plane call
For each `Complete` request the gateway SHALL render the minion route's header templates with every run variable (`task_id`, `task_key`, `run_id`, `project_key`, `persona_id`) set to `minion-<request ID>`, where the request ID is the new UUIDv7 of the call's usage record, and SHALL set them with the same rules as the rendered provider headers of a run, before the credential.

#### Scenario: Two minion calls
- **WHEN** the minion's provider has the header `x-opencode-session: {{ task_id | sha256 }}` and two `Complete` requests arrive
- **THEN** each upstream request carries an `x-opencode-session` value, and the two values differ

### Requirement: Errors of control-plane calls
The gateway SHALL answer a failed `Complete` request with a gRPC status, never with the upstream body: `FAILED_PRECONDITION` without a minion route or its credential, `UNAVAILABLE` when the upstream cannot be reached or answers `5xx` or stalls, `RESOURCE_EXHAUSTED` for `429`, `DEADLINE_EXCEEDED` when the upstream does not answer in time, `INVALID_ARGUMENT` for a request over 1 MiB or another upstream `4xx`, and `INTERNAL` for a response it cannot read or one over 4 MiB.

#### Scenario: Minion off
- **WHEN** `Complete` arrives and the snapshot has no minion route
- **THEN** the gateway answers `FAILED_PRECONDITION` and contacts no upstream

#### Scenario: Rate limit
- **WHEN** the upstream answers a `Complete` request with `429`
- **THEN** the gateway answers `RESOURCE_EXHAUSTED`

#### Scenario: Large response
- **WHEN** the upstream answers a `Complete` request with a 5 MiB body
- **THEN** the gateway answers `INTERNAL`

### Requirement: Minion route in the snapshot
The policy snapshot SHALL carry a minion route when the minion is on: the selected model's wire API, upstream URL, model ID and max output tokens, the setting's reasoning effort, the provider's credential reference and scheme, its header templates unrendered, and the provider credential. Without a minion setting the snapshot SHALL carry no minion route and no credential for it.

#### Scenario: Minion turned on
- **WHEN** a board member selects model `claude-haiku-5-5` of provider "Anthropic work" for the minion
- **THEN** the next snapshot carries a minion route for Anthropic Messages of "Anthropic work" with model `claude-haiku-5-5` and its credential, also when no run uses that provider

### Requirement: Reasoning effort of control-plane calls
When the minion route has a reasoning effort, the gateway SHALL send it unchanged as `reasoning.effort` on OpenAI Responses, as `reasoning_effort` on OpenAI Chat Completions and as `output_config.effort` on Anthropic Messages. Without one, the request SHALL have no reasoning or effort field. The gateway SHALL never send a `thinking` parameter.

#### Scenario: Responses with effort
- **WHEN** the minion route uses OpenAI Responses with reasoning effort `minimal`
- **THEN** the upstream request body has `"reasoning": {"effort": "minimal"}`

#### Scenario: Chat Completions with effort
- **WHEN** the minion route uses OpenAI Chat Completions with reasoning effort `low`
- **THEN** the upstream request body has `"reasoning_effort": "low"`

#### Scenario: Claude with effort
- **WHEN** the minion route uses Anthropic Messages with reasoning effort `low`
- **THEN** the upstream request body has `"output_config": {"effort": "low"}` and no `thinking`

#### Scenario: No effort set
- **WHEN** the minion route has no reasoning effort
- **THEN** the upstream request body has none of `reasoning`, `reasoning_effort` and `output_config`

#### Scenario: Rejected effort
- **WHEN** the minion route has reasoning effort `none` and the upstream answers `400`
- **THEN** the gateway answers `INVALID_ARGUMENT` with a status message that says to check the minion's reasoning effort

### Requirement: Usage records name their caller
Each usage record the gateway reports SHALL name its caller: the run for inference endpoint requests, or the minion job with its project, task and persona for `Complete` requests.

#### Scenario: Minion record
- **WHEN** a `Complete` request of job `task-title` for task `DF-7` gets a response with usage
- **THEN** the gateway reports one record with caller minion, job `task-title`, the task and project of `DF-7` and no run
