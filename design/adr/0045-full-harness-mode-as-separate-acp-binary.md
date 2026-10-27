# ADR-0045: Full-harness mode as a separate ACP binary

- **Status:** Proposed (spike `native-harness-spike`; becomes Accepted with the change `full-harness-mode`)
- **Date:** 2026-10-27
- **Related:** ADR-0005, ADR-0022, ADR-0027, ADR-0029

## Context

`product/context.md` §4.1 described full-harness mode as an agent loop inside the sandbox runtime. The Claude Code harness costs about 450 MiB peak per sandbox (Node adapter plus CLI). The spike `native-harness-spike` built a prototype (`df-harness`, `rpi-agent` loop, ACP over stdio) that peaks at 11 MiB for the same fixed task (tree RSS; cgroup peaks of 23 MiB against up to 381 MiB are warm-cache figures and not the basis of the decision), is a static musl binary of 18 MB, and ran the factory `complete_task` tool through the existing MCP bridge to a `COMPLETED` run.

## Decision

Full-harness mode is a separate binary that serves ACP over stdio, started by the runtime like any other harness and delivered as an image volume (`/opt/harness/df-native`, ADR-0022). The loop is built on the `rpi-*` crates, pinned. It reaches the model through the OpenAI-compatible inference endpoint (ADR-0029) and the factory tools through the MCP servers the runtime passes in `session/new` (ADR-0027). Gaps in ACP are closed with `_meta` or extension methods, not with a second integration path.

## Consequences

- The runtime keeps one integration path (transcript, resume, budgets, outcomes) for all harnesses and keeps its mTLS credentials out of the process that handles model output.
- Per-turn usage needs a contract: the runtime reads Claude's session log today; the native harness must provide usage through ACP or the same log format.
- The harness owns compaction, session persistence and prompts; effort per item is estimated in the spike findings.
- Subscription runs stay on Claude Code; the saving applies to API-billed runs only.
- Dependency weight (~340 crates) and a single-maintainer upstream are accepted; forking is the fallback.

## Alternatives considered

- **Loop inside `factory-agent`:** needs a second implementation of transcript translation, resume, turns and budgets, and puts model output handling in the process with the control-plane credentials.
- **octolib:** solves only the provider layer; loop, tools and persistence would be our code. Not needed after the spike.
- **Codex through its ACP adapter:** works, but costs about 290 MiB per sandbox, so it saves little.
