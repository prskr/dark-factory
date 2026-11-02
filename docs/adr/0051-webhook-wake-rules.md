# ADR-0051: Webhook wake rules

- **Status:** Accepted (planned for iteration 3, `iter3-webhooks`)
- **Date:** 2026-11-02
- **Related:** amends [ADR-0015](0015-webhook-filtering-and-routing.md); [ADR-0043](0043-delegation-chain-chats-as-tasks.md), [ADR-0049](0049-lifecycle-events-outbox-and-ed25519-signatures.md)

## Context

ADR-0015 leaves open how the webhook secret is stored, who may wake a task, and what happens to the bot's own events. All agents act in the forge through one credential per project, so every agent action, including an agent's review of another task's PR, is authored by the bot. Anyone who can comment on a public repository can also post a review or a mention.

## Decision

- **Secret:** one webhook secret per plugin (`plugins.<name>.webhookSecret`), endpoint `POST /webhooks/{plugin}`. The plugin reads the signature header and the host checks it with HMAC-SHA256. The event's repository must match the project's repository URL.
- **Bot:** reviews and comments by the bot account are dropped, whichever task caused them. Merges, closes and CI failures apply whoever the actor is.
- **Who may wake:** a project setting `webhook_wake` (`none`, `members` default, `anyone`) gates reviews and mentions. Authors' roles come from the plugin.
- **No new task on a mention** for now; a mention on an issue without an active task is dropped.
- **Debounce:** a fixed 30 s window from the first event per task, run through the outbox of ADR-0049.
- **Final tasks don't wake.** `completed`, `cancelled` and `failed` tasks ignore webhook events. A merged PR completes its task.

## Consequences

- Agent-to-agent review wake-ups need a rule in the task dependencies (ADR-0043), not webhooks.
- One secret covers every repository a plugin serves. The repository check keeps a valid signature from crossing projects.
- Outside contributors can't start compute on public repositories unless a project opts in with `anyone`.

## Alternatives considered

- **Recording which task posted each forge comment or review, and dropping only self-caused events:** precise, but every posting tool must record ids, and the dependency model covers the agent case anyway.
- **Secret per project only:** one webhook registration per repository instead of one per org. A per-project override on top of the plugin secret stays on the backlog; it needs a route naming the project, because the secret must be chosen before the payload is trusted.
- **Host-side verification with header name and scheme from config:** moves forge knowledge into operator config.
- **Debounce through a `deliver_after` column on queued messages:** mixes debouncing into message editing rules.
