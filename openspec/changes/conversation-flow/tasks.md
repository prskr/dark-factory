# Tasks

## 1. Display names of principals

- [x] 1.1 Write failing unit tests for `Principal::display_name()` in `crates/df-core/src/domain.rs`: human and agent return their `display_name`, each system reason of design D2 returns its name, an unknown reason returns "Dark Factory". Implement it; `cargo test -p df-core` passes.
- [x] 1.2 Use `display_name()` in the web UI's `author_name` (`crates/df-web/src/transcript/mod.rs`) and in the queued message stack. Add a df-web test showing "Task assignment" instead of `task.assigned`; `cargo test -p df-web` passes.

## 2. One control message per release

- [x] 2.1 In `contract/proto/darkfactory/agent/v1/agent.proto`:
  - Add `Prompts { repeated Prompt prompts }` to `ControlMessage.body`.
  - Remove the `Prompt` variant and reserve its field number.
  - Add `Prompt.author_name`.

  Verify `cargo build --workspace` and `bazel --bazelrc=.bazelrc.agents build //contract/... //crates/df-proto/...` succeed.
- [x] 2.2 Write a failing df-agent-channel test: two messages queued during a turn are sent in one `Prompts` after the persisted `TurnEnded`, in queue order. Change `send_new_prompts` and the reconnect re-send to build one `Prompts`, and fill `author_name` from `display_name()` in `prompt_from_row`. `cargo test -p df-agent-channel` passes.
- [x] 2.3 Write a failing factory-agent test: a `Prompts` with two prompts arriving while idle starts exactly one turn whose `TurnStarted.prompt_ids` lists both. In `handle_control_message`, push all prompts under one lock and wake once. `cargo test -p factory-agent` passes.
- [x] 2.4 Update `crates/fake-acp-agent` scripts and the e2e helpers that send or assert single `Prompt` messages. Verify `cargo test -p fake-acp-agent` passes and `cargo test -p df-e2e --no-run` compiles.
  - Verified no singular gRPC consumers in these crates; the ACP prompt API is unchanged.

## 3. Author lines in the harness prompt

- [x] 3.1 Write failing tests for `build_acp_prompt_blocks` (`crates/factory-agent/src/harness.rs`):
  - Each prompt starts with `"{author_name}:"`, in queue order, also for a single prompt.
  - An empty `author_name` adds no line.
  - The continuation summary stays first.

  Implement it; `cargo test -p factory-agent` passes.
- [x] 3.2 Document the author line and the `Prompts` message in `crates/factory-agent/docs/` (the page describing prompts and turns) and in `docs/content/architecture/agent-runs.md`. Verify with `just docs-build`.

## 4. Decision messages hidden

- [x] 4.1 Write a failing df-db test: `requests::decide` queues the decision message with every content block `hidden: true`, for a question and for an approval. Implement it in `crates/df-db/src/requests.rs`; `cargo test -p df-db` passes. Container-backed: needs a container socket.
- [x] 4.2 Add a df-web test: an answered question's transcript shows the card with the answer and no separate answer message; with `web.showHiddenBlocks` the message is shown labeled. `cargo test -p df-web` passes.
- [x] 4.3 Update `docs/content/using/requests.md` (where an answer appears) and `docs/content/using/tasks.md` (author names of system messages). Verify with `just docs-build`.

## 5. Request card design

- [x] 5.1 Design session in Penpot: request cards (pending and decided, question and approval) on the task timeline in the light and the dark theme, distinct from messages and tool calls. Record the chosen tokens and states in this task when done.
  - Approved by the user on 2026-11-06: `color.sunk` / `bg-sunk`, `color.border-strong` / `border-line-strong`, existing `rounded-xl` (12 px), 16 px padding and 12 px gaps. Keep existing typography, badges, suggested answers, scope radios and action buttons.
  - States inspected in Light and Dark: question pending/answered, approval pending/approved/denied. Decided cards retain the outcome and decider, without decision actions. No new palette tokens or interaction behavior.
  - Reference: `App Design`, page `Tasks`, boards `Desktop · conversation flow · question pending`, `question answered`, `approval pending`, `approval approved` and `approval denied`. The `Preview only` boards are comparison snapshots, not separate theme masters.
- [x] 5.2 Implement the design in `crates/df-web` with the existing color tokens. Verify with Playwright screenshots of a pending and an answered question in both themes, and a df-web render test for the card's state classes.

## 6. Integration

- [x] 6.1 Run the e2e suite (`just e2e-local`): a message queued during a turn and an answered question both reach the agent in one turn with author lines. List any container-backed tests that couldn't run.
- [x] 6.2 Clippy without warnings, `cargo-machete`, `openspec validate conversation-flow --strict`, and `bazel --bazelrc=.bazelrc.agents test //...` pass.
