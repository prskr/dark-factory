# Design

## Context

- The runtime is an ACP client and starts the harness as a child process (`acp-harness`). For Claude Code, that is the Node adapter `claude-agent-acp`, which in turn starts the Claude Code CLI, so two processes run per sandbox.
- `harness-images` delivers each harness as a read-only image volume under `/opt/harness/<harness>`. A static musl binary fits this without the glibc/Alpine question of the Node harness.
- Full-harness mode (`product/context.md` §4.1) uses an OpenAI-compatible inference endpoint in the gateway (`design/egress.md` §9), so provider keys stay out of the sandbox. That endpoint does not exist yet.
- The workspace already has `agent-client-protocol` (ACP server and client) and `rmcp` (MCP client).

## Goals / Non-Goals

**Goals:**
- Memory figures per harness (peak and steady), measured the same way and in the same sandbox.
- A go/no-go decision against the criteria under D5, with findings that let the follow-up change be drafted without another spike.

**Non-Goals:**
- No production code on `main`, no configuration, no new harness in the control plane.
- No inference endpoint in the gateway. The prototype talks directly to the provider.
- No custom compaction, no skills, no project config. Their effort is estimated, not built.
- No replacement for Claude Code on subscription runs.

## Decisions

### D1: Separate ACP binary, not a loop in the runtime

The prototype is a standalone binary `df-harness` that serves ACP over stdio. The runtime starts it like any other harness.

- **Memory:** a second Rust process costs a few MB. The saving comes from leaving Node, not from removing the process boundary.
- **One integration path:** transcript translation, session resume, turns, budgets and run outcomes (`acp-harness`) stay the same for all harnesses. A loop in the runtime would need a second implementation of each.
- **Isolation:** the runtime holds the mTLS credentials for the control plane. The harness processes untrusted model output and repository content. A harness crash or OOM kill doesn't take the runtime down with it.
- **Tests:** the existing setup with `fake-acp-agent` stays valid.

*Alternative:* the agent loop as a mode of `factory-agent`, as `product/context.md` §4.1 reads today. Rejected unless the spike finds something ACP cannot carry. Since we own both ends, a gap is closed with an ACP extension (`_meta`, extension methods), not with a second integration path.

### D2: `rpi-*` first, octolib as fallback, Codex as reference

1. **`rpi-ai`, `rpi-agent`, `rpi-tools`:** these cover the loop, tools and streaming, so the least code is left to write. `rpi-harness` (session tree, JSONL persistence, compaction) is evaluated but not necessarily used, because session state may have to fit our resume model (`continuation`, `session/load`).
2. **octolib:** only if `rpi-*` fails D5 on seams or dependency weight. It solves only the provider layer, so the loop, tools and persistence would be our own code. The effort is estimated, not prototyped.
3. **Codex via its ACP adapter:** measured only (memory, outcome). It is the option that needs no harness code at all, so it serves as the reference for whether our own code is worth it.

*Alternative:* writing our own loop on a plain HTTP client. Rejected for the spike, because it measures nothing that octolib doesn't measure too.

### D3: Model access through the spike's OpenAI-compatible endpoint

The prototype configures the model as base URL plus key from `HarnessCredentials` (environment, never under `/data`). For the spike it points at an OpenAI-compatible endpoint provided for the spike. It costs no extra API credits and returns reasoning tokens. Later the prototype points at the gateway's inference endpoint, which then injects the key.

- **Reasoning tokens** become ACP `agent_thought_chunk` updates, as Claude Code's thinking does today. OpenAI-compatible endpoints carry reasoning in non-standard fields (`reasoning_content`, `reasoning`). Which one this endpoint uses, and whether the `rpi-ai` OpenAI-compatible provider reads it, is part of the source review (task 1.2).
- **Claude Code** cannot use the endpoint (it speaks the Anthropic Messages API) and keeps running on its subscription. Memory does not depend on the model, so the baseline stays valid.
- **Quality** is compared between `df-harness` and Codex on the same endpoint and model, so only the harness differs. Claude Code's result is recorded as a reference. If Codex cannot use the endpoint, only its memory is measured and quality is judged against the reference.

*Alternative:* Claude via API key for all three harnesses. Rejected: extra credits for the spike.

### D4: Fixed measurement method

- **Task:** one fixed task in the demo project (`iter1-demo-e2e`), 3 runs per harness. `df-harness` and Codex use the same endpoint and model (D3).
- **Memory:** peak (`VmHWM`) and steady RSS of the harness process tree (adapter and CLI together for Claude Code), plus the cgroup's `memory.peak` of the sandbox container, so the workload is visible next to the harness.
- **Also recorded:** task outcome (tests pass, PR content reviewable), tokens per run, wall-clock time.

### D5: Decision criteria

| Criterion | Go if |
|---|---|
| Memory | Harness tree peak ≤ 25% of the Claude Code baseline |
| ACP fit | Every `acp-harness` requirement can be served: text and thought chunks (thoughts from the endpoint's reasoning tokens, D3), tool calls and updates, stop reasons including rate limit, `session/cancel`, usage per turn, `session/load` or the summary fallback |
| Factory MCP | Factory tools usable in the loop (through `rpi-*` itself or an `rmcp` bridge) |
| Seams | Execution environment and session store can be swapped without forking |
| Dependencies | Library crates without TUI and plugin loader, `cargo deny` passes, static musl build works |
| Quality | Task solved in at least as many runs as Codex on the same model (D3); Claude Code as reference |
| Maintenance | Upstream active, API stable enough to pin; forking is a realistic fallback |

A failure on memory, ACP fit or quality is a no-go. The rest are effort items that go into the estimate for the follow-up change.

## Risks / Trade-offs

- [`rpi-*` is 0.9.x with one maintainer] → pin the version; whether forking is feasible is part of D5 (maintenance).
- [A generic prompt and toolset solve fewer tasks than Claude Code] → quality is a no-go criterion. Prompt quality is part of the follow-up change, not the spike.
- [Claude Code runs a different model than `df-harness`, so its outcome mixes model and harness] → the quality criterion compares against Codex on the same model; Claude Code is a reference only (D3).
- [The endpoint's reasoning field is not read by `rpi-ai`] → a small patch or provider wrapper, estimated in the source review; thoughts are not a no-go on their own, missing text or tool calls are.
- [The memory figure is dominated by the workload, not the harness] → D4 also measures the cgroup peak; the decision weighs the saving per sandbox against typical memory limits.
- [The saving applies only to runs billed by API] → stated in the proposal; subscription runs stay on Claude Code.
- [The 3-day time box is exceeded] → stop; the findings so far and the open points go into the decision.

## Migration Plan

None. Nothing on `main` changes.

## Findings

<!-- Filled in during the spike. -->

| Harness | Peak RSS (tree) | Steady RSS | cgroup `memory.peak` | Solved (of 3) | Tokens per run |
|---|---|---|---|---|---|
| Claude Code (adapter + CLI) | | | | | |
| `df-harness` (`rpi-*`) | | | | | |
| Codex (ACP adapter) | | | | | |

**Decision:** pending.

## Open Questions

- Name of the harness in configuration and under `/opt/harness/` (`df-native`?). Settled in the follow-up change.
