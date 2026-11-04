# Proposal

## Why

The persona's Claude token is handed to the sandbox today (accepted risk in ADR-0010), and token usage comes from the harness (ACP adapter, session-log fallback), which every further harness and the factory minion would report differently or not at all. Routing all model traffic through the gateway closes the credential gap and gives one usage channel for every harness (ADR-0050).

## What Changes

- **Inference endpoint** in the gateway at `/inference` on the Git proxy's TLS listener: sandbox identified by pod IP, client credential stripped, provider credential injected last, request and response bodies streamed through unchanged. Three wire APIs, each with a fixed path list: Anthropic Messages (`/v1/messages`, `/v1/messages/count_tokens`, `/v1/models`), OpenAI Chat Completions (`/v1/chat/completions`, `/v1/models`), OpenAI Responses (`/v1/responses`, `/v1/models`). Every other path answers `404`.
- **Model providers**: instance-wide configuration of an upstream service and one credential: one upstream base URL per offered wire API, a credential reference (existing `credential_refs`), a credential scheme (`x-api-key`, `bearer`, `header` with name and prefix, `claude-subscription`, which takes no extra headers) and optional extra headers. Built-in presets (Anthropic, Claude subscription, OpenAI, OpenCode Go) prefill the form only.
- **Extra headers** are minijinja templates over `task_id`, `task_key`, `run_id`, `project_key`, `persona_id` with a `sha256` filter, validated on save and rendered by the control plane per run. They replace client headers of the same name.
- **BREAKING** Personas reference exactly one model provider (`model_provider_id`, required) instead of a harness credential. A migration creates a Claude subscription provider for every credential used by personas today. `claude-subscription` providers can only be used by `claude-code` personas.
- Harnesses declare their wire APIs (preference order) and environment variables as code constants; a persona is valid only if its harness and its provider share a wire API. At run start the control plane picks the first shared API and sets the harness's base URL, placeholder key and CA variables.
- **BREAKING** `HarnessCredentials` (agent contract) is removed; no provider credential reaches a sandbox.
- **Usage from the gateway**: the gateway reads usage from a copy of each response (final SSE events or JSON body) and reports one record per request (`GatewayUsage.Report`). The control plane stores the records idempotently; the run view and `df.agent.tokens` use them. A response without usage is recorded as unknown, never as zero.
- **BREAKING** Once the gateway path is verified (e2e and a manual Claude Code run), the runtime's usage reporting is removed: `TurnEnded.usage`, `usage_source`, `UsageSource` and the session-log parser.
- **BREAKING** The instance setting and Helm value `forge.allowPlainHttp` moves to `network.allowPlainHttp` (no alias): it governs forge plugin API URLs, Git routes and model provider upstreams.
- The fake ACP agent gets a model-call step; the e2e runs a fake Anthropic upstream.
- Docs: `api.anthropic.com` no longer belongs in the egress allowlist; Dark Factory stays a minimal pass-through, and routing features (fallback, load balancing, Copilot) belong in an LLM router such as agentgateway configured as the upstream of a model provider.

Out of scope:
- Provider adapters as WASI plugins (F5): backlog.
- Allowed models per provider, token and cost budgets enforced in the gateway: backlog.
- The factory minion as a client of the inference endpoint: backlog (factory minion row).
- OpenCode as a harness: `iter3-opencode-harness` (iteration 3).
- Generic ACP and the harness registry (F12): iteration 4.
- YAML import/export of model providers and personas: rejected for now (no consumer).
- GitHub Copilot subscriptions as a model provider: rejected (undocumented, partner-only API).
- Protocol translation between wire APIs, `/v1/embeddings`, batch and file APIs: rejected.

## Capabilities

### New Capabilities
- `inference-endpoint`: the gateway's model-traffic endpoint: caller identification, wire APIs and paths, header handling, credential injection, streaming, usage measurement.
- `model-providers`: configuration of model providers, credential schemes, header templates, presets, and the harness compatibility of personas.
- `model-usage`: per-request usage records from the gateway, their storage and aggregation per run and model.

### Modified Capabilities
- `run-credentials`: the harness credential delivery is replaced by model access through the gateway; no model credential reaches the sandbox.
- `acp-harness`: the harness starts with the gateway base URL, placeholder credential and CA variables; usage capture is removed.
- `configuration-management`: personas reference a model provider instead of a harness credential.
- `egress-policy-distribution`: sandbox entries carry an inference route and the snapshot carries model provider credentials.
- `telemetry`: `df.agent.tokens` comes from usage records; the gateway records inference metrics.
- `task-ui`: the runs list shows usage from the gateway, including requests with unknown usage.
- `fake-acp-agent`: a scripted step calls the model through the base URL (replaces the usage step).
- `logging`: the turn-ended log event no longer carries output tokens.
- `git-proxy`: plain-HTTP forges are allowed by `network.allowPlainHttp` (was `forge.allowPlainHttp`).

## Impact

- Contracts: `darkfactory.gateway.v1` (inference route, provider credentials, `GatewayUsage` service), `darkfactory.agent.v1` (`HarnessCredentials` removed), `darkfactory.transcript.v1` (`TurnEnded.usage`, `usage_source`, `UsageSource` removed).
- Database: `model_providers`, `model_usage`; `personas.harness_credential_id` → `model_provider_id` with a data migration; `.sqlx` metadata.
- Crates: `df-gateway` (new `inference` module, control-side usage service and snapshot), `df-core` (harness constants, model provider types, template rendering), `df-db`, `df-agent-channel` (credentials removed, harness env), `factory-agent` (usage parser removed, harness env), `df-web` (model provider pages, persona form, run usage), `fake-acp-agent`, `df-e2e`.
- Dependencies: `minijinja` (new); `sha2`, `eventsource-stream` already in `Cargo.lock`.
- Docs: `crates/df-gateway/README.md`, `crates/factory-agent/docs/configuration.md`, Helm README (allowlist), `docs/demo.md`, user docs on model providers.
