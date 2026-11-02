# ADR-0052: Dependencies gate the first start, later rounds go through the parent

- **Status:** Accepted (planned for iteration 3, `iter3-task-dependencies`)
- **Date:** 2026-11-02
- **Related:** amends [ADR-0016](0016-runs-and-triggers.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0033](0033-branch-rewrites-fan-out-and-merge-triggers.md), [ADR-0041](0041-tasks-based-on-another-tasks-branch.md), [ADR-0043](0043-delegation-chain-chats-as-tasks.md); [ADR-0051](0051-webhook-wake-rules.md)

## Context

Pattern 2.2 (agent playbook) runs several rounds: the implementer completes, the reviewer reviews, the implementer addresses the comments and completes again, the reviewer reviews again. ADR-0051 drops the bot's own reviews, so webhooks can't carry the reviewer's result back to the implementer. ADR-0041 restarted the reviewer on every new Completion of its base task.

Two automatic rules (a reviewer ending wakes the reviewed task, a new Completion restarts the dependent) form a loop: the reviewer approves, the implementer wakes with nothing to do, completes, and the reviewer runs again. Breaking it needs a guard such as "only on new commits", which then misses rounds without commits.

The parent already wakes on every child that ends (ADR-0043) and decides what is left to do.

## Decision

- **Dependencies gate only the first start.** A task with dependencies (`depends_on`, kinds `completion` and `done`) stays `awaiting_dependencies` until all of them are satisfied, then starts once. Dependencies are set at creation, never changed, and reference existing tasks of the same project, so cycles can't form.
- **`completion`** is satisfied by any past Completion of the target (the agent called `complete_task`), even if the target runs again later, and by Done. **`done`** is satisfied when the target is Done: its PR was merged or a board member marked it done.
- **Later rounds go through the parent.** No dependency restarts a task that already started, and no task wakes because a related task ended, except its parent. The parent wakes on a child's Completion, Done, failure or cancellation and sends a message to the child that has to act (`send_message`, own children only, with the same rules as a board member's message). A parent that is already final is not woken.
- **Handover between tasks never depends on webhooks.** Completion, parent wake-ups and messages are internal. Only forge facts (a PR merged or closed) come from the forge; without webhooks a `done` dependency is released by marking the target done by hand.
- **A failed or cancelled dependency** leaves the dependent waiting; the parent decides. A PR closed without merge doesn't release it either; the PR's own task is told (ADR-0051).

## Consequences

- No loop and no commit heuristics: the parent turn that decides the next round is already paid for, since it wakes on every child end.
- Every relay costs a parent turn. Without a parent (a human created both tasks without one), the human sends the next round.
- Chats and coordinators carry the review loop in their prompts; the playbook describes it.
- `awaiting_dependencies` tasks have no branch, volume or sandbox before their first start.

## Alternatives considered

- **A reviewing task ending wakes the reviewed task (`base_task` rule):** loops with the restart rule, see Context.
- **Restart the dependent on every Completion of its target:** loops or needs a "new commits" guard that misses rounds.
- **Restart only on new commits:** a round that answers comments without commits is never reviewed.
- **Polling the forge for merges and closes:** duplicates webhooks; forge facts come from the forge.
