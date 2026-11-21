# Proposal: factory-minion

## Why

Three things in the control plane need a short text that only a model can write well:

- Task titles. Today a task keeps the placeholder (first sentence of the description) until the agent calls `set_task_title`, and many agents never do. The `generated` title source is reserved (`ui-redesign` D7) but nothing writes it.
- Persona descriptions. `list_personas` returns empty descriptions for most personas, so planner agents pick personas at random.
- Continuation summaries. The mechanical summary (`df-agent-channel/src/summary.rs`) is a shortened transcript, not a summary. The follow-up change "better summary fallback" needs a model-written summary as a second strategy.

The **factory minion** is one optional, tool-less model call in the control plane for these cases (roadmap backlog row "Factory minion", decided). All model traffic must go through the gateway (ADR-0050), and the control plane has no pod identity there. ADR-0064 (draft in this change) decides how the control plane calls models.

## What Changes

- New capability **factory minion**: one model call with a fixed prompt per *job kind*, no tools, one text output, non-streaming. Input from transcripts, descriptions and system prompts is untrusted data. It is put in a delimited data block and is never the instruction. The output is sanitized per job kind and is only stored as plain text. It is never run, never parsed as commands and never sent to an agent as an instruction.
- **Off by default.** An instance setting selects one model of a configured model provider for the minion. Without it, no minion job runs and everything works as today.
- **Model calls from the control plane go through the gateway (ADR-0064):** the gateway serves a new unary gRPC `GatewayInference.Complete` on its internal mTLS port. The control plane's client certificate is the non-pod identity. The gateway injects the provider credential and measures usage exactly as for runs. The control plane never sends a model credential upstream itself.
- **Usage attribution:** `model_usage` records get a client (`run` or `minion`), and minion records get the job kind plus project, task and persona when the job has them. Run usage stays as it is. The task's usage section shows the minion's usage as a separate line.
- **Generated task titles:** when the minion is on, a task created without a title gets a generated title from its description (source `generated`, existing precedence and `df-core` sanitizing).
- **Generated persona descriptions:** when the minion is on, a persona without a description written by a human gets one generated from its system prompt. **BREAKING (data model):** personas get `description_source` (`none`, `generated`, `user`). A human edit sets `user` and always wins. Only `user` is protected from regeneration.
- **Summary job kind:** the minion offers a `continuation-summary` job that turns transcript events into a structured summary in the sections of Claude Code's compaction. Nothing calls it in this change.
- Failures (minion off, provider down, timeout, empty or rejected output) never block the action that triggered the job. The old value stays, and a warning is logged.

Out of scope:
- Using the summary job as a continuation strategy: the separate change "better summary fallback" (roadmap backlog, minion row).
- A summary posted to the PR of a task a human marked done (`iter2-cancel` D5): stays in the roadmap backlog (minion row). Reason: this is the only minion output that leaves the instance, to a forge that can be public. It needs its own output rules (mentions, links, images, secrets from the transcript) and its own review. The path is rare because `complete_task` asks agents to finish by themselves. It also needs the summary job's quality, which "better summary fallback" checks first.
- A minion model per project: open question in design.md, recommended later (roadmap backlog, minion row).
- Budgets for minion usage: part of the backlog row "Token and cost budgets".
- Tool use, multi-turn conversations or agents in the control plane: rejected (the minion is a single text call by definition).

## Capabilities

### New Capabilities
- `factory-minion`: the minion's configuration, job kinds, prompt framing, output sanitizing, failure behavior and the summary job's structure.

### Modified Capabilities
- `inference-endpoint`: control-plane calls over `GatewayInference.Complete`, identification by client certificate, attribution in usage records.
- `model-usage`: usage records with client `run` or `minion` and minion attribution; minion usage per task.
- `model-providers`: a provider or model the minion uses cannot be deleted or removed.
- `task-management`: generated titles at task creation.
- `configuration-management`: persona description sources and the minion setting.

## Impact

- Code: new crate `df-minion` (prompts, wire-API request and response bodies, sanitizing, jobs); `df-gateway` (`Complete` RPC, mTLS identity of the control plane), `df-gateway-control` (client, provider credential of the minion in the snapshot, usage attribution), `df-db` (migrations, queries), `df-web` (setting, persona form, usage line), `dark-factory` (wiring).
- Contracts: `gateway.proto` (`GatewayInference`, `UsageRecord` attribution).
- Database: `model_usage` changes (nullable `run_id`, client and attribution columns), `personas.description_source`, a table for the instance setting of the minion.
- Docs: `docs/architecture.md` (new crate), ADR-0064, published docs (see tasks).
- Depends on nothing in flight. `persona-identity` changes the persona form and `list_personas`; whichever change lands second rebases its persona form edits.
