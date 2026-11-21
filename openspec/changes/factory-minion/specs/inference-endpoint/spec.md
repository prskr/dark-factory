## ADDED Requirements

### Requirement: Model calls of the control plane
Every gateway pod SHALL serve `GatewayInference.Complete` on its internal mTLS port and SHALL accept it only from the control plane's client certificate. It SHALL forward the request body to the minion route of the latest snapshot with the same header rules, credential injection, timeouts and usage measurement as the inference endpoint, and SHALL return the upstream status and the whole response body. It SHALL answer `503` without contacting an upstream when the snapshot has no minion route, `404` when the path does not belong to the minion route's wire API, and `502` when the response body exceeds 4 MiB. Request bodies above 1 MiB SHALL be rejected.

#### Scenario: Minion request
- **WHEN** the control plane sends `Complete` with path `/v1/messages` and the minion route uses Anthropic Messages
- **THEN** the gateway sends `POST <upstream>/v1/messages` with the provider credential and returns the upstream status and body

#### Scenario: Sandbox calls the internal port
- **WHEN** a sandbox pod opens a connection to the gateway's internal port
- **THEN** the TLS handshake fails and no upstream is contacted

#### Scenario: Minion off
- **WHEN** `Complete` arrives and the snapshot has no minion route
- **THEN** the gateway answers `503` and contacts no upstream

#### Scenario: Wrong wire API
- **WHEN** the minion route uses OpenAI Chat Completions and `Complete` names `/v1/messages`
- **THEN** the gateway answers `404`

### Requirement: Minion route in the snapshot
The policy snapshot SHALL carry a minion route when the minion is on: the route of the selected provider with only the selected model's wire API, its header templates rendered with the value `minion` for every run variable, and the provider credential. Without a minion setting the snapshot SHALL carry no minion route and no credential for it.

#### Scenario: Minion turned on
- **WHEN** a board member selects model `claude-haiku-5-5` of provider "Anthropic work" for the minion
- **THEN** the next snapshot carries a minion route for Anthropic Messages of "Anthropic work" and its credential, also when no run uses that provider

## MODIFIED Requirements

### Requirement: Usage is reported to the control plane
The gateway SHALL report usage records to the control plane in batches and retry failed reports. Each record SHALL name its caller: the run for inference endpoint requests, or the minion job with its project, task and persona for `Complete` requests. When its buffer of unsent records is full it SHALL drop the oldest records and count them in `df.gateway.inference.usage_dropped`. Records the control plane refuses as invalid SHALL NOT be retried and SHALL be counted in `df.gateway.inference.usage_dropped` as well.

#### Scenario: Control plane briefly unreachable
- **WHEN** the control plane is unreachable for 10 s while runs make model requests
- **THEN** the records of those requests are delivered after it is reachable again

#### Scenario: Minion record
- **WHEN** a `Complete` request of job `task-title` for task `DF-7` gets a response with usage
- **THEN** the gateway reports one record with caller minion, job `task-title`, the task and project of `DF-7` and no run
