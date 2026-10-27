# Design

## Context

See ADR-0045 and `openspec/changes/archive/2026-10-27-native-harness-spike/design.md` (Findings, Decision). The prototype exists on the branch `spike/native-harness-spike`.

## Goals / Non-Goals

**Goals:** a production `df-harness` that passes the `acp-harness` requirements with the same transcript, budget and outcome handling as Claude Code.

**Non-Goals:** replacing Claude Code on subscription runs; the gateway inference endpoint itself.

## Decisions

Inherited from the spike: separate ACP binary (D1), `rpi-*` pinned, `rmcp` bridge for factory tools.

## Open items and estimates

| Item | Estimate |
|---|---|
| Compaction (`rpi-harness` or summary fallback) | 2-3 days |
| Session persistence and `session/load` | 2-3 days |
| Usage contract (ACP `usage_update`/end-turn usage vs session log) | 1 day |
| `Retry-After` for rate limits | 0.5 day |
| Prompt and tool tuning, output truncation (includes custom tools such as search/glob and tool-description work) | 3-5 days |
| `bash` tool hardening through `ExecutionEnv` | 1-2 days |
| Image, chart and `df-core` wiring | 1 day |

## Risks / Trade-offs

- Single-maintainer upstream: pin, fork as fallback.
- Quality on real tasks with a stronger model is unverified.
