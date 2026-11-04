# Design

## Context

See proposal.md (Why) and ADR-0050. Relevant current state:

- The gateway (`crates/df-gateway`, `dark-factory gateway`) has no database or Kubernetes access. Everything arrives with the `PolicyWatch` snapshot built in `control/snapshot.rs`, including forge credentials read through `SecretStore` on every rebuild (at least every 60 s).
- The Git proxy (`git/server.rs`) already does what the inference endpoint needs: TLS from the instance CA on port 8443, pod-IP identification (`identify`), header filtering, credential swap and streaming with hyper. Its host is in the sandbox's `NO_PROXY` and `hostAliases`, and the run network policy allows it.
- `HarnessCredentials` is filled in `df-agent-channel/src/credentials.rs` with `CLAUDE_CODE_OAUTH_TOKEN` from `personas.harness_credential_id`.
- Usage arrives in `TurnEnded.usage` (transcript JSON) and is summed per run and model in `df-db/src/task_views.rs`. `factory-agent/src/usage.rs` parses Claude Code's session log as fallback.
- The iteration-1 spike (`docs/demo.md`) showed Claude Code 2.1.316 sends everything to `ANTHROPIC_BASE_URL` (`POST /v1/messages?beta=true`), works with a placeholder `CLAUDE_CODE_OAUTH_TOKEN` when only `Authorization` is swapped before the headers go upstream, and needs unbuffered SSE.

## Goals / Non-Goals

**Goals:**
- Reuse the Git proxy's listener, identification and upstream client; the inference module only adds routing, header rules and usage parsing.
- Keep the gateway free of configuration logic: it receives finished routes (URL, scheme, rendered headers).
- One usage path, removed from the runtime in the same change once verified.

**Non-Goals:**
- Provider adapter plugins (F5): backlog.
- Budgets and allowed models in the gateway: backlog.
- Factory minion as a client (needs a non-pod identity): backlog.
- Live usage updates per request in the UI: rejected for now; the run card refreshes at turn end.
- Protocol translation, retries or fallback between providers: rejected (ADR-0050).

## Decisions

### D1: Endpoint on the Git proxy listener
`/inference/…` is routed next to `/git/…` in the existing TLS server. The sandbox already trusts the instance CA for this host (Git), reaches it without the egress proxy, and the network policy allows it. A separate port would mean another Service port, network policy rule, `NO_PROXY` entry and certificate SAN for no gain.
Alternative: a dedicated listener. Rejected for the reasons above; splitting later is a config change.

### D2: Module layout
New `df-gateway/src/inference/` with `route.rs` (path → wire API, allowed paths), `headers.rs` (strip, provider headers, credential), `forward.rs` (upstream request, streaming relay), `usage.rs` (parsers), `report.rs` (buffer and RPC client). The hyper client and TLS config of `git/upstream.rs` are shared, not duplicated; move them into a small shared module if needed. The control-plane side gets `control/usage_records.rs` next to `git_records.rs`.

### D3: Usage parsing on a tee of the response body
The response body is wrapped in a stream adapter that forwards each frame unchanged and feeds a copy to a parser for the wire API:
- Anthropic Messages: SSE (`eventsource-stream`, already in `Cargo.lock`) `message_start.message.{model,usage}` + last `message_delta.usage`; JSON: `model`, `usage`.
- OpenAI Chat: SSE chunk with non-null `usage` (and `model`); JSON: `model`, `usage` (`prompt_tokens`, `completion_tokens`, `prompt_tokens_details.cached_tokens`).
- OpenAI Responses: SSE `response.completed.response.{model,usage}`; JSON: `model`, `usage` (`input_tokens`, `output_tokens`, `input_tokens_details.cached_tokens`).

Cached tokens are reported as cache read; OpenAI input tokens include cached ones, so input is reported as `input − cached` to match Anthropic's semantics. Parser state is bounded: SSE lines above 1 MiB and JSON bodies above 8 MiB are skipped for parsing (usage unknown) but still relayed. The record is emitted when the body ends or the client disconnects.
Alternative: octolib as provider layer. Rejected: it decodes and re-encodes requests; we only need three small parsers over a pass-through.

### D4: Usage reporting
`service GatewayUsage { rpc Report(UsageBatch) returns (ReportUsageResponse); }` on the internal gRPC connection, like `RecordPush`. The gateway buffers up to 10,000 records, flushes every 2 s or at 100 records, retries with backoff, drops the oldest on overflow (`df.gateway.inference.usage_dropped`). The request ID is a UUIDv7 made by the gateway and the primary key of `model_usage`; `INSERT … ON CONFLICT DO NOTHING` makes retries idempotent. Records for unknown run IDs are dropped with a warning (run rows are never deleted before their usage could arrive).

### D5: Model provider data model
```
model_providers(id, name UNIQUE, credential_ref_id → credential_refs NOT NULL,
                credential_scheme TEXT CHECK IN ('x-api-key','bearer','header','claude-subscription'),
                credential_header TEXT NULL, credential_prefix TEXT NULL,
                headers JSONB  -- [{name, template}]
                created_at, updated_at)
model_provider_apis(provider_id → model_providers ON DELETE CASCADE,
                    wire_api TEXT CHECK IN ('anthropic-messages','openai-chat','openai-responses'),
                    upstream_url TEXT, PRIMARY KEY (provider_id, wire_api))
personas.model_provider_id → model_providers ON DELETE RESTRICT, NOT NULL
model_usage(request_id UUID PK, run_id → runs, recorded_at, model TEXT NULL, wire_api,
            upstream_status INT NULL, input, output, cache_read, cache_write BIGINT NULL,
            usage_known BOOL)
```
`ON DELETE RESTRICT` enforces "providers in use cannot be deleted"; the web layer turns the error into the message naming the personas. Presets are a Rust constant in `df-web` used only by the form.

### D6: Harness constants
`df_core::harness` gets a `match` per harness name returning wire APIs in preference order and, per wire API, the base URL variable, key variable (depends on the credential scheme for Claude Code: `CLAUDE_CODE_OAUTH_TOKEN` for `claude-subscription`, otherwise `ANTHROPIC_API_KEY`) and CA variables. `claude-code`: `[anthropic-messages]`, `ANTHROPIC_BASE_URL`, `NODE_EXTRA_CA_CERTS`. The compatibility check (persona save) and the wire API choice (run start) both use it. No table, no configuration; `iter3-opencode-harness` adds an entry.

### D7: Header templates rendered in the control plane
minijinja with an environment that has only the `sha256` filter registered, no loader, undefined variables as errors, fuel limit set. Validation on save renders the template with dummy values. Snapshot building renders per run with that run's values. The gateway receives `(name, value)` pairs and does no templating.
Alternative: placeholders only (`{task_id}`): no way to hash or combine values; rejected in the discussion. Alternative: rendering in the gateway: needs the engine there and per-request work for per-run constants.

### D8: Run wiring and contract changes
- Agent contract: `HarnessCredentials` and `ControlMessage` field 12 reserved; `Budgets.max_output_tokens` reserved. The inference env (base URL, placeholder, CA path) is added to `AcpHarness.env` by `assign.rs`. The CA path is the instance CA the pod template already mounts for the runtime (`<tls dir>/ca.crt`, also used for Git `sslCAInfo`); the control plane knows it from the pod template.
- Transcript contract: `TurnEnded.usage`, `TurnEnded.usage_source` reserved, `Usage` and `UsageSource` deleted. Stored transcript events keep their old JSON; readers ignore the fields.
- Gateway contract: `SandboxEntry.inference` (`InferenceRoute{wire_api, map<wire_api, upstream_url>, credential_ref, CredentialScheme scheme, header name/prefix, repeated Header headers}`), `Snapshot.provider_credentials` (map like `forge_credentials`), `GatewayUsage` service.
- The placeholder is the constant `df-placeholder`.

### D9: Removal of runtime usage as the last task group
Tasks are ordered so the gateway path works and is verified (e2e with fake upstream, one manual Claude Code run with a subscription token against the dev setup) before `TurnEnded.usage`, `usage.rs`, the session-log fallback and `df.agent.tokens` from `TurnEnded` are removed. The run view switches to `model_usage` in the same group, so no intermediate state shows double counts.

### D10: Fake upstream for tests
`fake-acp-agent` gets a `model` step (reqwest, rustls with the CA from `NODE_EXTRA_CA_CERTS`). A fake Anthropic upstream (small hyper server in `df-gateway` test support, also usable by `df-e2e`) answers `/v1/messages` with a canned SSE stream including usage. Gateway integration tests run upstream and gateway in-process; no containers needed.

## Risks / Trade-offs

- [Gateway outage stops all model traffic] → ≥2 replicas with PDB already exist; harnesses retry on 502/503.
- [Long interactive Claude Code sessions call endpoints other than `/v1/messages` (OAuth refresh, account)] → spike in task 1; with a setup token no refresh happens; unknown paths answer 404 and are logged at DEBUG with the path so missing ones show up.
- [Upstream URL in the snapshot points at an internal host (SSRF from configuration)] → only board members configure providers, `https` is required, and changes are audited.
- [Usage from a response the agent influences] → only the upstream writes the response; the sandbox can at most cause unknown usage by aborting.
- [OpenAI streams without `include_usage`] → reported as unknown; visible on the run card.
- [Usage records lost on gateway restart] → accepted; at most 2 s of records per replica.

## Migration Plan

1. Migration creates the model provider tables and, for each distinct `credential_refs` row referenced by `personas.harness_credential_id`, a provider "Claude subscription (<ref name>)" with `claude-subscription`, Anthropic Messages at `https://api.anthropic.com`; sets `personas.model_provider_id`, then makes it `NOT NULL` and drops `harness_credential_id`. Personas without a credential (only possible in tests) fail the migration with a clear message.
2. Gateway and control plane are deployed from the same chart version; the gateway contract change is additive for the gateway, the agent contract change requires the runtime image of the same version (already the case).
3. Operators may remove `api.anthropic.com` from the instance allowlist afterwards; leaving it is harmless.
Rollback: restore the database backup taken before `migrate`; the old version cannot read the new persona column.

## Open Questions

- Exact `count_tokens` and `/v1/models` behaviour of OpenCode Go upstreams; only relevant for presets and testable in `iter3-opencode-harness`.
