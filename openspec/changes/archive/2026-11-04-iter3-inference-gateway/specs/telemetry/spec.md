## MODIFIED Requirements

### Requirement: Run and turn instruments
The control plane SHALL record:

| Instrument | Type | Attributes | Source |
|------------|------|------------|--------|
| `df.runs.ended` | counter | `outcome` | `RunReport` |
| `df.run.duration` | histogram (s) | `outcome` | run start to `RunReport` |
| `df.agent.turns` | counter | `stop_reason` | `TurnEnded` |
| `df.agent.tokens` | counter | `model`, `token_type` | stored gateway usage records |

`token_type` is one of `input`, `output`, `cache_read`, `cache_write`.

#### Scenario: Token usage
- **WHEN** a gateway usage record `{model: "claude-sonnet-5-5", input: 100, output: 20}` is stored
- **THEN** `df.agent.tokens` increases by 100 for `token_type=input` and by 20 for `token_type=output` with `model=claude-sonnet-5-5`

#### Scenario: Duplicate record
- **WHEN** a usage record with an already stored request ID arrives again
- **THEN** `df.agent.tokens` does not change

## ADDED Requirements

### Requirement: Inference metrics
The gateway SHALL record `df.gateway.inference.requests` (counter; `wire_api`, `status_class` = `2xx`, `4xx`, `5xx` or `error`), `df.gateway.inference.duration` (histogram, s; `wire_api`) and `df.gateway.inference.usage_dropped` (counter). Models, hosts and runs SHALL NOT be attributes.

#### Scenario: Upstream rate limit
- **WHEN** an upstream answers a chat completion with 429
- **THEN** `df.gateway.inference.requests{wire_api="openai-chat",status_class="4xx"}` increases by one
