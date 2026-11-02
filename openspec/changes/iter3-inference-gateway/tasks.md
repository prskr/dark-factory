# Tasks

## 1. Spike and contracts

- [ ] 1.1 Spike a long interactive Claude Code session (project image version) against a pass-through with placeholder `CLAUDE_CODE_OAUTH_TOKEN`: list every method and path it calls besides `/v1/messages` and confirm none needs a credential outside `/inference`. Record the result in `docs/demo.md` next to the iteration-1 spike; extend the path list of `specs/inference-endpoint/spec.md` only if a path is required
- [ ] 1.2 Extend `contract/proto/darkfactory/gateway/v1/gateway.proto` with `InferenceRoute`, `SandboxEntry.inference`, `Snapshot.provider_credentials` and the `GatewayUsage` service (design D8, D4); update `contract/README.md`. Verify with the `protoc` command from `contract/README.md` and `buf format`

## 2. Model providers in the control plane

- [ ] 2.1 Add `minijinja` to `[workspace.dependencies]` (latest release, minimal features) and the header template module in `df-core` (design D7): `sha256` filter, strict undefined, fuel limit, validation with dummy values, reserved header names. Verify the "stable session per task", "invalid template" and "header that collides with the credential" scenarios as unit tests; `cargo xtask outdated` and `cargo deny check` pass, Bazel lock files updated (skill `rust`)
- [ ] 2.2 Add the harness constants in `df-core` (design D6) with the compatibility check and wire API choice. Verify "incompatible provider", "wire API chosen at run start" and the `claude-subscription` restriction as unit tests
- [ ] 2.3 Add the migration for `model_providers`, `model_provider_apis` and `personas.model_provider_id` including the data migration (design D5, Migration Plan), the `df-db` catalog functions, and regenerate `.sqlx`. Verify "existing persona after the upgrade", "delete a used provider" (RESTRICT) and an empty-instance migration with `df_test_postgres` tests (container-backed; CI)
- [ ] 2.4 Add the model provider pages to `df-web` (list, modal form with presets, wire API checkboxes with one URL each, scheme fields, extra headers) and audit entries. Verify "create a provider", "plain HTTP upstream", "Claude subscription with another API", "OpenCode Go preset", "provider page" (no secret value) and "delete a used provider" with handler tests
- [ ] 2.5 Replace the harness credential on the persona form with a model provider select that only lists compatible providers. Verify "create a persona" and "incompatible provider" (server-side rejection) with handler tests
- [ ] 2.6 Document model providers, credential schemes, header templates and presets in the user docs (scenario guide "Use a Claude subscription", "Use an OpenAI-compatible provider", "Use OpenCode Go"; reference table of all fields), including the stance: Dark Factory passes through three wire APIs; routing features and Copilot belong in an LLM router such as agentgateway configured as upstream. Verify the guide's steps against `just serve-local`

## 3. Snapshot and run wiring

- [ ] 3.1 Extend snapshot building (`control/snapshot.rs`) with the inference route per sandbox entry (rendered headers, chosen wire API) and the provider credentials read through `SecretStore`. Verify "run with an inference route", "provider credential rotated" and "unresolvable provider credential" with the existing snapshot tests and `FakeSecretStore`
- [ ] 3.2 Set the inference env (base URL, placeholder, CA variables) in `AcpHarness.env` from the harness constants in `df-agent-channel/src/assign.rs`; remove `HarnessCredentials`, `credentials.rs` and the runtime's credential handling, reserve the proto fields (design D8). Verify "Claude Code with a subscription", "Claude Code with an API key" and "sandbox environment" with assign and runtime tests, and that `factory-agent` starts the harness with the env (`specs/acp-harness` "harness start")
- [ ] 3.3 Update `crates/factory-agent/docs/configuration.md` and the run-credentials part of `docs/demo.md` for the new env; update the Helm README allowlist section ("`api.anthropic.com` no longer needed"). Verify the documented variables match the harness constants

## 4. Inference endpoint in the gateway

- [ ] 4.1 Add a fake Anthropic upstream test server (design D10) answering `/v1/messages` (SSE and JSON with usage), `/v1/chat/completions` and `/v1/responses` with canned bodies and capturing request headers
- [ ] 4.2 Implement routing on the Git listener (design D1, D2): `/inference` prefix, identification, path and wire API check. Verify "anthropic request", "unknown pod", "gateway just started", "API the provider doesn't offer", "path outside the list", "query string kept" and "models of a provider with two APIs" with an in-process gateway
- [ ] 4.3 Implement header handling and credential injection. Verify "placeholder key", "client sends its own session header", "harness header passes through", "Claude subscription", "custom header" and "credential missing from the snapshot" against captured upstream requests, and that no log line contains the credential (log capture)
- [ ] 4.4 Implement streaming forwarding and upstream error mapping. Verify "streamed answer" (event received before the upstream ends), "rate limit relayed" and "provider host down" (502)
- [ ] 4.5 Implement the usage parsers on a tee of the body (design D3) test-first with recorded response fixtures per wire API (streamed and JSON, with and without usage, oversized lines). Verify "streamed Anthropic message", "chat completion stream without usage" and "count tokens"
- [ ] 4.6 Implement the usage buffer and reporter (design D4) and the inference metrics. Verify "control plane briefly unreachable" with a fake `GatewayUsage` server that fails first, overflow counting in `df.gateway.inference.usage_dropped`, and "upstream rate limit" for `df.gateway.inference.requests`
- [ ] 4.7 Document the inference endpoint (paths, status codes, header rules, metrics) in `crates/df-gateway/README.md` and verify `cargo llvm-cov -p df-gateway` stays at ≥ 70 % line coverage

## 5. Usage records in the control plane

- [ ] 5.1 Add the `model_usage` migration and the `GatewayUsage.Report` server (insert ignoring duplicates, unknown runs dropped with a warning), regenerate `.sqlx`. Verify "retried report" and "run that already ended" with `df_test_postgres` tests (container-backed; CI)
- [ ] 5.2 Record `df.agent.tokens` from stored records (only newly inserted rows). Verify "token usage" and "duplicate record" scenarios of `specs/telemetry`

## 6. Fake agent and end-to-end verification

- [ ] 6.1 Add the `model` step to `fake-acp-agent` (design D10). Verify "model call through the gateway" with the fake upstream and an in-process gateway
- [ ] 6.2 Extend the `iter1-demo-e2e` scripts and `df-e2e` setup with a model step and the fake upstream as the persona's model provider. Verify with `just e2e-local` that every run reports gateway usage records and the sandbox env contains no credential (container-backed; CI)
- [ ] 6.3 Run one task manually with Claude Code and a subscription model provider against `just serve-local` and record in `docs/demo.md`: streaming works, every `/v1/messages` request has a usage record, and the sandbox env holds only the placeholder

## 7. Remove usage from the runtime (after group 6 passes)

- [ ] 7.1 Switch the run view and `task_views.rs` to `model_usage` including the unknown-usage count, and refresh run regions at turn end. Verify "token table", "requests without usage" and "usage refreshed at turn end" with view and SSE tests
- [ ] 7.2 Remove `TurnEnded.usage`, `usage_source`, `Usage`, `UsageSource` and `Budgets.max_output_tokens` from the contracts (fields reserved), `factory-agent/src/usage.rs`, the adapter usage handling, the old `df.agent.tokens` source, the fake agent's usage step, and output tokens from the turn-ended log event. Verify `cargo build --workspace`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo-machete` and the "harness reports usage over ACP" scenario as a runtime test
- [ ] 7.3 Update `contract/README.md`, `docs/demo.md` and ADR references that still describe harness usage; verify `grep -rn "USAGE_SOURCE\|usage_source" crates contract docs` finds only reserved-field comments
