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

- [`rpi-*` is 0.x (0.11.0 at the time of the spike) with one maintainer] → pin the version; whether forking is feasible is part of D5 (maintenance).
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

### Source review (task 1.2)

Reviewed `rpi-ai`, `rpi-agent`, `rpi-tools`, `rpi-harness` 0.11.0. Sources unpacked from crates.io; paths are relative to the crate root.

| Criterion | Result |
|---|---|
| Streaming events vs. ACP | Fits. `AgentEvent::MessageUpdate` carries `AssistantMessageEvent::{TextDelta, ThinkingDelta, ToolCallDelta, ...}`; `ToolExecutionStart/Update/End` and `TurnEnd` map 1:1 to ACP updates (`rpi-agent/src/events.rs:23-95`, `rpi-ai/src/types.rs:893`). Prototype maps them in `crates/df-harness/src/agent.rs`. |
| Reasoning field | `rpi-ai` OpenAI-compatible provider reads `reasoning_content`, then `reasoning`, then `reasoning_text` from the delta (`rpi-ai/src/providers/openai_completions.rs:904-908`). Which one the spike endpoint sends is open (no endpoint access yet); all three common names are covered, so no patch is expected. Replay of reasoning to the model is opt-in via compat `requires_reasoning_content` (`:397`). |
| Cancellation | `Agent::abort()` cancels a `CancellationToken` passed to provider and tools (`rpi-agent/src/agent.rs:369`, `hooks.rs:141`); the final message gets `StopReason::Aborted`. Maps to ACP `cancelled`. |
| Usage per turn | `AssistantMessage.usage` (`input`, `output`, `cache_read`, `cache_write`, `reasoning`, `total_tokens`, cost) per assistant message (`rpi-ai/src/types.rs:315`). Maps to ACP usage updates. Wired in the prototype as `usage_update` (context occupancy). |
| Rate limit | `AiError::is_retryable` treats 408/409/429/>=500 as retryable; the retry loop is controlled by `max_retries` (default none) and surfaces as `AgentEvent::RetryScheduled` (`rpi-ai/src/error.rs:66`). After the last attempt the status is only in `AssistantMessage.error_message` (text), with `StopReason::Error`; there is no typed rate-limit variant and no `Retry-After` value. The prototype maps a message containing `429` to the ACP error `{error: "rate_limited"}` that `factory-agent` `parse_rate_limit` already parses (verified against a fake 429 endpoint, `tests/smoke.rs`). Effort: small; `retry_after_seconds` needs a patch or header access. |
| `ExecutionEnv` seam | `ExecutionEnv: FileSystem + Shell` are async traits (`rpi-tools/src/env.rs:145,306,325`); the built-in tools take `Arc<dyn ExecutionEnv>` through `ExecutionToolContext`. Swappable without a fork. `OsExecutionEnv::with_cwd` is enough for `/data/repo`. |
| Session store seam | `rpi-harness` defines `SessionStorage`, `SessionRepo`, `SessionTree` traits (`rpi-harness/src/session/types.rs:1297-1436`); JSONL is one implementation. `rpi-agent` alone needs no store: messages can be passed in via `AgentBuilder::messages`, which is the hook for our own resume. |
| MCP client | None in any `rpi-*` crate (grep). Tools are `AgentTool` (`rpi-agent/src/agent_tool.rs:29`), so factory tools need an `rmcp` bridge: one `AgentTool` per MCP tool with the schema from `tools/list`. Effort: small-medium (task 2.3). |
| Panics in library code | `create_edit_tool` / `create_write_tool` `expect` a mutation-capable context (`rpi-tools/src/tools/edit.rs:91`); fine by construction, noted against the "errors over panics" rule. |
| Sync `StreamFn` | `rpi_agent::StreamFn` is a synchronous `Fn` returning a stream, so an async provider call has to be bridged with `block_in_place` + `block_on` (the crate's own README does this). Needs the multi-thread runtime; works, but is a wart. |
| Features and dependencies | `rpi-ai` HTTP providers are behind feature `providers` (default off), rustls only (`reqwest` with `default-features = false`). Transitive closure of `rpi-ai[providers]` + `rpi-agent` + `rpi-tools` is about 340 crates. `rpi-tools` 0.11.0 pulls `image` with an explicit feature list that includes `exr` (-> `pulp` -> `paste`); its own `[features]` are only `default = []` and `webp`, so a consumer cannot switch `exr` off, which fails `cargo deny check advisories` (RUSTSEC unmaintained `paste`); needs an `[advisories] ignore` entry (RUSTSEC-2024-0436, unmaintained, not a vulnerability, compile-time proc-macro) until fixed upstream, in `rpi-tools`' `image` feature list or in the `exr`/`pulp` chain that uses `paste`; an upstream PR is being prepared. `rpi-harness` additionally pulls `rusqlite` with `bundled` (C build, musl OK but heavy) and `yaml_serde`; not used by the prototype. No TUI or plugin loader in the library crates. Edition 2021, MSRV 1.88. Static musl build not verified here: the installed musl `rust-std` in this sandbox is unusable (`E0463 can't find crate for core`); task 2.4 goes through Bazel. |
| Maintenance | Single repository (`bigfish1913/pi-rust`), MIT, 0.x with frequent releases (0.10.1 -> 0.11.0 within the observed window). Pin with `=0.11.0`. Forking is realistic: library crates are ~84 kLOC including tests, no unsafe (`#![forbid(unsafe_code)]`). |
| Compaction | Only in `rpi-harness` (`compaction/`), coupled to its session tree. Not used; effort estimate belongs in the follow-up. |

**ACP-fit stop point (task 1.3):** the source review shows no no-go on ACP fit. Decision: continue with `rpi-*` (no octolib estimate needed). Open points for the endpoint run: reasoning field name, exact error text of a 429 from the real endpoint.

### Prototype status

- `crates/df-harness` (spike branch): ACP server over stdio, `rpi-agent` loop, tools `bash`/`read`/`write`/`edit` on `OsExecutionEnv` in the session `cwd`, factory MCP bridge (`src/mcp.rs`), model by `DF_HARNESS_BASE_URL`/`DF_HARNESS_API_KEY`/`DF_HARNESS_MODEL`.
- **2.1 verified** against the real spike endpoint (oMLX, `qwen3.6-35b-a3b`): `crates/df-harness/tests/smoke.rs::live_endpoint_streams_thoughts_and_text` (runs with `DF_HARNESS_LIVE_*` set) streams `agent_thought_chunk` and `agent_message_chunk` and ends `end_turn`. The endpoint sends reasoning as `reasoning_content`, which `rpi-ai` reads (D3 answered: no patch needed).
- **2.2 verified** against a fake endpoint (same file): tool call (`tool_call` + `tool_call_update` completed, file written), `session/cancel` during a tool call (`cancelled`), `usage_update` after each turn, 429 -> ACP error `{error: "rate_limited"}`.
- **2.3 verified**: `factory-agent` test `test_df_harness_complete_task_ends_run_completed` starts `df-harness` as the ACP command with the real `factory-agent mcp` bridge; the (fake) model calls `mcp__factory__complete_task` and the run ends `COMPLETED`.
- **2.4 verified**: Bazel (`//crates/df-harness`, `-c opt`, host_musl) yields a static aarch64 musl binary of **18 MB** (glibc cargo release, unstripped: 15.2 MB). `/opt/harness/df-native/bin/df-harness --version` runs in `debian:trixie-slim` and `alpine:3` (Podman, read-only mount). No size optimisation (strip, LTO, `opt-level=s`) was tried.

Real runs (not skipped; no `Skipping` line printed), 2026-10-27:

```
$ cargo build -p df-harness -p factory-agent   # binaries present in target/debug
$ cargo test -p factory-agent test_df_harness_complete_task_ends_run_completed -- --nocapture
test tests::test_df_harness_complete_task_ends_run_completed ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 101 filtered out; finished in 0.10s

$ DF_HARNESS_LIVE_BASE_URL=http://192.168.1.10:1234/v1 DF_HARNESS_LIVE_API_KEY=... \
  DF_HARNESS_LIVE_MODEL=qwen3.6-35b-a3b \
  cargo test -p df-harness --test smoke live_endpoint_streams_thoughts_and_text -- --nocapture
test live_endpoint_streams_thoughts_and_text ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 11.03s
```

Without the binaries or the `DF_HARNESS_LIVE_*` variables both tests return early and pass vacuously; a plain `cargo test` run proves nothing for them.

Gaps found while building (effort items for the follow-up):

- **Streaming granularity:** with a reasoning-only chunk followed by silence, `rpi` emitted no `ThinkingDelta` until the block ended (or the stream errored). Real endpoint streams looked incremental, but a cancel test could not rely on thoughts; the cancel test uses a running tool call.
- **Cancel during a tool call** ends the loop without an `Aborted` assistant message, so the prototype tracks `session/cancel` itself (flag per session).
- **Usage per turn:** `factory-agent` takes per-turn usage from Claude's session log (`usage.rs`); ACP `usage_update` is only context occupancy and is ignored. A native harness needs either the same JSONL log or a runtime change to read ACP usage (`unstable_end_turn_token_usage`). The prototype sends `usage_update` (total tokens, context window).
- **MCP:** the bridge is about 100 lines on `rmcp` (spawns the stdio server from `session/new`, one `AgentTool` per MCP tool, names `mcp__<server>__<tool>`). Only stdio servers; http/sse skipped.
- **Rate limit:** no `Retry-After`; see source review.
- **Codex wire API:** oMLX serves `/v1/chat/completions` and also answers `/v1/responses` (verified with `curl`; its README documents only chat completions and `/v1/messages`). Current Codex only speaks the Responses API, so `config.toml` uses `wire_api = "responses"` with `base_url = http://192.168.1.10:1234/v1`; that worked for all three runs.

### Measurement (tasks 1.1, 3.1)

Method (D4), identical for all three harnesses, files in `measure/`:

- Fixed task: `measure/task/` (a buggy `slugify` plus `test.js`), prompt `measure/PROMPT.txt`. The demo project `iter1-demo-e2e` was not used (it needs the control plane); the task is deliberately small, so the figures are a floor for the workload part.
- One Podman container per run (`docker.io/library/node:24-trixie-slim`, rootless, own cgroup). `measure/sampler.js` is the container entrypoint: it spawns the harness command, samples `VmRSS` of all other processes in the container every 250 ms (tree peak = max of the summed samples; steady = median) and reads `/sys/fs/cgroup/memory.peak` at exit. `measure/drive.py` is the host-side ACP client (auto-approves permissions). Run: `DOCKER_HOST=unix:///run/user/501/podman/podman.sock H=<staging dir> OUT=<dir> measure/run.sh <claude|df-harness|codex|idle> <n>`.
- Commands inside: Claude Code `node claude-agent-acp@0.95.0` + native `claude` 2.1.311 (subscription credentials copied into a throwaway HOME, not committed); `df-harness` static musl binary; Codex `node codex-acp@2.3.0` + its bundled `codex` (0.164) with the oMLX provider.
- Idle baseline (sampler only, `sleep 5`): cgroup `memory.peak` 16.2 MiB. The cgroup figures are warm-cache figures (cold df-harness run: 95 MiB, replaced by the rerun) and the go decision rests on the tree RSS, not on them. cgroup figures include that sampler, the page cache of the mounted binaries and the tool processes.
- Raw results: `measure/results/<harness>-<n>/`. Claude Code runs on its subscription model, `df-harness` and Codex on `qwen3.6-35b-a3b`; the model is deliberately weak, so tokens and outcome are **not meaningful (weak local model)**; "solved" only shows that the tool-call loop works for the fixed task.

| Harness | Peak RSS (tree), 3 runs | Steady RSS (median) | cgroup `memory.peak`, 3 runs (warm cache) | Solved (of 3) | Wall-clock (s), 3 runs | Context used (ACP `usage_update`) |
|---|---|---|---|---|---|---|
| Claude Code (adapter + CLI) | 377 / 452 / 403 MiB | 359 / 365 / 380 MiB | 275 / 381 / 300 MiB | 3 (reference, other model) | 9.3 / 7.0 / 9.3 | ~21.4k context (not comparable) |
| `df-harness` (`rpi-*`) | 10.7 / 10.8 / 10.7 MiB | 9.3 / 9.4 / 9.3 MiB | 22 / 23 / 23 MiB | 3 | 5.4 / 6.7 / 6.9 | not meaningful (weak local model); ~2.0-2.5k context |
| Codex (ACP adapter) | 291 / 294 / 291 MiB | 282 / 289 / 289 MiB | 113 / 117 / 115 MiB | 3 | 12.1 / 7.6 / 5.8 | not meaningful (weak local model); ~10.7k context |

Wall-clock is the ACP client's `wall_seconds` (the models differ, so it is not a harness comparison). "PR content reviewable" (decision criterion in D4) does not apply to this task: it produces a patched file in a scratch directory, no PR.

Memory criterion (D5): worst `df-harness` peak 10.8 MiB against the worst Claude Code peak 452 MiB is **2.4%**, far below the 25% limit. Codex through its adapter costs about 290 MiB (Node adapter plus the native app-server), so the saving comes from the native Rust process, not from the endpoint.

### D5 evaluation (task 3.2)

| Criterion | Result |
|---|---|
| Memory | Go. Worst `df-harness` peak 10.8 MiB vs 452 MiB Claude Code (2.4%); the go decision rests on tree RSS (cgroup peaks, 23 vs up to 381 MiB, are warm-cache figures). |
| ACP fit | Go with gaps. Verified: text and thought chunks (real endpoint), tool calls and updates, `end_turn`, `cancelled`, rate limit as `rate_limited` error, `usage_update`. Open: `Retry-After` (needs a patch or header access), per-turn usage for the runtime (it reads Claude's session log today), `session/load` (not built; the summary fallback exists, `iter2-continuation-summary`). |
| Factory MCP | Go. `rmcp` bridge of about 100 lines; `complete_task` ends the run `COMPLETED` through the real `factory-agent` runtime. |
| Seams | Go. `ExecutionEnv` and `rpi-harness` session traits are replaceable; `rpi-agent` takes messages from outside. |
| Dependencies | Go with effort. No TUI or plugin loader in the library crates; static musl via Bazel works (18 MB, runs on trixie-slim and alpine); ~340 crates; `cargo deny` needs an advisory ignore for `paste` (RUSTSEC-2024-0436, unmaintained, not a vulnerability; via `rpi-tools`' explicit `image` feature list with `exr`, which consumers cannot disable). The ignore stays; the fix belongs upstream and the user is preparing an upstream PR. Plain `rust-std` musl in the sandbox failed (E0463), Bazel is the path. |
| Quality | Go, weakly evidenced. 3/3 solved for `df-harness`, Codex and Claude Code on the small fixed task. The model is a weak local one and the task is trivial, so this shows the loop works, not that it matches Claude Code on real tasks; the prompt and tool set were not tuned. |
| Maintenance | Risk, accepted. One repository, MIT, frequent 0.x releases; pin `=0.x`; forking is realistic (~84 kLOC, `forbid(unsafe_code)`). |

Effort estimate for the follow-up, per open item: compaction (adopt `rpi-harness` compaction or a summary fallback) 2-3 days; session persistence and `session/load` 2-3 days; usage reporting contract (ACP vs session log) 1 day; `Retry-After` 0.5 day; project prompt and tool tuning (output truncation, read/edit behaviour) 3-5 days; sandbox hardening of the `bash` tool (`ExecutionEnv` policy) 1-2 days; image and chart wiring (`harness-images`) 1 day.

**Decision:** build full-harness mode as a separate ACP binary on `rpi-*` (D1, D2), pinned, with the open items above as the follow-up change. octolib is not needed. Quality needs re-checking on a real task with a stronger model before the harness is offered to users.

## Open Questions

- Harness name: decided as `df-native` (ADR-0045, image volume `/opt/harness/df-native`). Only the configuration key is left to the follow-up change.
