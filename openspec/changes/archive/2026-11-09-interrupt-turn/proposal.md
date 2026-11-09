# Proposal

## Why

The only way to stop an agent that is going the wrong way is to stop the whole run. That ends the harness session, pushes, writes a WIP snapshot and needs a new run for the correction. Board members need to interrupt the current turn and steer the agent with a message, in the same run.

## What Changes

- **New `Interrupt` control message.** The runtime ends the current turn through ACP `session/cancel` and keeps the run.
  - With messages waiting, the next turn starts right away.
  - Otherwise the run waits for input as after a normal turn end.
- **The agent learns about it.** The next turn's input starts with "Your previous turn was interrupted by <name>.". Dark Factory tool calls still open get a cancelled result. Nothing already done is rolled back.
- **The transcript shows it.** An interrupted turn ends with a row "Interrupted by <name>".
- **Best effort.** An interrupt is not stored. Without a running turn, or with the runtime not connected, the board member gets the notice "No turn is running" and nothing changes.
- **Permissions and audit.** Same permission as stopping a run. Audited as `run.interrupt`.
- **It doesn't count against the run.** An interrupted turn counts as a normal turn: not a failure, and it doesn't use an automatic resume.
- **BREAKING (UI):** the message box's "Stop" button becomes **Interrupt**, shown only while a turn runs. Stopping the run moves to **Stop run** in the status switch and stays in the run row.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `acp-harness`: new requirement "Interrupting a turn".
- `run-orchestration`: new requirement "Interrupting a turn from the control plane".
- `task-ui`:
  - "Stop and cancel actions" adds Interrupt and moves "Stop run".
  - New requirement "Interrupted turns in the transcript".

## Impact

- **Agent protocol:** `contract/proto/darkfactory/agent/v1/agent.proto` gets `ControlMessage.Interrupt { string by }`. Runtime and control plane must be deployed together.
- **Runtime:** `crates/factory-agent`:
  - `handle_control_message`
  - `await_prompt` (cancel only the turn)
  - `determine_run_outcome` (an interrupted turn awaits input)
  - pending tool calls
  - the next turn's input
- **Control plane:**
  - `crates/df-agent-channel`: the session sends `Interrupt` when notified.
  - `crates/df-db`: `interrupt_turn` with the audit entry and notification.
  - `crates/df-web`: endpoint, buttons, transcript row.
- **Docs:**
  - `docs/content/using/tasks.md`
  - `docs/content/architecture/agent-runs.md`
  - the factory-agent docs
- **Out of scope:**
  - Merging held-back messages into one turn, and author lines: change `conversation-flow`, which this change builds on for "messages waiting start the next turn".
  - A "send and interrupt" action on a held-back message: backlog (`product/iterations/roadmap.md`) if the plain Interrupt proves too slow to steer.
