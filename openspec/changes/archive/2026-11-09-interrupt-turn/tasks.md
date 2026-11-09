# Tasks

## 1. Protocol

- [x] 1.1 Add `ControlMessage.Interrupt { string by }` to `contract/proto/darkfactory/agent/v1/agent.proto` and `TurnEnded.interrupted_by` to `contract/proto/darkfactory/transcript/v1/transcript.proto`. Verify `cargo build --workspace` and `bazel --bazelrc=.bazelrc.agents build //contract/... //crates/df-proto/...` succeed.

## 2. Runtime

- [x] 2.1 Write failing tests for `determine_run_outcome` (`crates/factory-agent/src/harness.rs`):
  - interrupted with queued prompts → `ContinueNextTurn`;
  - interrupted without → `AwaitInput`, even with stop reason `CANCELLED`;
  - `cancelled` still wins.

  Implement `TurnOutcomeInput.interrupted`; `cargo test -p factory-agent` passes.
- [x] 2.2 Write failing runner tests with `fake-acp-agent`:
  - `Interrupt` during a turn sends `session/cancel`, emits `TurnEnded` with `interrupted_by`, and the run reports `RUN_PHASE_AWAITING_INPUT`;
  - `Interrupt` while idle changes nothing.

  Implement D3 (shared state, `interrupt_rx` branch in `await_prompt`); `cargo test -p factory-agent` passes.
- [x] 2.3 Write a failing test: an open Dark Factory tool call gets a cancelled result on interrupt, and a late `ToolCallResult` is dropped. Implement it; `cargo test -p factory-agent` passes.
- [x] 2.4 Write a failing test: the turn after an interrupt starts with "Your previous turn was interrupted by <name>." before the queued prompts, and counts toward `max_turns`. Implement it in `begin_turn`; `cargo test -p factory-agent` passes.
- [x] 2.5 Add a `fake-acp-agent` script that keeps a turn open until `session/cancel`, for the tests above and the e2e test. `cargo test -p fake-acp-agent` passes.

## 3. Control plane

- [x] 3.1 Write failing df-db tests for `interrupt_turn`:
  - a `running` run → audit entry `run.interrupt` with the actor, and the `RunInterrupt` notification;
  - an `awaiting_input` run or no active run → `NoTurnRunning`, no audit entry.

  Implement it; `cargo test -p df-db` passes. Container-backed: needs a container socket.
- [x] 3.2 Write a failing df-agent-channel test: on `RunInterrupt` for its run, a session with `busy` set sends `Interrupt { by }`, and one without `busy` sends nothing. Implement it; `cargo test -p df-agent-channel` passes.

## 4. Web UI

- [x] 4.1 Write failing df-web tests for `POST /tasks/{key}/interrupt`:
  - permission as for stop;
  - a running turn → 204;
  - otherwise the notice "No turn is running".

  Implement the handler; `cargo test -p df-web` passes.
- [x] 4.2 Write failing df-web render tests:
  - the message box shows "Interrupt" only while the run is `running`;
  - the status switch offers "Stop run";
  - the run row's button reads "Stop run".

  Implement them; `cargo test -p df-web` passes.
- [x] 4.3 Write a failing transcript test: a `TurnEnded` with `interrupted_by` renders the row "Interrupted by <name>". Implement it; `cargo test -p df-web` passes.
- [x] 4.4 Update `docs/content/using/tasks.md` (Interrupt vs. Stop run vs. Cancel task, what the agent is told), `docs/content/architecture/agent-runs.md` (turn interrupt in the run flow, with the state diagram) and the factory-agent docs on control messages. Verify with `just docs-build`.

## 5. Integration

- [ ] 5.1 Add an e2e test in `crates/df-e2e`: queue a message during a long turn, press Interrupt; the transcript shows "Interrupted by …", and the next turn of the same run starts with the interrupt line and the message. Run `just e2e-local`.
- [x] 5.2 Clippy without warnings, `cargo-machete`, `openspec validate interrupt-turn --strict`, and `bazel --bazelrc=.bazelrc.agents test //...` pass.
