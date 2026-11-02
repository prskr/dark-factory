# ADR-0050: Model traffic and usage only through the gateway

- **Status:** Accepted (planned for iteration 3, `iter3-inference-gateway`)
- **Date:** 2026-11-02
- **Related:** amends [ADR-0010](0010-credentials-in-the-sandbox.md) and [ADR-0021](0021-usage-and-cost-data.md); [ADR-0020](0020-configuration-in-database.md), [ADR-0029](0029-egress-gateway-and-generated-network-policies.md)

## Context

Today the persona hands its Claude credential to the sandbox (accepted risk in ADR-0010), and usage comes from the harness: the ACP adapter's `TurnEnded` usage, with Claude Code's session JSONL as fallback (ADR-0021). Every further harness (OpenCode, `df-native`, Generic ACP) reports usage differently or not at all. The factory minion runs inside the control plane and has no ACP path at all.

## Decision

- Every persona references exactly one **model provider** (upstream service + one credential). There is no other way for a harness to get model access.
- All model traffic goes through the gateway's inference endpoint. The harness gets the gateway as base URL and a placeholder credential; the gateway swaps it for the real one.
- Usage is measured only in the gateway, per model request, from the response. The runtime no longer reports usage (`TurnEnded.usage`, `UsageSource` and the session-log fallback are removed).
- The gateway stays minimal: pass-through of three wire APIs (Anthropic Messages, OpenAI Chat Completions, OpenAI Responses), no protocol translation, no fallback routing, no body rewriting. Operators who need routing features put a dedicated LLM router (e.g. agentgateway) upstream of a model provider.

## Consequences

- No provider credential enters any sandbox; the ADR-0010 accepted risk is closed.
- One usage channel for every harness and, later, for the factory minion as a further gateway client.
- Usage is per request, not per turn; the run view aggregates per run and model.
- A request whose response carries no usage (e.g. a streamed chat completion without `include_usage`) is recorded as "usage unknown", not as zero.
- The gateway is on the path of every model request: a gateway outage stops all agents, not only Git and egress.

## Alternatives considered

- **Usage from the harness, gateway for credentials only**: one parser per harness, and the minion would need a third path.
- **Both sources, gateway preferred**: two channels to keep consistent and to explain when they disagree.
- **Direct credentials kept as an option per persona**: reopens the ADR-0010 risk and leaves those runs without usage.
