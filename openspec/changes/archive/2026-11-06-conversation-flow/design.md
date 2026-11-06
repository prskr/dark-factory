# Design

## Context

See proposal.md for the motivation. Current behavior:

- **Runtime merge** (`crates/factory-agent/src/harness.rs`):
  - `PromptQueue::start_turn` takes every queued prompt when a turn starts and emits one `TurnStarted{prompt_ids}`, then one `UserMessage` per prompt.
  - It sends one ACP `PromptRequest`.
  - `build_acp_prompt_blocks` copies only the text blocks of each prompt. It drops the author and ignores `hidden`, so hidden text reaches the agent.
- **Control plane** (`crates/df-agent-channel/src/session.rs`, `send_new_prompts`):
  - While a turn runs, new messages stay held back (`forwarded_at IS NULL`).
  - On the persisted `TurnEnded`, `pending_prompts` claims all of them and the loop sends one `ControlMessage{Prompt}` per row.
  - The idle runtime wakes on the first one (`idle_until_next_prompt` → `begin_turn`). Between the wake and `start_turn` it awaits a phase send, so whether the other prompts are queued in time is a race.
- **Decisions** (`crates/df-db/src/requests.rs`):
  - `decide` queues a message authored by `Principal::system("request.decided")` with text from `decision_message`.
  - The web UI renders it as a `UserMessage` whose author is the reason (`author_name`, `crates/df-web/src/transcript/mod.rs`).
  - The request card on the timeline already shows the answer and who gave it.
- **Authors of queued messages:**
  - System principals: `task.assigned`, `run.resumed`, `request.decided`, `push.rejected`, and `webhook` (`iter3-webhooks`).
  - Humans and agents already carry a `display_name`.

## Goals / Non-Goals

**Goals:**
- Messages released together always form one turn.
- The agent knows each message's author.
- Humans see a decision once, on its card.

**Non-Goals:**
- Interrupting a turn: change `interrupt-turn`.
- Merging messages across runs (a new run already starts with every undelivered message): already the case, nothing to change.
- Showing an "answered" marker on the agent's `ask_question` tool call row: rejected; the card is the single place.

## Decisions

### D1: One control message per release

`ControlMessage.body` gets `Prompts { repeated Prompt prompts = 1; }` and loses the `Prompt` variant. Its field number is reserved.

`send_new_prompts`:
1. Collects the not-yet-sent prompts from `pending_prompts`.
2. Sends them in one `Prompts`.
3. Records all ids in `sent_prompts`.
4. Sets `busy` once.

On reconnect, re-sending forwarded, undelivered prompts also uses one `Prompts`.

The runtime pushes every prompt of a `Prompts` onto `PromptQueue` under one lock, then wakes the idle wait once. The race is gone because the queue holds all of them before the wake.

Alternatives:
- The runtime waits a short time after the first prompt: rejected, timing-based.
- Keep a single `Prompt` and add a "more follow" flag: rejected, more state for the same result.

### D2: Author name on the prompt

`Prompt` gets `string author_name = 5`, filled by the control plane from `Principal::display_name()` (new, `crates/df-core/src/domain.rs`):

| Principal | Name |
|---|---|
| Human, Agent | its `display_name` |
| System `task.assigned` | Task assignment |
| System `run.resumed` | Resume |
| System `request.decided` | Request decision |
| System `push.rejected` | Push check |
| System `webhook` | Forge events |
| System `task.child_ended` | Subtask update |
| Any other system reason | Dark Factory |

The `webhook` row applies once `iter3-webhooks` is merged.

The runtime doesn't map principals itself; it writes `author_name` as given. One function serves the agent and the web UI, so both use the same wording. Keeping the reason code in `Principal` (and the audit log) is unchanged.

The queued stack lists every undelivered message and prefixes the rows of agents and the system with that name (`Principal::non_human_name`); human rows have no prefix.

### D3: Author line in the harness prompt

`build_acp_prompt_blocks` emits, per prompt in queue order, a text block `"{author_name}:"` followed by the prompt's text blocks. It does this always, even for a single prompt, so the agent's input doesn't depend on how messages were batched.

The continuation summary of a first turn stays in front of everything, unchanged. An empty `author_name` (an older control plane, tests) skips the line.

### D4: Decision messages hidden

`decide` marks every content block of the decision message `hidden: true`. This covers all request kinds, not only questions, because every card shows its decision.

The existing hidden-block rules (task-ui "Hidden content blocks") then:
- keep the message out of the transcript and the queued stack;
- show it, labeled, with `web.showHiddenBlocks`;
- still deliver it to the harness.

Messages stored before this change stay visible. No migration: before the MVP, old transcripts don't matter.

### D5: Request card design

Penpot session first; its outcome sets the card's colors and borders through the existing tokens in `crates/df-web`. The spec only requires the card to be distinct in both themes. The concrete styling is the design session's decision, recorded in the tasks when done.

## Risks / Trade-offs

- [Runtime and control plane must be upgraded together (proto break).] → Both ship in one release and image set. Before the MVP there are no installations to keep compatible.
- [Author lines add tokens to every prompt.] → One short line per message; negligible against the prompt.
- [An agent might echo "Peter Kurfer:" style prefixes back into its output.] → The line names an author and is not a format instruction. Watch e2e transcripts; adjust the wording if needed.

## Migration Plan

No data migration. Deploy the control plane and the runtime image of the same release. Rollback is a rollback of both.
