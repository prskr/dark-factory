# Proposal

## Why

Every sandbox pays the memory overhead of the Claude Code harness: the Node ACP adapter plus the Claude Code CLI. A local measurement of an interactive Claude Code session shows a 260 MB physical footprint (290 MB peak) after 18 minutes. The baseline inside a sandbox has yet to be measured (task 1.1). Memory is the scarce resource per sandbox, so a smaller harness raises the number of concurrent runs per node. Full-harness mode (`product/context.md` §4.1) has so far been rated expensive to build for little extra value. Existing Rust crates change that estimate: `rpi-*` ([pi-rust](https://github.com/bigfish1913/pi-rust), MIT) provides the agent loop, tools, session persistence and compaction, and [octolib](https://github.com/muvon/octolib) (Apache-2.0) provides a provider layer with unified tool calls. This spike measures whether the estimate holds before full-harness mode is scheduled.

## What Changes

- A time-boxed spike (3 days) with a decision at the end: build full-harness mode on `rpi-*`, on octolib, or not for now.
- **Measured:** the memory baseline of the Claude Code harness in a sandbox (adapter and CLI), and the memory of a prototype harness for the same task.
- **Prototype, not merged:** a `df-harness` binary that serves ACP over stdio and runs the `rpi-agent` loop with its own `ExecutionEnv`, model access through an OpenAI-compatible base URL, and the factory MCP server. It lives on the spike branch only.
- **Model access:** an OpenAI-compatible endpoint provided for the spike. It costs no extra API credits and exposes thinking tokens, which the prototype passes on as ACP thought chunks.
- **Compared:** task outcome and token use on the same task. Codex (Rust, via an ACP adapter) runs on the same endpoint and model as the prototype, so only the harness differs, and it is the option that needs no harness code of our own. Claude Code cannot use the endpoint and stays on its subscription as the memory baseline and quality reference.
- **Recorded:** findings and the decision in `design.md`. If the decision is to build, the follow-up change for full-harness mode is drafted, including an ADR on the process boundary (D1).
- No change to runtime behavior, the protocol or the configuration on `main`. The change is not assigned to an iteration.

## Capabilities

### New Capabilities
None. The spike produces findings and a decision, not system behavior. Requirements for full-harness mode follow in the follow-up change (`skip_specs: true`).

### Modified Capabilities
None.

## Impact

- **Code on `main`:** none. The prototype stays on the spike branch.
- **Docs, only if the decision is to build:** `product/context.md` §4.1 (full-harness mode as a separate ACP binary instead of a loop in the runtime, if D1 holds), `product/iterations/roadmap.md` (backlog entry updated), a new ADR.
- **Limitation:** the memory saving applies only to runs billed by API through the inference endpoint (`design/egress.md` §9). Runs on a Claude subscription keep Claude Code (`product/context.md` §1).
- **Dependencies, if the decision is to build:** `rpi-ai`, `rpi-agent`, `rpi-tools` (0.11.0 at the time of the spike, one maintainer) or `octolib`. `cargo deny` and dependency weight are part of the evaluation.
