# ADR-0033: Branch rewrites, fan-out across branches and merge triggers

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0015](0015-webhook-filtering-and-routing.md), [ADR-0016](0016-runs-and-triggers.md), [ADR-0025](0025-pr-and-branch-handling.md), [ADR-0026](0026-chats-dependencies-project-context.md), [ADR-0028](0028-integrations-wit-capability-interfaces.md)

## Context

Dependency update PRs (e.g. from renovate) pile up when the build breaks or an update needs extra work. Working through them needs three things the design didn't cover: rebasing a branch onto an updated base (a history rewrite, which the runtime and the Git proxy forbade), handling several branches, and ordering — the remaining branches should be rebased onto the base *after* the fix has been merged, not onto the fixed branch.

## Decision

- **Branch rewrite policy per project** for task branches: `never`, `with-lease` (default) or `always`.
  - With `with-lease`, the runtime pushes non-fast-forward updates with `--force-with-lease=<ref>:<expected>`, where `<expected>` is the remote SHA it last fetched or pushed. Commits someone else pushed in between (a human, renovate) are never overwritten; the push fails and the task is marked diverged as before.
  - The Git proxy allows non-fast-forward updates of the task's branch when the policy permits; `receive-pack` already rejects updates whose expected old value no longer matches. The WIP snapshot ref is always force-pushed; base and other branches are never writable.
  - Rewrites are reported (`PushReport`) and visible in the transcript.
- **Several branches are handled by fan-out, not multi-branch tasks:** a coordinating task (or the CEO) creates one child task per branch, each started from that existing branch with PR mode "attach" (ADR-0025). Children run in parallel with their own sandbox, workspace and PR.
- **"PR merged" is a dependency trigger:** a task can be blocked until another task's PR is merged — not only until that task completes (ADR-0016, ADR-0026). The event comes from the routed forge webhooks (ADR-0015). A PR closed without merge doesn't unblock; it queues a message on the dependent task and its creator.
- **Forge capabilities for this flow** (integrations package, ADR-0028): list open pull requests with filters (head prefix, author, label), and CI status plus job logs for a ref or PR — as plugin capabilities, or through a forge MCP server via the gateway (ADR-0032).

## Consequences

- Dependency-update pile-ups can be worked through: fix the root cause, merge it, then fan out over the remaining branches.
- Fan-out costs a cold start and clone per branch; accepted for clear ownership and parallelism.
- Pushing agent commits onto a renovate branch typically makes renovate treat it as modified and stop updating it. For purely mechanical rebases, asking renovate to rebase (its PR checkbox) is often the lighter path; the agent playbook (`product/agent-playbook.md`) describes the pattern.
- The force-with-lease semantics protect concurrent human and bot pushes without extra locking.

## Alternatives considered

- **Never rewrite task branches**: blocks rebases, which are the normal way to bring a dependency branch up to date.
- **Unconditional force pushes**: would overwrite commits pushed by humans or renovate in the meantime.
- **Tasks owning several branches**: one context for all branches, but a new concept for workspace, proxy rules and routing; deferred, may be revisited if fan-out proves too heavy.
- **Dependencies on task completion only**: completion happens before review and merge, so dependents would rebase onto unmerged work.
