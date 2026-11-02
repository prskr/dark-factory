# Spec Delta

## Purpose

The gateway's endpoint for model traffic: it identifies the calling run, forwards requests of the supported wire APIs to the run's model provider with the provider credential, streams the answers back and measures usage, so that no model credential is needed in a sandbox.

## ADDED Requirements

### Requirement: Inference endpoint
The gateway SHALL serve model traffic under `https://<gateway host>:<git port>/inference/` with the certificate of the instance CA, on the same listener as the Git proxy. The path after `/inference` is the path of the upstream API.

#### Scenario: Anthropic request
- **WHEN** the harness of run R sends `POST https://<gateway>:8443/inference/v1/messages`
- **THEN** the gateway forwards it to the run's model provider as `POST <anthropic-messages upstream URL>/v1/messages`

### Requirement: Caller identification
The endpoint SHALL identify the run by source pod IP from the latest policy snapshot. It SHALL answer 403 for unknown sources and 503 until the first snapshot arrived, and SHALL contact no upstream in both cases.

#### Scenario: Unknown pod
- **WHEN** a pod that isn't an active sandbox sends `POST /inference/v1/messages`
- **THEN** the gateway answers 403 and contacts no upstream

#### Scenario: Gateway just started
- **WHEN** a request arrives before the gateway received its first snapshot
- **THEN** the gateway answers 503

### Requirement: Wire APIs and paths
The endpoint SHALL forward only these paths, and only for wire APIs the run's model provider offers: Anthropic Messages `POST /v1/messages`, `POST /v1/messages/count_tokens`, `GET /v1/models`; OpenAI Chat Completions `POST /v1/chat/completions`, `GET /v1/models`; OpenAI Responses `POST /v1/responses`, `GET /v1/models`. Query strings SHALL be kept. Every other method or path SHALL get 404 without contacting an upstream.

#### Scenario: API the provider doesn't offer
- **WHEN** the run's provider offers only Anthropic Messages and the harness sends `POST /inference/v1/chat/completions`
- **THEN** the gateway answers 404 and contacts no upstream

#### Scenario: Path outside the list
- **WHEN** the harness sends `POST /inference/v1/messages/batches`
- **THEN** the gateway answers 404

#### Scenario: Query string kept
- **WHEN** Claude Code sends `POST /inference/v1/messages?beta=true`
- **THEN** the upstream request is `POST <upstream>/v1/messages?beta=true`

#### Scenario: Models of a provider with two APIs
- **WHEN** the run's provider offers OpenAI Chat Completions and Anthropic Messages with different upstream URLs and the harness sends `GET /inference/v1/models`
- **THEN** the request goes to the upstream URL of the wire API chosen for the run

### Requirement: Client credentials are stripped
The endpoint SHALL remove `Authorization`, `Proxy-Authorization`, `x-api-key`, `api-key` and the provider's credential header from every request before forwarding it.

#### Scenario: Placeholder key
- **WHEN** the harness sends `x-api-key: df-placeholder`
- **THEN** the upstream request does not contain `df-placeholder`

### Requirement: Provider headers replace client headers
The endpoint SHALL forward all other client headers except hop-by-hop headers, `Host` and `Content-Length`, and SHALL set the run's rendered provider headers, replacing client headers of the same name.

#### Scenario: Client sends its own session header
- **WHEN** the provider sets `x-opencode-session: 3f1c…` for the run and the harness sends `x-opencode-session: abc`
- **THEN** the upstream request carries only `x-opencode-session: 3f1c…`

#### Scenario: Harness header passes through
- **WHEN** Claude Code sends `anthropic-beta: oauth-2025-04-20,claude-code-20250219`
- **THEN** the upstream request carries the same `anthropic-beta` header

### Requirement: Credential injection
The endpoint SHALL inject the provider credential after all other header changes, as defined by its credential scheme, and SHALL never return the credential to the client or write it to logs.

#### Scenario: Claude subscription
- **WHEN** the run's provider has scheme `claude-subscription`
- **THEN** the upstream request carries `Authorization: Bearer <token>` and no `x-api-key`

#### Scenario: Custom header
- **WHEN** the provider has scheme `header` with name `api-key` and no prefix
- **THEN** the upstream request carries `api-key: <key>`

#### Scenario: Credential missing from the snapshot
- **WHEN** the run's provider credential is not in the snapshot (unresolvable Secret)
- **THEN** the gateway answers 502 with a fixed message and contacts no upstream

### Requirement: Streaming pass-through
The endpoint SHALL send request bodies upstream and response bodies back as they arrive, without buffering the whole body and without changing them. Upstream responses SHALL be relayed with their status and headers, hop-by-hop headers removed.

#### Scenario: Streamed answer
- **WHEN** the upstream answers with `text/event-stream` and sends an event
- **THEN** the harness receives that event before the upstream response ends

#### Scenario: Rate limit relayed
- **WHEN** the upstream answers 429 with `retry-after: 30`
- **THEN** the harness receives 429 with `retry-after: 30`

### Requirement: Unreachable upstream
The endpoint SHALL answer 502 with a fixed message when the upstream cannot be connected to or the TLS handshake fails, with a connect timeout of 10 s and no limit on the total duration of a response.

#### Scenario: Provider host down
- **WHEN** the upstream host refuses the connection
- **THEN** the harness receives 502 and a usage record with unknown usage is reported

### Requirement: Usage measurement
For every request to `/v1/messages`, `/v1/chat/completions` or `/v1/responses` the gateway SHALL read token usage from a copy of the response without delaying it: from the final usage events of a stream or from the `usage` object of a JSON body, together with the model named in the response. A response without usage SHALL be reported with usage unknown.

#### Scenario: Streamed Anthropic message
- **WHEN** a streamed `/v1/messages` response reports 120 input and 15 cache-read tokens in `message_start` and 40 output tokens in its final `message_delta`
- **THEN** the gateway reports one record with model from `message_start`, input 120, cache read 15 and output 40

#### Scenario: Chat completion stream without usage
- **WHEN** a streamed `/v1/chat/completions` response contains no `usage` chunk
- **THEN** the gateway reports one record with usage unknown

#### Scenario: Count tokens
- **WHEN** the harness calls `POST /v1/messages/count_tokens`
- **THEN** no usage record is reported for that request

### Requirement: Usage is reported to the control plane
The gateway SHALL report usage records to the control plane in batches and retry failed reports. When its buffer of unsent records is full it SHALL drop the oldest records and count them in `df.gateway.inference.usage_dropped`.

#### Scenario: Control plane briefly unreachable
- **WHEN** the control plane is unreachable for 10 s while runs make model requests
- **THEN** the records of those requests are delivered after it is reachable again
