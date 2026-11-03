# ADR-0025: Pull request and branch handling

- **Status:** Accepted (amended by ADR-0033 and on 2026-11-03: existing branches, attached PRs, stored branch)
- **Date:** 2026-10-16
- **Related:** [ADR-0009](0009-agent-managed-workspaces-git-durable.md), [ADR-0015](0015-webhook-filtering-and-routing.md)

## Context

Agents produce commits on a task branch, and humans review them as pull requests. Letting agents open PRs themselves risks PRs against wrong bases or repositories.

## Decision

- The control plane opens a draft PR as soon as the runtime reports a pushed task branch ahead of base. The agent only rephrases title/body and comments through tools. When the task completes, the control plane marks the PR ready for review.
- The runtime pushes after every turn with new commits.
- *(Later)* Tasks can start from an existing branch; the task branch is then that branch, the WIP snapshot still goes to `factory/wip/<key>`.
  Amended 2026-11-03 (iteration 3): set through `create_task` (`branch`) or the new-task form. The project's default branch and `factory/` branches are rejected; `branch` and `base_task` exclude each other. The branch's existence is checked at the first start: a missing branch fails the task.
- *(Later)* PR mode per task: create (default), none (mainly for existing branches), attach (link an existing PR).
  Amended 2026-11-03: no PR mode. A PR is attached when it was found instead of created: at creation, exactly one open PR with the branch as head attaches it and its base becomes the task's base (more than one fails `create_task`; none gives the default branch); later, `find-open` before creating the PR attaches as well. The factory never changes an attached PR's title, body or draft state (no "ready" on Completion, no "back to draft"); the task may comment, and a merge makes it Done. `none` is not needed: a task without commits opens no PR.
- The branch → task mapping lives in the database; webhook routing and the Git proxy use it. At most one active task per branch.
  Amended 2026-11-03: every task stores its branch (also `factory/task/<key>`), unique per project among tasks that are not Done, failed or cancelled; `in_review` holds the branch. `create_task` fails naming the task that holds it. Webhook routing looks the branch up instead of parsing it.

## Consequences

- Exactly one PR per task with commits, always against the correct base.
- `design/agent-run.md` §6.2 covers the default branch case only so far.

## Alternatives considered

- **Agents opening PRs via tools**: no guarantee of one PR per task or correct targets.
