# Proposal

## Why

Messages that wait for the same turn should reach the agent as one input, but today two gaps break that:

- **Turns can split.** After a turn ends, the control plane forwards the held-back messages as separate `Prompt` control messages. The idle runtime starts a turn on the first one, so the others may land in the next turn.
- **The agent can't see who wrote what.** The runtime drops the author of every message. In a merged turn, a human's request, an answer to a question and a system note look the same.

Answers to questions also show twice in the transcript: on the question card, and as a separate message whose author reads `request.decided`.

## What Changes

- **BREAKING (agent protocol):** the control plane forwards all messages it releases at once in one `Prompts` control message, replacing the `Prompt` body. The runtime keeps merging every message waiting at a turn start into one harness prompt.
- **Author lines:** every message in a turn's input starts with a line naming its author (for example `Peter Kurfer:` or `Request decision:`). The control plane computes the name and sends it with the message.
- **Readable system names:** system authors show readable names in the transcript and the queued message stack instead of their reason code. Unknown reasons show "Dark Factory".
- **Decision messages hidden:** the message that delivers a request's decision is hidden in the transcript, like the title instruction. The agent still receives it, and the request card on the timeline shows the decision.
- **Request card design:** a design session in Penpot for request cards that stand out in both themes. Today they are hard to spot in the light theme. Its result is implemented in this change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `acp-harness`: "Turn-based prompts" adds the author line in front of each message.
- `run-orchestration`: "Prompts during a run" forwards the released messages together.
- `agent-requests`: "Decisions are delivered as messages" marks the message hidden.
- `task-ui`: new requirement "Readable system authors".
- `request-inbox`: "Requests on the task timeline" requires cards that stand out from messages in both themes.

## Impact

- **Agent protocol:** `contract/proto/darkfactory/agent/v1/agent.proto` gets a `Prompts` control message and a `Prompt.author_name` field. Runtime and control plane must be deployed together; before the MVP there is no compatibility shim.
- **Runtime:** `crates/factory-agent` handles `Prompts` and renders author lines in `build_acp_prompt_blocks`.
- **Control plane:**
  - `crates/df-agent-channel` (`send_new_prompts`, `pending_prompts`)
  - `crates/df-core` (display name of a `Principal`)
  - `crates/df-db` (`requests::decide` marks the decision message hidden)
- **Web UI:** `crates/df-web` (`author_name` in the transcript and queued stack; request card styling).
- **Tests:** `crates/fake-acp-agent` scripts and the e2e suite, wherever they check prompt content.
- **Out of scope:**
  - Interrupting a turn: change `interrupt-turn`.
  - Dropping the webhook debounce: review of `iter3-webhooks`.
