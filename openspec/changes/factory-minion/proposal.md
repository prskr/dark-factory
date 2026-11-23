# Proposal: factory-minion

## Why

Three things in the control plane need a short text that only a model can write well:

- Task titles. Today a task keeps the placeholder (first sentence of the description) until the agent calls `set_task_title`, and many agents never do. The `generated` title source is reserved (`ui-redesign` D7) but nothing writes it.
- Persona descriptions. `list_personas` returns empty descriptions for most personas, so planner agents pick personas at random.
- Continuation summaries. The mechanical summary (`df-agent-channel/src/summary.rs`) is a shortened transcript, not a summary. The change `better-continuation-summary` (DF-11, in progress) uses a model-written summary as a second strategy.

The **factory minion** is one optional, tool-less model call in the control plane for these cases (roadmap backlog row "Factory minion", decided). All model traffic must go through the gateway (ADR-0050), and the control plane has no pod identity there. ADR-0064 (draft in this change) decides how the control plane calls models.

## What Changes

- New capability **factory minion**: one model call with a fixed prompt per *job kind*, no tools, one text output, non-streaming. Input from transcripts, descriptions and system prompts is untrusted data. It is put in a delimited data block and is never the instruction. The output is sanitized per job kind and is only stored as plain text. It is never run, never parsed as commands and never sent to an agent as an instruction.
- **Off by default.** An instance setting selects one model of a configured model provider for the minion, with an optional reasoning effort for reasoning models. Without it, no minion job runs and everything works as today.
- **Model calls from the control plane go through the gateway (ADR-0064):** the gateway serves a new unary gRPC `Inference.Complete` on its internal mTLS port. It is in its own contract package `darkfactory.inference.v1`, not in the gateway link package `darkfactory.gateway.v1`. The RPC is provider-neutral (instructions, input, token limit in; text out): the gateway builds the provider request from its route, so the control plane knows no wire API, header or credential. The control plane's client certificate is the non-pod identity. The gateway injects the provider credential and measures usage exactly as for runs. The control plane never sends a model credential upstream itself.
- **Usage attribution:** `model_usage` records get a client (`run` or `minion`), and minion records get the job kind plus project, task and persona when the job has them. Run usage stays as it is. The task's usage section shows the minion's usage as a separate line.
- **Generated task titles:** when the minion is on, a task or chat created without a title gets a generated title from its description (source `generated`, existing precedence and `df-core` sanitizing). The agent is then no longer asked to call `set_task_title`. A 🪄 button in the title rename field suggests a title on demand, for example after a failed job.
- **Generated persona descriptions:** a 🪄 button in the description box of the persona form writes a description from the system prompt in the form. The text lands in the box and is stored only when the board member saves. In the background, the minion also fills descriptions that are still empty. It never overwrites text. There is no description source: the field stays as it is.
- **Background jobs are outbox jobs:** generated titles and the filling of empty descriptions are kinds of the existing transactional outbox. They are enqueued with the change that causes them, survive restarts and are retried with backoff. A separate dispatcher keeps slow model calls away from PR sync and lifecycle events.
- **Summary job kind:** the minion offers a `continuation-summary` job that turns transcript events into a structured summary in the sections of Claude Code's compaction. Nothing calls it in this change.
- Failures (minion off, provider down, timeout, empty or rejected output) never block the action that started the job. The old value stays, and a warning is logged. Outbox jobs retry transient failures; the 🪄 button shows the error.

Out of scope:
- Using the summary job as a continuation strategy (project setting, stored summary updated at run end, previous summary as job input): change `better-continuation-summary` (DF-11), which depends on this change.
- A summary posted to the PR of a task a human marked done (`iter2-cancel` D5): stays in the roadmap backlog (minion row). Reason: this is the only minion output that leaves the instance, to a forge that can be public. It needs its own output rules (mentions, links, images, secrets from the transcript) and its own review. The path is rare because `complete_task` asks agents to finish by themselves. It also needs the summary job's quality, which `better-continuation-summary` checks first.
- A minion model per project: later (roadmap backlog, minion row). The minion uses one instance setting.
- Budgets for minion usage: part of the backlog row "Token and cost budgets".
- Tool use, multi-turn conversations or agents in the control plane: rejected (the minion is a single text call by definition).

## Capabilities

### New Capabilities
- `factory-minion`: the minion's configuration, job kinds, prompt framing, output sanitizing, failure behavior and the summary job's structure.

### Modified Capabilities
- `inference-endpoint`: control-plane calls over `Inference.Complete`, identification by client certificate, attribution in usage records.
- `model-usage`: usage records with client `run` or `minion` and minion attribution; minion usage per task.
- `model-providers`: a provider or model the minion uses cannot be deleted or removed.
- `task-management`: generated titles at task creation; no title request in the assignment prompt while the minion is on.
- `chats`: no title request in the first message while the minion is on.
- `task-ui`: the 🪄 button in the title rename field.
- `configuration-management`: the 🪄 button, filling empty persona descriptions, and the minion setting.

## Impact

- Code: new crate `df-minion` (prompts, framing, sanitizing, outbox handlers; no wire API); `df-gateway` (`Complete` RPC, mTLS identity of the control plane, request builders and output-text readers for the three wire APIs, per-call header rendering), `df-gateway-control` (client, provider credential of the minion in the snapshot, usage attribution), `df-db` (migrations, queries), `df-web` (setting, persona form, usage line), `dark-factory` (wiring).
- Contracts: new `contract/proto/darkfactory/inference/v1/inference.proto` (`Inference` service, `MinionCaller`); `gateway.proto` (`Snapshot.minion`, `UsageRecord` attribution, importing `MinionCaller`); `df-proto` module `inference::v1`. New fields in existing messages have placeholder numbers; a task assigns the final ones.
- Database: `model_usage` changes (nullable `run_id`, client and attribution columns), a table for the instance setting of the minion. Minion jobs use the existing `outbox` table.
- Docs: `docs/architecture.md` (new crate), ADR-0064, published docs (see tasks).
- Depends on nothing in flight. Overlaps:
  - `persona-identity` changes the persona form and `list_personas`.
  - `markdown-editor` replaces the persona edit modal with the settings page `/personas/{id}` (description on the General tab) and makes the system prompt optional. "Persona form" in this change means whichever form edits the description when it is implemented.
  - Whichever change lands second rebases its persona form edits.
