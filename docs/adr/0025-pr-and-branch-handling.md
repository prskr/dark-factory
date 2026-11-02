# ADR-0025: Pull request and branch handling

- **Status:** Accepted (amended by ADR-0033)
- **Date:** 2026-10-16
- **Related:** [ADR-0009](0009-agent-managed-workspaces-git-durable.md), [ADR-0015](0015-webhook-filtering-and-routing.md)

## Context

Agents produce commits on a task branch, and humans review them as pull requests. Letting agents open PRs themselves risks PRs against wrong bases or repositories.

## Decision

- The control plane opens a draft PR as soon as the runtime reports a pushed task branch ahead of base. The agent only rephrases title/body and comments through tools. When the task completes, the control plane marks the PR ready for review.
- The runtime pushes after every turn with new commits.
- *(Later)* Tasks can start from an existing branch; the task branch is then that branch, the WIP snapshot still goes to `factory/wip/<key>`.
- *(Later)* PR mode per task: create (default), none (mainly for existing branches), attach (link an existing PR).
- The branch → task mapping lives in the database; webhook routing and the Git proxy use it. At most one active task per branch.

## Consequences

- Exactly one PR per task with commits, always against the correct base.
- `design/agent-run.md` §6.2 covers the default branch case only so far.

## Alternatives considered

- **Agents opening PRs via tools**: no guarantee of one PR per task or correct targets.
